# Session and Simulator as Message-Driven State Machines (v2)

Draft design for formalizing the game session and simulator into
message-driven state machines — structs whose fields only change in
response to typed messages processed through a single entry point.

**Supersedes:** `session_state_machine.md` (v1). Kept for comparison.

**Key differences from v1:** No finite-state-machine with named states
and transition diagrams. The session is a struct with fields (players,
optional sim, paused flag, speed). Messages mutate those fields. The
"state" is just the totality of field values at any point. Single-player
uses a local relay for tick pacing (same path as multiplayer).
Networking is external to the session. Save files remain sim-only. The
session supports having no sim loaded as a normal part of its lifecycle.

---

## 1. What "State Machine" Means Here

Not a finite-state automaton with named states and an explicit transition
table. In the Turing machine sense: a deterministic system that processes
a stream of typed messages, where identical message streams produce
identical results. The session's "state" is its complete set of field
values — `players`, `sim`, `paused`, `speed`, and so on. The number of
reachable states is astronomically large (as large as SimState's state
space), but the *interface* is a small, well-typed message enum.

The important properties:

- **All mutation goes through messages.** No direct field writes from
  outside. If you want to change speed, send a `SetSpeed` message.
- **Deterministic.** Two sessions processing the same message stream
  from the same initial state produce identical results.
- **Testable.** Feed messages in, inspect fields. No mock networking,
  no frame loop, no Godot.

---

## 2. GameSession

A Rust struct in `elven_canopy_sim` (no Godot dependency). Owns all
session-level state and optionally a simulator.

```rust
/// A game session: the shared context among players. Contains zero or
/// one simulators, player information, and session-level settings.
///
/// All mutation goes through `process()`. The session is a pure
/// message-processing struct — networking, rendering, and I/O are
/// external concerns that feed messages in and read state out.
pub struct GameSession {
    /// Connected players. Always non-empty (at least the local player
    /// in single-player). BTreeMap for deterministic iteration.
    pub players: BTreeMap<PlayerId, PlayerSlot>,

    /// The host (whoever can start/load/unload games).
    pub host_id: PlayerId,

    /// The simulation, if one is loaded. `None` when no game is
    /// running (pre-start, between games, after unload).
    pub sim: Option<SimState>,

    /// Whether the session is paused. When true, AdvanceTo messages
    /// are rejected. Commands can still be buffered.
    pub paused: bool,

    /// Who paused the session (for UI display).
    pub paused_by: Option<PlayerId>,

    /// Current sim speed (how fast time advances relative to wall
    /// clock). Only meaningful when a sim is loaded and not paused.
    pub speed: SessionSpeed,

    /// Commands received but not yet applied to the sim. Flushed on
    /// the next AdvanceTo.
    pub pending_commands: Vec<SimCommand>,

    /// Seed for the current or next game. Set during configuration,
    /// used when starting a new game.
    pub seed: u64,

    /// Game config for the current or next game.
    pub config: GameConfig,
}

pub struct PlayerSlot {
    pub id: PlayerId,
    pub name: String,
    pub is_local: bool,
}

/// Session speed. No "Paused" variant — pausing is a separate boolean.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionSpeed {
    Normal,   // 1x
    Fast,     // 2x
    VeryFast, // 5x
}
```

### Why `Option<SimState>`

A session is a room. Players join, chat, pick settings. Then someone
starts a game (sim gets created) or loads a save (sim gets deserialized).
They play. They might save, unload, load a different save. The sim comes
and goes; the session persists.

This means "no sim loaded" is a normal, supported state — not an error
or initialization artifact. The session just has `sim: None` and rejects
messages that require a sim (AdvanceTo, SimCommand) until one is loaded.

### Why `paused` is a field, not a state

A paused session is identical to an unpaused session except that
`AdvanceTo` is rejected. The same players are connected. The same
commands can be buffered (they'll apply when the game resumes). The same
queries work. Making "paused" a separate state would duplicate every
field and complicate ownership.

`paused: bool` is simple, correct, and sufficient. The `process()`
method checks it in the `AdvanceTo` handler. Done.

---

## 3. SessionMessage — The Input Alphabet

Every mutation to `GameSession` goes through one of these:

```rust
/// A typed message that drives the session. In single-player, produced
/// locally. In multiplayer, ordered and broadcast by the relay.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionMessage {
    /// A player has connected to the session.
    PlayerJoined { id: PlayerId, name: String },

    /// A player has disconnected.
    PlayerLeft { id: PlayerId },

    /// Start a new game with the session's current seed and config.
    /// Creates a fresh SimState. Includes initial setup (creature
    /// spawning) so all clients produce identical initial state.
    StartGame,

    /// Load a sim from serialized state (save file or snapshot).
    /// Replaces any existing sim.
    LoadSim { json: String },

    /// Unload the current sim. Returns to the "no game" state.
    UnloadSim,

    /// A simulation command from a player. Buffered until the next
    /// AdvanceTo. Rejected if no sim is loaded.
    SimCommand { player_id: PlayerId, action: SimAction },

    /// Advance the simulation to the given tick, flushing all
    /// buffered commands. Rejected if no sim, or if paused.
    AdvanceTo { tick: u64 },

    /// Change the simulation speed.
    SetSpeed { speed: SessionSpeed },

    /// Pause the session (stop advancing time).
    Pause { by: PlayerId },

    /// Resume the session.
    Resume { by: PlayerId },
}
```

### Message processing

```rust
impl GameSession {
    /// Process a message. Returns events for the UI/log.
    ///
    /// This is the single entry point for all mutation.
    pub fn process(&mut self, msg: SessionMessage) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        match msg {
            SessionMessage::PlayerJoined { id, name } => {
                self.players.insert(id, PlayerSlot {
                    id, name: name.clone(), is_local: false,
                });
                events.push(SessionEvent::PlayerJoined { name });
            }

            SessionMessage::PlayerLeft { id } => {
                if let Some(slot) = self.players.remove(&id) {
                    events.push(SessionEvent::PlayerLeft { name: slot.name });
                }
            }

            SessionMessage::StartGame => {
                let mut sim = SimState::with_config(self.seed, self.config.clone());
                // Initial creature spawning — data-driven from config so
                // all clients produce identical results.
                sim.spawn_initial_creatures();
                self.sim = Some(sim);
                self.paused = false;
                self.pending_commands.clear();
                events.push(SessionEvent::GameStarted);
            }

            SessionMessage::LoadSim { json } => {
                match SimState::from_json(&json) {
                    Ok(sim) => {
                        self.sim = Some(sim);
                        self.paused = false;
                        self.pending_commands.clear();
                        events.push(SessionEvent::SimLoaded);
                    }
                    Err(e) => {
                        events.push(SessionEvent::Error {
                            message: format!("Failed to load sim: {e}"),
                        });
                    }
                }
            }

            SessionMessage::UnloadSim => {
                self.sim = None;
                self.paused = false;
                self.pending_commands.clear();
                events.push(SessionEvent::SimUnloaded);
            }

            SessionMessage::SimCommand { player_id, action } => {
                if self.sim.is_some() {
                    self.pending_commands.push(SimCommand {
                        player_id,
                        // Tick assigned at flush time (AdvanceTo), not here.
                        tick: 0,
                        action,
                    });
                }
                // Silently dropped if no sim. Could emit an error event
                // if we want stricter validation.
            }

            SessionMessage::AdvanceTo { tick } => {
                if self.paused {
                    return events; // Reject: session is paused.
                }
                if let Some(sim) = &mut self.sim {
                    // Assign tick to all pending commands.
                    for cmd in &mut self.pending_commands {
                        cmd.tick = tick;
                    }
                    let result = sim.step(&self.pending_commands, tick);
                    self.pending_commands.clear();
                    // Wrap sim events as session events.
                    for sim_event in result.events {
                        events.push(SessionEvent::Sim(sim_event));
                    }
                }
            }

            SessionMessage::SetSpeed { speed } => {
                self.speed = speed;
                events.push(SessionEvent::SpeedChanged { speed });
            }

            SessionMessage::Pause { by } => {
                if !self.paused {
                    self.paused = true;
                    self.paused_by = Some(by);
                    events.push(SessionEvent::Paused { by });
                }
            }

            SessionMessage::Resume { by } => {
                if self.paused {
                    self.paused = false;
                    self.paused_by = None;
                    events.push(SessionEvent::Resumed { by });
                }
            }
        }
        events
    }

    /// Convenience: does the session currently have a sim?
    pub fn has_sim(&self) -> bool {
        self.sim.is_some()
    }

    /// Current sim tick, or 0 if no sim is loaded.
    pub fn current_tick(&self) -> u64 {
        self.sim.as_ref().map_or(0, |s| s.tick)
    }

    /// Speed multiplier for wall-clock → sim-tick conversion.
    pub fn speed_multiplier(&self) -> f64 {
        if self.paused { return 0.0; }
        match self.speed {
            SessionSpeed::Normal => 1.0,
            SessionSpeed::Fast => 2.0,
            SessionSpeed::VeryFast => 5.0,
        }
    }
}
```

---

## 4. SessionEvent — Output

```rust
/// Events produced by processing a SessionMessage. For UI display,
/// logging, and notification.
pub enum SessionEvent {
    PlayerJoined { name: String },
    PlayerLeft { name: String },
    GameStarted,
    SimLoaded,
    SimUnloaded,
    SpeedChanged { speed: SessionSpeed },
    Paused { by: PlayerId },
    Resumed { by: PlayerId },
    /// A sim-level event (creature arrived, build completed, etc.).
    Sim(SimEvent),
    /// Something went wrong.
    Error { message: String },
}
```

---

## 5. Sim Changes

### Removing speed from SimState

Speed is a session concern. The sim shouldn't know or care how fast it's
being ticked — it just processes commands and advances to whatever tick
it's told.

- Remove `speed: SimSpeed` from `SimState`.
- Remove `SimAction::SetSimSpeed` from the command enum.
- Remove `SimEventKind::SpeedChanged` from sim events.
- The `SimSpeed` type can be removed entirely (replaced by
  `SessionSpeed` at the session level).

### Separating command enqueue from time advancement

Currently `apply_or_send()` in `SimBridge` calls `sim.step()` with a
1-tick advance as a side effect of applying a command. This goes away.

The new flow: commands are buffered in `GameSession.pending_commands`.
When `AdvanceTo` arrives, all pending commands get their tick assigned
and `sim.step()` is called once with the full batch.

The existing `sim.step(&[SimCommand], target_tick)` API works perfectly
for this — it already accepts a batch of commands and a target tick. No
changes needed to `step()` itself.

Optionally, we could add `enqueue_command()` / `advance_to()` methods on
SimState for cases where someone wants to use the sim without a session.
But since the session handles buffering, this isn't strictly needed. The
existing `step()` is the right primitive.

### Initial creature spawning

Currently, initial creature spawning (5 elves, 5 capybaras, etc.) is
duplicated in `main.gd` (single-player `_ready()` and
`_on_mp_game_started()`). This moves into the sim, triggered by
`StartGame`:

```rust
impl SimState {
    /// Spawn the initial set of creatures for a new game.
    /// Called by GameSession::process(StartGame).
    ///
    /// Uses data from config (initial_creatures list) so it's
    /// deterministic and not hardcoded.
    pub fn spawn_initial_creatures(&mut self) {
        // Spawn creatures from config.initial_creatures list.
        // Each entry: { species, count, food_pct, rest_pct, bread_count }
        // All spawning uses the sim's PRNG for determinism.
    }
}
```

This means `GameConfig` gains an `initial_creatures` field describing
what to spawn at game start. Moves hardcoded spawn logic from GDScript
into data-driven config.

---

## 6. Tick Pacing: Local Relay for Single-Player

Single-player and multiplayer use the same tick-pacing path: a relay
(or relay-like component) that converts wall-clock time into
`AdvanceTo` messages.

### Why same path

- One code path to test and maintain.
- Multiplayer bugs that only appear under specific tick pacing are
  caught in single-player too.
- The replay system (future) records `SessionMessage` streams. If SP
  and MP produce the same message types with the same semantics,
  replays work identically.

### How it works

The relay (real or local) is the tick authority. It decides "the sim
should now be at tick N" and produces `AdvanceTo { tick: N }`.

**Multiplayer relay:** Runs on a timer. Every turn interval (e.g., 50ms
at 1x speed), it broadcasts a `Turn` message with `sim_tick_target`. The
gdext bridge translates this to `AdvanceTo { tick }`.

**Local relay (single-player):** A Rust struct that receives
`delta_seconds` each frame, maintains an accumulator, and produces
`AdvanceTo` messages at the appropriate rate. It lives in the gdext
crate (since it needs frame deltas from Godot) but implements the same
tick-pacing logic as the real relay.

```rust
/// Local tick pacer for single-player. Converts wall-clock deltas
/// into AdvanceTo messages at the appropriate rate for the current
/// session speed.
///
/// Lives in elven_canopy_gdext, not in the session (which is pure
/// logic with no wall-clock concept).
pub struct LocalRelay {
    /// Fractional seconds of unprocessed sim time.
    accumulator: f64,
    /// Seconds per sim tick (from config, typically 0.001).
    seconds_per_tick: f64,
    /// Maximum ticks per frame (spiral-of-death cap).
    max_ticks_per_frame: u64,
}

impl LocalRelay {
    /// Called each frame. Returns an AdvanceTo message if ticks should
    /// advance, or None if no advancement needed this frame.
    pub fn update(
        &mut self,
        delta: f64,
        speed_multiplier: f64,
        current_tick: u64,
    ) -> Option<SessionMessage> {
        self.accumulator += delta * speed_multiplier;
        // Cap to prevent unbounded growth.
        if self.accumulator > 0.1 {
            self.accumulator = 0.1;
        }
        let ticks = (self.accumulator / self.seconds_per_tick) as u64;
        let ticks = ticks.min(self.max_ticks_per_frame);
        if ticks > 0 {
            self.accumulator -= ticks as f64 * self.seconds_per_tick;
            Some(SessionMessage::AdvanceTo {
                tick: current_tick + ticks,
            })
        } else {
            None
        }
    }

    /// Return a fractional render tick for smooth interpolation.
    /// current_tick + accumulator_fraction.
    pub fn render_tick(&self, current_tick: u64) -> f64 {
        current_tick as f64 + (self.accumulator / self.seconds_per_tick)
    }
}
```

### Render tick interpolation

Both paths need smooth creature interpolation between ticks. The
`LocalRelay` provides `render_tick()` directly from its accumulator.
In multiplayer, the gdext bridge computes render_tick from
time-since-last-turn (same logic as current `_mp_time_since_turn`).

A `RenderTickComputer` trait or enum could unify this, but it's simple
enough that two implementations (local accumulator vs. multiplayer
interpolation) are fine.

---

## 7. Networking Is External

`GameSession` is pure logic. It doesn't know about TCP, relays, or wire
protocols. The gdext bridge (`SimBridge`) sits between the network and
the session:

```
                    ┌─────────────────┐
                    │   SimBridge     │
                    │   (gdext)       │
                    │                 │
  GDScript ────────▶  Translates     │
  (UI actions)     │  UI → Session   │
                    │  Messages       │
                    │                 │
  NetClient ───────▶  Translates     │
  (wire msgs)      │  Wire → Session │
                    │  Messages       │
                    │        │        │
                    │        ▼        │
                    │  ┌───────────┐  │
                    │  │GameSession│  │
                    │  │  (pure)   │  │
                    │  └───────────┘  │
                    │        │        │
                    │   SessionEvents │
                    │        │        │
                    │        ▼        │
                    │  → GDScript     │
                    │  → NetClient    │
                    └─────────────────┘
```

### Why external

- **Testability.** Tests create a `GameSession`, feed it messages, and
  inspect state. No mock sockets needed.
- **Separation of concerns.** The session is the game logic. The network
  is I/O. Mixing them creates the kind of scattered state we're trying
  to eliminate.
- **Flexibility.** The same session processes messages regardless of
  source: local relay, remote relay, replay file, test harness.

### Translation layer (SimBridge)

`SimBridge` translates between the wire protocol
(`ClientMessage`/`ServerMessage`) and `SessionMessage`:

```rust
// In SimBridge::poll_network():
for msg in self.net_client.poll() {
    match msg {
        ServerMessage::Turn { sim_tick_target, commands, .. } => {
            // Each command in the turn → SessionMessage::SimCommand
            for tc in commands {
                if let Ok(action) = serde_json::from_slice(&tc.payload) {
                    session.process(SessionMessage::SimCommand {
                        player_id: /* map relay ID → sim PlayerId */,
                        action,
                    });
                }
            }
            // Then advance to the turn's target tick.
            session.process(SessionMessage::AdvanceTo {
                tick: sim_tick_target,
            });
        }
        ServerMessage::GameStart { seed, config_json } => {
            session.seed = seed as u64;
            session.config = parse_config(&config_json);
            session.process(SessionMessage::StartGame);
        }
        ServerMessage::Paused { by } => {
            session.process(SessionMessage::Pause {
                by: map_player_id(by),
            });
        }
        ServerMessage::Resumed { by } => {
            session.process(SessionMessage::Resume {
                by: map_player_id(by),
            });
        }
        ServerMessage::SnapshotLoad { data, .. } => {
            if let Ok(json) = String::from_utf8(data) {
                session.process(SessionMessage::LoadSim { json });
            }
        }
        // PlayerJoined, PlayerLeft, etc. map directly.
        _ => { /* ... */ }
    }
}
```

For outgoing messages, `SimBridge` translates player actions into both
a `SessionMessage` (for local processing in single-player) and a
`ClientMessage` (for sending to the relay in multiplayer):

```rust
// Player clicks "spawn elf":
fn spawn_creature(&mut self, species, x, y, z) {
    let action = SimAction::SpawnCreature { species, position };
    if self.is_multiplayer() {
        // Send to relay. It'll come back in a Turn.
        self.net_client.send_command(&serde_json::to_vec(&action));
    } else {
        // Process locally.
        self.session.process(SessionMessage::SimCommand {
            player_id: self.local_player_id,
            action,
        });
    }
}
```

---

## 8. SimBridge Refactoring

### What moves into GameSession

| Current location           | New location                   |
|---------------------------|-------------------------------|
| `SimBridge.sim`           | `GameSession.sim`             |
| `SimBridge.is_multiplayer_mode` | Implicit: has `net_client`  |
| `SimBridge.is_host`       | `GameSession.host_id == local_id` |
| `SimBridge.game_started`  | `GameSession.has_sim()`       |
| `SimBridge.mp_ticks_per_turn` | Relay config (external)    |
| `SimState.speed`          | `GameSession.speed`           |

### What stays in SimBridge

```rust
pub struct SimBridge {
    base: Base<Node>,
    /// The session: pure game logic.
    session: GameSession,
    /// Local tick pacer (single-player). None in multiplayer.
    local_relay: Option<LocalRelay>,
    /// Network client (multiplayer). None in single-player.
    net_client: Option<NetClient>,
    /// Embedded relay server handle (host mode only).
    relay_handle: Option<RelayHandle>,
    /// Chunk mesh cache — rendering concern, not session state.
    mesh_cache: Option<MeshCache>,
    /// Buffered events for GDScript (from SessionEvents).
    pending_events: Vec<String>,
    /// Local player ID.
    local_player_id: PlayerId,
}
```

### Key methods

```rust
#[godot_api]
impl SimBridge {
    /// Called each frame by GDScript. Advances time and returns the
    /// fractional render tick for interpolation.
    #[func]
    fn frame_update(&mut self, delta: f64) -> f64 {
        // 1. Poll network (multiplayer).
        if let Some(client) = &self.net_client {
            // Translate wire messages → SessionMessages, process them.
        }

        // 2. Local tick pacing (single-player).
        if let Some(relay) = &mut self.local_relay {
            let mult = self.session.speed_multiplier();
            let tick = self.session.current_tick();
            if let Some(msg) = relay.update(delta, mult, tick) {
                self.session.process(msg);
            }
            return relay.render_tick(self.session.current_tick());
        }

        // 3. Multiplayer render tick interpolation.
        // (compute from time-since-last-turn)
        self.session.current_tick() as f64
    }

    /// All sim queries delegate through the session.
    #[func]
    fn get_creature_positions(&self, species: GString, render_tick: f64)
        -> PackedVector3Array
    {
        let Some(sim) = &self.session.sim else {
            return PackedVector3Array::new();
        };
        // ... same as current code, using sim instead of self.sim ...
    }
}
```

---

## 9. GDScript Changes

### main.gd simplification

The biggest win: `_process()` becomes trivial.

```gdscript
func _process(delta: float) -> void:
    var bridge: SimBridge = $SimBridge
    if not bridge.has_sim():
        return

    # One call: handles network polling, tick pacing, and returns
    # render_tick for interpolation. Same in SP and MP.
    var render_tick := bridge.frame_update(delta)

    # Distribute render_tick to renderers (unchanged).
    $ElfRenderer.set_render_tick(render_tick)
    $CapybaraRenderer.set_render_tick(render_tick)
    for r in _extra_renderers:
        r.set_render_tick(render_tick)
    _selector.set_render_tick(render_tick)

    # Refresh renderers (unchanged).
    # ...
```

Removed from main.gd:
- `_sim_accumulator` — now in `LocalRelay`.
- `_mp_time_since_turn` — now in multiplayer render tick logic (Rust).
- `_seconds_per_tick` — now in `LocalRelay`.
- The single-player vs. multiplayer branching in `_process()`.
- Initial creature spawning from `_ready()` and `_on_mp_game_started()`
  — now in `SimState::spawn_initial_creatures()`.

### game_session.gd

Unchanged in structure. Still carries menu → game transition data (seed,
tree profile, load path, multiplayer config). These are read once by
`main.gd`'s `_ready()` and used to configure the Rust `GameSession`.

---

## 10. Save Files

Save files remain sim-only (`SimState` serialized to JSON), as they are
today. The session is a transient context (a group of friends playing
together) — it's not persisted to disk.

For future extensibility, saves can optionally use an envelope:

```json
{
  "version": 1,
  "sim": { ... existing sim JSON ... }
}
```

With backward compatibility: if the top-level JSON has a `tick` field,
it's a legacy sim-only save; if it has a `version` field, it's an
envelope. This lets us attach metadata later (screenshot thumbnail,
session info, mod list) without breaking existing saves.

For multiplayer synchronization (mid-game join), the session serializes
the sim via `sim.to_json()` and sends it through the relay. The
receiving client processes `SessionMessage::LoadSim { json }`. This
is the same operation as loading a save file — the session doesn't
distinguish between "loaded from disk" and "loaded from network."

---

## 11. Determinism

### What's synchronized in multiplayer

The relay orders and broadcasts `SessionMessage`s (translated from the
wire protocol by each client's `SimBridge`). All clients process the
same messages in the same order, producing identical `GameSession` state.

Since `GameSession` is deterministic and all fields are derived from the
message stream, session-level state (speed, paused, players) is
automatically synchronized. No extra bookkeeping.

### Desync detection

Keep the current approach: sim-only checksums via `state_checksum()`
(FNV-1a of JSON serialization), compared by the relay every 1000 ticks.
Session-level state is small and directly controlled by relay messages —
if the relay delivers messages correctly, session state can't diverge
independently.

If we later discover session-level desync bugs, we can extend the
checksum to include session fields. But for now, the sim is the only
complex state that can diverge.

---

## 12. Test Plan

All tests in `elven_canopy_sim` — no Godot dependency needed. Use the
existing `test_config()` pattern (small 64³ world).

### 12.1 Message processing basics

**PlayerJoined / PlayerLeft:**
- Create a session. Send `PlayerJoined`. Assert player is in
  `session.players`.
- Send `PlayerLeft` for that player. Assert they're removed.
- Send `PlayerLeft` for a nonexistent player. Assert no crash, no
  change.

**StartGame:**
- Create a session with seed 42 and config. Send `StartGame`.
- Assert `session.has_sim()` is true.
- Assert `session.sim.unwrap().tick == 0`.
- Assert initial creatures exist (from `spawn_initial_creatures()`).

**LoadSim / UnloadSim:**
- Create a session. Send `StartGame`. Advance to tick 1000.
- Serialize the sim: `let json = session.sim.unwrap().to_json()`.
- Send `UnloadSim`. Assert `session.has_sim()` is false.
- Send `LoadSim { json }`. Assert sim is back, tick is 1000.

**UnloadSim resets pending commands:**
- Buffer some SimCommands. Send `UnloadSim`. Assert pending_commands
  is empty.

### 12.2 Command buffering and tick advancement

**Commands don't apply until AdvanceTo:**
- Send `StartGame`. Send `SimCommand(SpawnCreature)`.
- Assert creature does NOT exist yet (command is buffered).
- Send `AdvanceTo { tick: 1 }`. Assert creature now exists.

**Multiple commands flush together:**
- Buffer two spawn commands. Send `AdvanceTo`.
- Assert both creatures exist.

**Commands get their tick from AdvanceTo:**
- Buffer a command. Send `AdvanceTo { tick: 500 }`.
- The command should have been applied at tick 500 (verify via sim
  state or event timestamps).

**AdvanceTo with no sim is a no-op:**
- Create a session with no sim. Send `AdvanceTo`.
- Assert no crash, no events.

### 12.3 Pause / resume

**Pause blocks AdvanceTo:**
- Start a game. Send `Pause`.
- Send `AdvanceTo`. Assert sim tick doesn't change.
- Send `Resume`. Send `AdvanceTo`. Assert sim advances.

**Commands buffer while paused:**
- Start a game. Send `Pause`.
- Send `SimCommand(SpawnCreature)`.
- Assert command is in pending_commands.
- Send `Resume`. Send `AdvanceTo`.
- Assert creature now exists.

**Double pause is a no-op:**
- Pause. Pause again. Assert `paused` is still true, only one
  `Paused` event emitted.

**Resume while not paused is a no-op:**
- Assert no `Resumed` event.

### 12.4 Speed

**SetSpeed changes multiplier:**
- `SetSpeed { Normal }` → `speed_multiplier() == 1.0`.
- `SetSpeed { Fast }` → `speed_multiplier() == 2.0`.
- `SetSpeed { VeryFast }` → `speed_multiplier() == 5.0`.
- While paused: `speed_multiplier() == 0.0` regardless of speed.

**Speed doesn't affect sim logic:**
- Run two sessions with identical messages but different speed changes
  interspersed. Assert checksums are identical after the same number
  of AdvanceTo ticks. (Speed only affects wall-clock pacing, not sim
  results.)

### 12.5 Determinism

**Two sessions, same messages:**
- Create two sessions with seed 42.
- Feed identical message streams: StartGame, various SimCommands,
  multiple AdvanceTo messages.
- Assert `sim.state_checksum()` matches after every AdvanceTo.

**Determinism across pause/resume/speed changes:**
- Session A: StartGame, advance to tick 1000 straight.
- Session B: StartGame, advance to 500, pause, resume, change speed,
  advance to 1000.
- Assert identical checksums (pause/resume/speed don't affect sim).

### 12.6 Save/load round-trip

- Create session, start game, advance to tick 5000 with some commands.
- Serialize sim: `json = sim.to_json()`.
- Create new session, `LoadSim { json }`.
- Advance both by 1000 more ticks (same commands).
- Assert identical checksums.

### 12.7 Edge cases

- **AdvanceTo with tick == current_tick:** No-op, no crash.
- **AdvanceTo with tick < current_tick:** No-op or error event. The sim
  must never go backward.
- **SimCommand with no sim loaded:** Silently dropped.
- **StartGame when sim already loaded:** Replaces the sim (acts like
  UnloadSim + StartGame).
- **LoadSim with invalid JSON:** Error event, sim unchanged.
- **Rapid pause/resume (10x):** Assert consistent state.
- **10,000 ticks with no commands:** Sim processes heartbeats, tree
  growth, etc. normally.

---

## 13. File-by-File Change Plan

### New files

| File | Purpose |
|------|---------|
| `elven_canopy_sim/src/session.rs` | `GameSession`, `SessionMessage`, `SessionEvent`, `SessionSpeed`, `PlayerSlot`. Core message-processing struct + tests. |

### Modified: Rust

**`elven_canopy_sim/src/lib.rs`**
- Add `pub mod session;`
- Re-export `GameSession`, `SessionMessage`, `SessionSpeed`,
  `SessionEvent`.

**`elven_canopy_sim/src/sim.rs`**
- Remove `speed: SimSpeed` field from `SimState`.
- Remove the `SetSimSpeed` match arm from `apply_command()`.
- Remove `SpeedChanged` event emission from `apply_command()`.
- Add `spawn_initial_creatures()` method (data-driven from config).
- Keep `step()` unchanged — it's the right primitive for the session
  to call.

**`elven_canopy_sim/src/command.rs`**
- Remove `SimAction::SetSimSpeed` variant.
- Update docstring.

**`elven_canopy_sim/src/event.rs`**
- Remove `SimEventKind::SpeedChanged` variant.
- Update docstring.

**`elven_canopy_sim/src/types.rs`**
- Remove `SimSpeed` enum (or keep as a deprecated alias if anything
  external depends on it — check all references). `SessionSpeed`
  replaces it.

**`elven_canopy_sim/src/config.rs`**
- Add `initial_creatures: Vec<InitialCreatureSpec>` to `GameConfig`.
  Each spec: `{ species, count, food_pct, rest_pct, bread_count }`.
  Defaults to current hardcoded values.

**`elven_canopy_gdext/src/sim_bridge.rs`**
- Replace `sim: Option<SimState>` with `session: GameSession`.
- Add `local_relay: Option<LocalRelay>`.
- Remove `is_multiplayer_mode`, `is_host`, `game_started`,
  `mp_ticks_per_turn` fields.
- Rewrite `apply_or_send()` → `session.process(SimCommand { ... })`
  (single-player) or `net_client.send_command()` (multiplayer).
- Rewrite `step_to_tick()` → `session.process(AdvanceTo { tick })`.
- Rewrite `set_sim_speed()` → `session.process(SetSpeed { ... })`.
- Add `frame_update(delta) -> f64` method that handles tick pacing
  and returns render_tick.
- Rewrite `poll_network()` to translate wire messages →
  SessionMessages.
- All sim query methods delegate through `session.sim`.
- Update docstring.

**`elven_canopy_gdext/src/sim_bridge.rs` (new struct)**
- Add `LocalRelay` struct (accumulator-based tick pacer).

### Modified: GDScript

**`godot/scripts/main.gd`**
- Unify `_process()`: call `bridge.frame_update(delta)` instead of
  separate SP/MP branches.
- Remove `_sim_accumulator`, `_mp_time_since_turn`, `_seconds_per_tick`.
- Remove initial creature spawning from `_ready()` and
  `_on_mp_game_started()` (now in `spawn_initial_creatures()`).
- `_ready()` simplified: configure session, send StartGame or LoadSim,
  then `_setup_common()`.

**`godot/scripts/action_toolbar.gd`**
- No changes. Still emits `speed_changed` signal. Handler in main.gd
  calls `bridge.set_sim_speed()`, which calls
  `session.process(SetSpeed)`.

**`godot/scripts/game_session.gd`**
- No structural changes. Still carries menu → game transition data.

### Modified: Documentation

**`CLAUDE.md`**
- Update project structure to mention `session.rs`.
- Update "Implementation Status" to reflect session architecture.
- Update "Codebase Patterns and Gotchas" to replace `apply_or_send`
  side-effect note with session message model.
- Remove `SimSpeed::Paused` references.

**`docs/design_doc.md`**
- Update §4 to describe the session message model.
- Note that speed/pause are session concerns.

**`docs/tracker.md`**
- Update F-session-sm status as work progresses.

---

## 14. Migration Phases

### Phase 1: GameSession struct + tests

Create `session.rs` with `GameSession`, `SessionMessage`,
`SessionEvent`, `SessionSpeed`. Implement `process()`. Write all tests
from section 12. `SimState.speed` and `SimAction::SetSimSpeed` still
exist but are unused by the session. The session manages speed
independently.

This is a pure addition — no existing code changes. Everything compiles
and tests pass alongside the old code.

### Phase 2: Initial creature spawning

Add `initial_creatures` to `GameConfig`. Implement
`spawn_initial_creatures()` on `SimState`. Update `GameSession::process(StartGame)` to call it. Update session tests to verify initial
creatures. Still no integration with SimBridge — old code paths work.

### Phase 3: Wire GameSession into SimBridge

Replace `SimBridge.sim` with `SimBridge.session`. Add `LocalRelay`.
Rewrite the key methods (`apply_or_send`, `step_to_tick`,
`set_sim_speed`, `poll_network`). All sim queries go through
`session.sim`. Add `frame_update()`.

This is the big integration step. Both SP and MP paths now go through
the session.

### Phase 4: Simplify GDScript

Unify `_process()` in main.gd. Remove accumulator fields. Remove
initial creature spawning from GDScript.

### Phase 5: Remove SimSpeed from sim

Remove `speed: SimSpeed` from `SimState`. Remove
`SimAction::SetSimSpeed`. Remove `SimEventKind::SpeedChanged`. Remove
the `SimSpeed` type. Update all references. Clean up dead code in
SimBridge.

### Phase 6: Documentation

Update CLAUDE.md, design_doc.md, tracker.md.
