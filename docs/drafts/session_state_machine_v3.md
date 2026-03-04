# Session as a Message-Driven Struct (v3)

Draft design for formalizing the game session as a message-driven struct
whose fields only change in response to typed messages processed through
a single entry point.

**Supersedes:** `session_state_machine_v2.md`. Kept for comparison.
**Also see:** `session_state_machine.md` (v1) for the original FSM-based
approach that started this line of thinking.

**Key differences from v2:**

- No "state" or "state machine" framing at all. GameSession is a struct
  with fields. Messages mutate fields through `process()`. The "state"
  is just the totality of field values at any point.
- PlayerId resolution is explicit: sim PlayerId (128-bit UUID for tree
  ownership) vs. session-level player tracking (reuses relay's
  `RelayPlayerId` as `SessionPlayerId`).
- `StartGame` carries `seed` and `config` as parameters.
- Concrete `GameSession::new()` constructor for both single-player and
  multiplayer bootstrapping.
- `AdvanceTo` backward-tick guard that rejects without clearing
  pending commands.
- Relay simplified to a pure message orderer with no game-level
  semantics.
- Initial creature setup moved entirely into sim-side
  `spawn_initial_creatures()` with data-driven per-creature config.
- Direct-mutation methods (`set_creature_food`, `set_creature_rest`,
  `add_creature_item`, `add_ground_pile_item`) become `SimAction`
  variants.
- Delayed command application audit results documented.
- All command ticks assigned at flush time from `AdvanceTo`, not at
  enqueue time.
- Concrete test additions for multiplayer translation, LocalRelay math,
  spawn determinism, concurrent commands, and `PartialEq` derivations.

---

## 1. Design Philosophy

GameSession is a Rust struct. It has fields. You call `process(msg)` to
change those fields. There are no named states, no transition diagrams,
no finite-state-machine concepts. The struct just has an optional sim, a
paused flag, a speed, a player list, and a command buffer.

The important properties:

- **All mutation goes through messages.** No direct field writes from
  outside. If you want to change speed, send a `SetSpeed` message. If
  you want to start a game, send `StartGame { seed, config }`.
- **Deterministic.** Two sessions processing the same message stream
  from the same initial state produce identical results.
- **Testable.** Feed messages in, inspect fields. No mock networking,
  no frame loop, no Godot.

---

## 2. Player ID Resolution

The codebase has two distinct player identity types that serve different
purposes:

### Sim's PlayerId (types.rs)

A `SimUuid` -- 128-bit UUID generated from the sim's PRNG. There is
currently only one per `SimState` (at `sim.player_id`). It represents
**tree ownership** -- the identity of the tree spirit. In shared-tree
multiplayer, all commands use the same `sim.player_id` because all
players control the same tree. This is a sim-level concept tied to the
deterministic state.

### Relay's RelayPlayerId (protocol/types.rs)

A compact `u32` assigned by the relay server when a client connects.
Used for wire protocol efficiency. Relay-scoped -- meaningless outside
a single relay session.

### Session's SessionPlayerId

For session-level player tracking (who's connected, who paused, who
sent what command), GameSession uses its own lightweight type that
reuses the relay's representation:

```rust
/// Session-level player identifier. In single-player, the local player
/// gets ID 0. In multiplayer, IDs are assigned by the relay (matching
/// RelayPlayerId values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
         Serialize, Deserialize)]
pub struct SessionPlayerId(pub u32);

impl SessionPlayerId {
    pub const LOCAL: SessionPlayerId = SessionPlayerId(0);
}
```

### How they relate

- `GameSession.players` is keyed by `SessionPlayerId`.
- `SimCommand.player_id` uses the sim's `PlayerId` (tree owner UUID).
- When processing a `SessionMessage::SimCommand`, the session looks up
  the sim's `player_id` from the loaded `SimState` and uses that for
  the actual `SimCommand`. The `SessionPlayerId` on the message is for
  session-level attribution (logging, UI display), not sim-level
  identity.
- `SimBridge` maintains the mapping between `RelayPlayerId` (from wire
  messages) and `SessionPlayerId` (which are the same values, just
  different types for clarity).

---

## 3. GameSession

A Rust struct in `elven_canopy_sim` (no Godot dependency). Owns all
session-level fields and optionally a simulator.

```rust
/// A game session: the shared context among players. Contains zero or
/// one simulators, player information, and session-level settings.
///
/// All mutation goes through `process()`. Networking, rendering, and
/// I/O are external concerns that feed messages in and read fields out.
pub struct GameSession {
    /// Connected players. Always non-empty (at least the local player
    /// in single-player). BTreeMap for deterministic iteration.
    pub players: BTreeMap<SessionPlayerId, PlayerSlot>,

    /// The host (whoever can start/load/unload games).
    pub host_id: SessionPlayerId,

    /// The simulation, if one is loaded. `None` when no game is
    /// running (pre-start, between games, after unload).
    pub sim: Option<SimState>,

    /// Whether the session is paused. When true, AdvanceTo messages
    /// are rejected. Commands can still be buffered.
    pub paused: bool,

    /// Who paused the session (for UI display).
    pub paused_by: Option<SessionPlayerId>,

    /// Current sim speed (how fast time advances relative to wall
    /// clock). Only meaningful when a sim is loaded and not paused.
    pub speed: SessionSpeed,

    /// Commands received but not yet applied to the sim. Flushed on
    /// the next AdvanceTo.
    pub pending_commands: Vec<PendingAction>,
}

/// A buffered action waiting for flush. Carries the session player ID
/// for attribution and the sim action to apply.
pub struct PendingAction {
    pub from: SessionPlayerId,
    pub action: SimAction,
}

pub struct PlayerSlot {
    pub id: SessionPlayerId,
    pub name: String,
    pub is_local: bool,
}

/// Session speed. No "Paused" variant -- pausing is a separate boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionSpeed {
    Normal,   // 1x
    Fast,     // 2x
    VeryFast, // 5x
}
```

### Constructor

```rust
impl GameSession {
    /// Create a new single-player session with one local player.
    pub fn new_singleplayer() -> Self {
        let local = SessionPlayerId::LOCAL;
        let mut players = BTreeMap::new();
        players.insert(local, PlayerSlot {
            id: local,
            name: "Player".to_string(),
            is_local: true,
        });
        Self {
            players,
            host_id: local,
            sim: None,
            paused: false,
            paused_by: None,
            speed: SessionSpeed::Normal,
            pending_commands: Vec::new(),
        }
    }

    /// Create a new multiplayer session. The local player is added via
    /// PlayerJoined after the relay assigns an ID. The host_id is set
    /// when the first player joins (or by the relay's Welcome message).
    pub fn new_multiplayer(local_id: SessionPlayerId) -> Self {
        let mut players = BTreeMap::new();
        players.insert(local_id, PlayerSlot {
            id: local_id,
            name: String::new(), // filled by PlayerJoined
            is_local: true,
        });
        Self {
            players,
            host_id: local_id,
            sim: None,
            paused: false,
            paused_by: None,
            speed: SessionSpeed::Normal,
            pending_commands: Vec::new(),
        }
    }
}
```

### Why `Option<SimState>`

A session is a room. Players join, pick settings. Then someone starts a
game (sim gets created) or loads a save (sim gets deserialized). They
play. They might save, unload, load a different save. The sim comes and
goes; the session persists.

`sim: None` is a normal, supported configuration -- not an error or
initialization artifact. The session just rejects messages that require
a sim (`AdvanceTo`, `SimCommand`) until one is loaded.

### Why `paused` is a field, not a separate struct variant

A paused session is identical to an unpaused session except that
`AdvanceTo` is rejected. The same players are connected. The same
commands can be buffered (they'll apply when the game resumes). The same
queries work. Making "paused" a separate variant would duplicate every
field and complicate ownership.

`paused: bool` is simple, correct, and sufficient. The `process()`
method checks it in the `AdvanceTo` handler. Done.

---

## 4. SessionMessage -- The Input Alphabet

Every mutation to `GameSession` goes through one of these:

```rust
/// A typed message that drives the session. In single-player, produced
/// locally. In multiplayer, ordered and broadcast by the relay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionMessage {
    /// A player has connected to the session.
    PlayerJoined { id: SessionPlayerId, name: String },

    /// A player has disconnected.
    PlayerLeft { id: SessionPlayerId },

    /// Start a new game with the given seed and config. Creates a
    /// fresh SimState. All mutation through messages -- seed and config
    /// are parameters, not read from session fields.
    StartGame { seed: u64, config: GameConfig },

    /// Load a sim from serialized state (save file or snapshot).
    /// Replaces any existing sim.
    LoadSim { json: String },

    /// Unload the current sim. Returns to the "no game" configuration.
    UnloadSim,

    /// A simulation command from a player. Buffered until the next
    /// AdvanceTo. Rejected if no sim is loaded.
    SimCommand { from: SessionPlayerId, action: SimAction },

    /// Advance the simulation to the given tick, flushing all
    /// buffered commands. Rejected if no sim, if paused, or if
    /// tick <= current sim tick.
    AdvanceTo { tick: u64 },

    /// Change the simulation speed.
    SetSpeed { speed: SessionSpeed },

    /// Pause the session (stop advancing time).
    Pause { by: SessionPlayerId },

    /// Resume the session.
    Resume { by: SessionPlayerId },
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

            SessionMessage::StartGame { seed, config } => {
                let mut sim = SimState::with_config(seed, config);
                // Initial creature spawning -- data-driven from config so
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

            SessionMessage::SimCommand { from, action } => {
                if self.sim.is_some() {
                    self.pending_commands.push(PendingAction {
                        from,
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
                    // Backward tick guard: reject if tick is not
                    // strictly greater than the sim's current tick.
                    // Do NOT clear pending_commands on rejection --
                    // they should persist for the next valid AdvanceTo.
                    if tick <= sim.tick {
                        return events;
                    }
                    // Build SimCommands from pending actions. All
                    // commands get their tick from the AdvanceTo
                    // target -- assigned at flush time, not enqueue
                    // time. This is a behavioral change from the
                    // current "apply at tick+1" pattern.
                    let sim_player_id = sim.player_id;
                    let commands: Vec<SimCommand> = self.pending_commands
                        .drain(..)
                        .map(|pa| SimCommand {
                            player_id: sim_player_id,
                            tick,
                            action: pa.action,
                        })
                        .collect();
                    let result = sim.step(&commands, tick);
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

    /// Speed multiplier for wall-clock -> sim-tick conversion.
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

### Key details in `AdvanceTo`

**Backward tick guard:** If `tick <= sim.tick`, the message is rejected
silently. Pending commands are preserved (not cleared) so they apply on
the next valid `AdvanceTo`. This prevents accidental time reversal and
handles edge cases like duplicate network messages gracefully.

**Command tick assignment:** All pending commands receive the
`AdvanceTo` message's target tick. This is different from the current
`apply_or_send()` pattern which assigns `sim.tick + 1` at enqueue time.
The new behavior matches multiplayer semantics where the relay's `Turn`
message assigns the tick, and makes single-player and multiplayer
command timing identical.

**Sim player ID:** All commands use `sim.player_id` regardless of which
session player sent them. In the current game, all players share a
single tree, so there is only one sim-level player identity. The
`SessionPlayerId` on the original `SimCommand` message is retained for
logging and attribution but not passed into the sim.

---

## 5. SessionEvent -- Output

```rust
/// Events produced by processing a SessionMessage. For UI display,
/// logging, and notification.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    PlayerJoined { name: String },
    PlayerLeft { name: String },
    GameStarted,
    SimLoaded,
    SimUnloaded,
    SpeedChanged { speed: SessionSpeed },
    Paused { by: SessionPlayerId },
    Resumed { by: SessionPlayerId },
    /// A sim-level event (creature arrived, build completed, etc.).
    Sim(SimEvent),
    /// Something went wrong.
    Error { message: String },
}
```

Note: `PartialEq` is derived on both `SessionEvent` and
`SessionMessage` to enable clean test assertions without custom
comparison logic.

---

## 6. Sim Changes

### Removing speed from SimState

Speed is a session concern. The sim shouldn't know or care how fast it's
being ticked -- it just processes commands and advances to whatever tick
it's told.

- Remove `speed: SimSpeed` from `SimState`.
- Remove `SimAction::SetSimSpeed` from the command enum.
- Remove `SimEventKind::SpeedChanged` from sim events.
- The `SimSpeed` type can be removed entirely (replaced by
  `SessionSpeed` at the session level).

Removing `speed` from `SimState` changes serialization format. Save
compatibility at this stage of development is not a concern.

### Separating command enqueue from time advancement

Currently `apply_or_send()` in `SimBridge` calls `sim.step()` with a
1-tick advance as a side effect of applying a command. This goes away.

The new flow: commands are buffered in `GameSession.pending_commands`.
When `AdvanceTo` arrives, all pending commands get their tick assigned
and `sim.step()` is called once with the full batch.

The existing `sim.step(&[SimCommand], target_tick)` API works perfectly
for this -- it already accepts a batch of commands and a target tick. No
changes needed to `step()` itself.

### Direct mutations become SimAction variants

The following `SimBridge` methods currently bypass the command system
with direct sim state mutation:

| Method | What it does | Current pattern |
|--------|-------------|-----------------|
| `set_creature_food()` | directly writes `creature.food` | `sim.creatures.get_mut(&id).food = value` |
| `set_creature_rest()` | directly writes `creature.rest` | `sim.creatures.get_mut(&id).rest = value` |
| `add_creature_item()` | directly modifies `creature.inventory` | inserts/updates inventory entries |
| `add_ground_pile_item()` | directly modifies `sim.ground_piles` | creates or updates a ground pile |

These must become `SimAction` variants processed through the command
system:

```rust
// New SimAction variants:
SimAction::SetCreatureFood {
    creature_id: CreatureId,
    food: i64,
},
SimAction::SetCreatureRest {
    creature_id: CreatureId,
    rest: i64,
},
SimAction::AddCreatureItem {
    creature_id: CreatureId,
    item_kind: ItemKind,
    quantity: i32,
},
SimAction::AddGroundPileItem {
    position: VoxelCoord,
    item_kind: ItemKind,
    quantity: i32,
},
```

In practice, these four actions are only used during initial game setup.
Once `spawn_initial_creatures()` (section 7) is implemented, they could
be simplified to internal setup actions rather than full `SimAction`
variants. However, making them proper commands first ensures they go
through the deterministic command path, which is the right ordering --
we can simplify later if they're truly never needed as player-facing
commands.

### Initial creature spawning

Currently, initial creature spawning (5 elves, 5 capybaras, 3 each of
boar/deer/monkey/squirrel, plus food/rest variation and initial bread)
is duplicated in `main.gd` (`_ready()` and `_on_mp_game_started()`).
This moves entirely into the sim:

```rust
impl SimState {
    /// Spawn the initial set of creatures for a new game.
    /// Called by GameSession::process(StartGame).
    ///
    /// Uses data from config.initial_creatures list so it's
    /// deterministic and not hardcoded. All spawning uses the sim's
    /// PRNG for determinism.
    pub fn spawn_initial_creatures(&mut self) {
        for spec in &self.config.initial_creatures {
            for i in 0..spec.count {
                // 1. Spawn creature at default position
                //    (near world center, snapped to nav).
                let creature_id = self.spawn_creature_internal(
                    spec.species, spec.spawn_position,
                );
                // 2. Set food level from per-index override or default.
                let food_pct = spec.food_pcts
                    .get(i)
                    .copied()
                    .unwrap_or(100);
                if food_pct < 100 {
                    if let Some(c) = self.creatures.get_mut(&creature_id) {
                        let max = self.config.species[&spec.species].food_max;
                        c.food = max * food_pct as i64 / 100;
                    }
                }
                // 3. Set rest level from per-index override or default.
                let rest_pct = spec.rest_pcts
                    .get(i)
                    .copied()
                    .unwrap_or(100);
                if rest_pct < 100 {
                    if let Some(c) = self.creatures.get_mut(&creature_id) {
                        let max = self.config.species[&spec.species].rest_max;
                        c.rest = max * rest_pct as i64 / 100;
                    }
                }
                // 4. Add initial items from per-index override.
                let bread_count = spec.bread_counts
                    .get(i)
                    .copied()
                    .unwrap_or(0);
                if bread_count > 0 {
                    if let Some(c) = self.creatures.get_mut(&creature_id) {
                        c.inventory.add(ItemKind::Bread, bread_count);
                    }
                }
            }
        }
        // 5. Place initial ground piles from config.
        for pile_spec in &self.config.initial_ground_piles {
            // Find valid position (scan upward for air).
            let pos = self.find_surface_position(pile_spec.position);
            let pile = self.ground_piles
                .entry(pos)
                .or_insert_with(GroundPile::new);
            pile.inventory.add(pile_spec.item_kind, pile_spec.quantity);
        }
    }
}
```

This means `GameConfig` gains:

```rust
/// Specification for initial creature spawning at game start.
pub struct InitialCreatureSpec {
    pub species: Species,
    pub count: usize,
    pub spawn_position: VoxelCoord,
    /// Per-creature food percentages (index 0 = first creature).
    /// Missing indices default to 100%.
    pub food_pcts: Vec<u32>,
    /// Per-creature rest percentages. Missing indices default to 100%.
    pub rest_pcts: Vec<u32>,
    /// Per-creature bread counts. Missing indices default to 0.
    pub bread_counts: Vec<u32>,
}

/// Specification for an initial ground pile at game start.
pub struct InitialGroundPileSpec {
    pub position: VoxelCoord,
    pub item_kind: ItemKind,
    pub quantity: i32,
}

// Added to GameConfig:
pub initial_creatures: Vec<InitialCreatureSpec>,
pub initial_ground_piles: Vec<InitialGroundPileSpec>,
```

The defaults reproduce the current hardcoded behavior from `main.gd`:
5 elves with food_pcts `[100, 90, 70, 60, 48]`, rest_pcts
`[100, 95, 80, 60, 45]`, bread_counts `[0, 1, 2, 3, 4]`; 5 capybaras;
3 each of boar/deer/monkey/squirrel; one ground bread pile of 5.

---

## 7. Tick Pacing: Local Relay for Single-Player

Single-player and multiplayer use the same tick-pacing path: a
relay-like component that converts wall-clock time into `AdvanceTo`
messages.

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

**Multiplayer relay:** The relay is a pure message orderer. It
synchronizes `SessionMessage`s, not sim commands. It doesn't need to
know about pause/resume semantics -- those are `SessionMessage`s
delivered through the normal turn stream. The relay accepts messages
from clients, assigns them a total order, and broadcasts them. It has
no game-level knowledge. Pause and Resume are just messages that happen
to affect how clients use the `AdvanceTo` messages they receive.

This simplifies the relay design from `multiplayer_relay.md`: the relay
doesn't need separate pause/resume wire messages or speed-change logic.
It just orders and delivers `SessionMessage`s. The `session.paused`
field is the primary pause mechanism; the relay doesn't need to track
or enforce it.

(In practice, the relay may still manage turn pacing -- deciding *when*
to emit turns -- but this is a transport concern, not a game logic
concern. The session handles all game semantics.)

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
    pub fn new(seconds_per_tick: f64, max_ticks_per_frame: u64) -> Self {
        Self {
            accumulator: 0.0,
            seconds_per_tick,
            max_ticks_per_frame,
        }
    }

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

---

## 8. Networking Is External

`GameSession` is pure logic. It doesn't know about TCP, relays, or wire
protocols. The gdext bridge (`SimBridge`) sits between the network and
the session:

```
                    +------------------+
                    |    SimBridge      |
                    |    (gdext)        |
                    |                  |
  GDScript -------->  Translates      |
  (UI actions)     |  UI -> Session   |
                    |  Messages        |
                    |                  |
  NetClient ------->  Translates      |
  (wire msgs)      |  Wire -> Session |
                    |  Messages        |
                    |        |         |
                    |        v         |
                    |  +------------+  |
                    |  |GameSession |  |
                    |  |  (pure)    |  |
                    |  +------------+  |
                    |        |         |
                    |   SessionEvents  |
                    |        |         |
                    |        v         |
                    |  -> GDScript     |
                    |  -> NetClient    |
                    +------------------+
```

### Why external

- **Testability.** Tests create a `GameSession`, feed it messages, and
  inspect fields. No mock sockets needed.
- **Separation of concerns.** The session is the game logic. The network
  is I/O. Mixing them creates the kind of scattered state we're trying
  to eliminate.
- **Flexibility.** The same session processes messages regardless of
  source: local relay, remote relay, replay file, test harness.

### Translation layer (SimBridge)

`SimBridge` translates between the wire protocol
(`ClientMessage`/`ServerMessage`) and `SessionMessage`. The relay is a
pure message orderer, so the translation is straightforward:

```rust
// In SimBridge::poll_network():
for msg in self.net_client.poll() {
    match msg {
        ServerMessage::Turn { sim_tick_target, commands, .. } => {
            // Each command in the turn -> SessionMessage::SimCommand
            for tc in commands {
                if let Ok(action) = serde_json::from_slice(&tc.payload) {
                    // Map RelayPlayerId -> SessionPlayerId (same u32 value)
                    let session_pid = SessionPlayerId(tc.player_id.0);
                    session.process(SessionMessage::SimCommand {
                        from: session_pid,
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
            // StartGame carries seed and config as parameters.
            let config = parse_config(&config_json);
            session.process(SessionMessage::StartGame {
                seed: seed as u64,
                config,
            });
        }
        ServerMessage::Paused { by } => {
            session.process(SessionMessage::Pause {
                by: SessionPlayerId(by.0),
            });
        }
        ServerMessage::Resumed { by } => {
            session.process(SessionMessage::Resume {
                by: SessionPlayerId(by.0),
            });
        }
        ServerMessage::SnapshotLoad { data, .. } => {
            if let Ok(json) = String::from_utf8(data) {
                session.process(SessionMessage::LoadSim { json });
            }
        }
        ServerMessage::PlayerJoined { player } => {
            session.process(SessionMessage::PlayerJoined {
                id: SessionPlayerId(player.id.0),
                name: player.name,
            });
        }
        ServerMessage::PlayerLeft { player_id, name } => {
            session.process(SessionMessage::PlayerLeft {
                id: SessionPlayerId(player_id.0),
            });
        }
        _ => { /* Welcome, Rejected, SpeedChanged, etc. handled by SimBridge */ }
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
            from: SessionPlayerId::LOCAL,
            action,
        });
    }
}
```

---

## 9. SimBridge Refactoring

### What moves into GameSession

| Current location           | New location                          |
|---------------------------|--------------------------------------|
| `SimBridge.sim`           | `GameSession.sim`                    |
| `SimBridge.is_multiplayer_mode` | Implicit: has `net_client`       |
| `SimBridge.is_host`       | `GameSession.host_id == local_id`    |
| `SimBridge.game_started`  | `GameSession.has_sim()`              |
| `SimBridge.mp_ticks_per_turn` | Relay config (external)           |
| `SimState.speed`          | `GameSession.speed`                  |

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
    /// Chunk mesh cache -- rendering concern, not session state.
    mesh_cache: Option<MeshCache>,
    /// Buffered events for GDScript (from SessionEvents).
    pending_events: Vec<String>,
    /// Local player's session ID.
    local_player_id: SessionPlayerId,
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
            // Translate wire messages -> SessionMessages, process them.
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

## 10. Delayed Command Application: Audit Results

Moving from immediate command application (`apply_or_send()` with its
1-tick step side effect) to deferred application (buffer until
`AdvanceTo`) could break code that queries sim state immediately after
sending a command. An audit of the codebase found the following:

### A1/A5 (HIGH): Spawn then immediately set food/rest/items

**Location:** `main.gd` `_ready()` and `_on_mp_game_started()`

**Pattern:** Spawn 5 elves, then immediately call
`bridge.set_creature_food("Elf", i, ...)` which references elves by
index. This only works because `spawn_elf()` has the 1-tick step side
effect.

**Resolution:** Eliminated entirely by moving to
`spawn_initial_creatures()` (section 6). The sim handles all initial
setup atomically -- no cross-boundary spawn-then-query.

### A2-A4 (LOW): Count queries after spawns

**Location:** `main.gd` `_ready()` and `_on_mp_game_started()`

**Pattern:** `print("spawned %d elves" % bridge.elf_count())` after
spawning. With deferred application, the count would still be 0 at
print time.

**Resolution:** Cosmetic print statements only. These move into
`spawn_initial_creatures()` (or are removed) as part of the migration.
No gameplay impact.

### A6 (MEDIUM): `designate_build_rect()` reads `last_build_message`

**Location:** `sim_bridge.rs` `designate_build_rect()` and similar

**Pattern:** After calling `apply_or_send(DesignateBuild { ... })`, the
method immediately reads `sim.last_build_message` to return a validation
message to GDScript. With deferred application, `last_build_message`
would still have the old value.

**Resolution:** Rely on `validate_platform_preview()` (which already
exists) for pre-command validation rather than post-command message
reading. The construction controller already calls
`validate_platform_preview()` before showing the ghost preview. The
`designate_build_rect()` return value can be changed to a simple success
indicator, with validation happening beforehand. This is a cleaner
pattern -- validate first, then command -- rather than command-then-read.

### A7 (LOW): Preview cache invalidation

**Location:** `construction_controller.gd`

**Pattern:** After a build designation, the construction controller
invalidates its preview cache. With deferred application, the world
hasn't changed yet.

**Resolution:** Self-corrects next frame. The preview cache is rebuilt
every frame when the construction panel is active. A one-frame delay in
cache invalidation is invisible to the player.

### Summary

The audit confirms that the codebase is clean with respect to delayed
command application. The only HIGH issues (A1/A5) are eliminated by
`spawn_initial_creatures()`. The MEDIUM issue (A6) has a clean
resolution via existing validation infrastructure. The LOW issues are
cosmetic or self-correcting.

---

## 11. GDScript Changes

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
- `_sim_accumulator` -- now in `LocalRelay`.
- `_mp_time_since_turn` -- now in multiplayer render tick logic (Rust).
- `_seconds_per_tick` -- now in `LocalRelay`.
- The single-player vs. multiplayer branching in `_process()`.
- Initial creature spawning from `_ready()` and `_on_mp_game_started()`
  -- now in `SimState::spawn_initial_creatures()`.
- `set_creature_food()`, `set_creature_rest()`, `add_creature_item()`,
  `add_ground_pile_item()` calls -- now in `spawn_initial_creatures()`.

### game_session.gd

Unchanged in structure. Still carries menu -> game transition data (seed,
tree profile, load path, multiplayer config). These are read once by
`main.gd`'s `_ready()` and used to construct `SessionMessage::StartGame`.

---

## 12. Save Files

Save files remain sim-only (`SimState` serialized to JSON), as they are
today. The session is a transient context (a group of friends playing
together) -- it's not persisted to disk.

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

Removing `SimSpeed` from `SimState` will change the serialization
format. At this stage of development, save compatibility is not a
concern -- we can bump the format freely.

For multiplayer synchronization (mid-game join), the session serializes
the sim via `sim.to_json()` and sends it through the relay. The
receiving client processes `SessionMessage::LoadSim { json }`. This
is the same operation as loading a save file -- the session doesn't
distinguish between "loaded from disk" and "loaded from network."

---

## 13. Determinism

### What's synchronized in multiplayer

The relay orders and broadcasts messages that clients translate into
`SessionMessage`s. All clients process the same messages in the same
order, producing identical `GameSession` field values.

Since `GameSession` is deterministic and all fields are derived from the
message stream, session-level fields (speed, paused, players) are
automatically synchronized. No extra bookkeeping.

### Desync detection

Keep the current approach: sim-only checksums via `state_checksum()`
(FNV-1a of JSON serialization), compared by the relay every 1000 ticks.
Session-level fields are small and directly controlled by relay messages
-- if the relay delivers messages correctly, session fields can't
diverge independently.

If we later discover session-level desync bugs, we can extend the
checksum to include session fields. But for now, the sim is the only
complex piece that can diverge.

### spawn_initial_creatures() determinism

`spawn_initial_creatures()` is part of the sim's deterministic contract.
Given the same seed and config, it must produce identical results across
all clients. This is guaranteed by:
- Using the sim's PRNG for all random decisions (creature IDs,
  spawn position snapping).
- Reading initial creature specs from `GameConfig` (which is part of
  `StartGame` and thus synchronized).
- Performing all mutations internally (no external calls).

---

## 14. Test Plan

All tests in `elven_canopy_sim` -- no Godot dependency needed. Use the
existing `test_config()` pattern (small 64x64x64 world).

### 14.1 Message processing basics

**PlayerJoined / PlayerLeft:**
- Create a session. Send `PlayerJoined`. Assert player is in
  `session.players`.
- Send `PlayerLeft` for that player. Assert they're removed.
- Send `PlayerLeft` for a nonexistent player. Assert no crash, no
  change.

**StartGame:**
- Create a session with a known seed and config. Send
  `StartGame { seed: 42, config }`.
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

### 14.2 Command buffering and tick advancement

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
  event timestamps).

**AdvanceTo with no sim is a no-op:**
- Create a session with no sim. Send `AdvanceTo`.
- Assert no crash, no events.

**Concurrent commands from different session players:**
- Create a multiplayer session with two players (IDs 1 and 2).
- Buffer `SimCommand { from: SessionPlayerId(1), action: SpawnCreature }`
  and `SimCommand { from: SessionPlayerId(2), action: SpawnCreature }`.
- Send `AdvanceTo`. Assert both creatures exist.
- Assert deterministic ordering (both get the same sim player_id since
  they share a tree; ordering is by insertion order in pending_commands).

### 14.3 Pause / resume

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

### 14.4 AdvanceTo backward tick guard

**AdvanceTo with tick == current_tick:**
- Start a game at tick 0. Send `AdvanceTo { tick: 0 }`.
- Assert tick doesn't change. Assert pending_commands are NOT cleared.

**AdvanceTo with tick < current_tick:**
- Advance to tick 100. Send `AdvanceTo { tick: 50 }`.
- Assert tick stays at 100. Assert pending_commands preserved.

**AdvanceTo guard preserves buffered commands:**
- Buffer a SpawnCreature command. Send `AdvanceTo { tick: 0 }` (rejected).
- Assert command is still in pending_commands.
- Send `AdvanceTo { tick: 1 }` (accepted).
- Assert creature exists. Assert pending_commands is empty.

### 14.5 Speed

**SetSpeed changes multiplier:**
- `SetSpeed { Normal }` -> `speed_multiplier() == 1.0`.
- `SetSpeed { Fast }` -> `speed_multiplier() == 2.0`.
- `SetSpeed { VeryFast }` -> `speed_multiplier() == 5.0`.
- While paused: `speed_multiplier() == 0.0` regardless of speed.

**Speed doesn't affect sim logic:**
- Run two sessions with identical messages but different speed changes
  interspersed. Assert checksums are identical after the same number
  of AdvanceTo ticks. (Speed only affects wall-clock pacing, not sim
  results.)

### 14.6 Determinism

**Two sessions, same messages:**
- Create two sessions.
- Feed identical message streams: StartGame { seed: 42, config },
  various SimCommands, multiple AdvanceTo messages.
- Assert `sim.state_checksum()` matches after every AdvanceTo.

**Determinism across pause/resume/speed changes:**
- Session A: StartGame, advance to tick 1000 straight.
- Session B: StartGame, advance to 500, pause, resume, change speed,
  advance to 1000.
- Assert identical checksums (pause/resume/speed don't affect sim).

**Determinism of spawn_initial_creatures():**
- Create two sessions with the same seed and config.
- Send `StartGame` to both with identical seed and config.
- Assert creature counts match.
- Assert creature IDs match (same PRNG sequence).
- Assert food/rest/inventory values match.
- Assert `state_checksum()` matches before any AdvanceTo.

### 14.7 Save/load round-trip

- Create session, start game, advance to tick 5000 with some commands.
- Serialize sim: `json = sim.to_json()`.
- Create new session, `LoadSim { json }`.
- Advance both by 1000 more ticks (same commands).
- Assert identical checksums.

### 14.8 Multiplayer translation layer

**ServerMessage::Turn -> SessionMessages:**
- Construct a `ServerMessage::Turn` with two `TurnCommand`s and a
  `sim_tick_target`.
- Translate to `SessionMessage`s using the same logic as `poll_network()`.
- Assert: produces two `SimCommand` messages followed by one `AdvanceTo`.
- Assert: `RelayPlayerId` values map correctly to `SessionPlayerId`.

**ServerMessage::GameStart -> StartGame:**
- Construct `ServerMessage::GameStart { seed, config_json }`.
- Translate to `SessionMessage::StartGame { seed, config }`.
- Assert seed and config match.

**ServerMessage::Paused/Resumed -> Pause/Resume:**
- Construct `ServerMessage::Paused { by: RelayPlayerId(3) }`.
- Translate to `SessionMessage::Pause { by: SessionPlayerId(3) }`.
- Assert ID mapping is correct.

### 14.9 LocalRelay accumulator math

**Basic tick advancement:**
- Create a `LocalRelay` with `seconds_per_tick = 0.001`.
- Call `update(delta: 0.016, speed_mult: 1.0, current_tick: 0)`.
- Assert returns `AdvanceTo { tick: 16 }` (16ms at 1000 ticks/sec).

**Speed multiplier:**
- Call `update(delta: 0.016, speed_mult: 2.0, current_tick: 0)`.
- Assert returns `AdvanceTo { tick: 32 }` (2x speed).

**Accumulator carryover:**
- Call `update(delta: 0.0005, speed_mult: 1.0, current_tick: 0)`.
- Assert returns `None` (not enough for a full tick... wait, 0.5ms =
  0.5 ticks, rounds to 0).
- Call again with same delta. Assert returns `AdvanceTo { tick: 1 }`.

**Spiral-of-death cap:**
- Call `update(delta: 1.0, speed_mult: 1.0, current_tick: 0)`.
- Assert tick advancement is capped (accumulator capped at 0.1s = 100
  ticks max, then further capped by max_ticks_per_frame).

**Render tick fraction:**
- After `update()`, call `render_tick(current_tick)`.
- Assert it equals `current_tick + accumulator_remainder`.

**Paused (speed_mult = 0.0):**
- Call `update(delta: 0.016, speed_mult: 0.0, current_tick: 100)`.
- Assert returns `None`. Assert `render_tick(100)` == 100.0.

### 14.10 PartialEq derivations

Verify that `SessionEvent` and `SessionMessage` derive `PartialEq` by
using `==` in test assertions:

```rust
assert_eq!(
    SessionEvent::GameStarted,
    SessionEvent::GameStarted,
);
assert_ne!(
    SessionEvent::SpeedChanged { speed: SessionSpeed::Normal },
    SessionEvent::SpeedChanged { speed: SessionSpeed::Fast },
);
assert_eq!(
    SessionMessage::StartGame { seed: 42, config: test_config() },
    SessionMessage::StartGame { seed: 42, config: test_config() },
);
```

(This requires `GameConfig` to derive `PartialEq`, or use a helper.)

### 14.11 Edge cases

- **SimCommand with no sim loaded:** Silently dropped.
- **StartGame when sim already loaded:** Replaces the sim (acts like
  UnloadSim + StartGame).
- **LoadSim with invalid JSON:** Error event, sim unchanged.
- **Rapid pause/resume (10x):** Assert consistent fields.
- **10,000 ticks with no commands:** Sim processes heartbeats, tree
  growth, etc. normally.

---

## 15. File-by-File Change Plan

### New files

| File | Purpose |
|------|---------|
| `elven_canopy_sim/src/session.rs` | `GameSession`, `SessionMessage`, `SessionEvent`, `SessionSpeed`, `SessionPlayerId`, `PlayerSlot`, `PendingAction`. Core struct + `process()` + tests. |

### Modified: Rust

**`elven_canopy_sim/src/lib.rs`**
- Add `pub mod session;`
- Re-export `GameSession`, `SessionMessage`, `SessionSpeed`,
  `SessionEvent`, `SessionPlayerId`.

**`elven_canopy_sim/src/sim.rs`**
- Remove `speed: SimSpeed` field from `SimState`.
- Remove the `SetSimSpeed` match arm from `apply_command()`.
- Remove `SpeedChanged` event emission from `apply_command()`.
- Add `spawn_initial_creatures()` method (data-driven from config).
- Keep `step()` unchanged -- it's the right primitive for the session
  to call.

**`elven_canopy_sim/src/command.rs`**
- Remove `SimAction::SetSimSpeed` variant.
- Add `SimAction::SetCreatureFood`, `SetCreatureRest`,
  `AddCreatureItem`, `AddGroundPileItem` variants.
- Update docstring.

**`elven_canopy_sim/src/event.rs`**
- Remove `SimEventKind::SpeedChanged` variant.
- Update docstring.

**`elven_canopy_sim/src/types.rs`**
- Remove `SimSpeed` enum. `SessionSpeed` replaces it.

**`elven_canopy_sim/src/config.rs`**
- Add `initial_creatures: Vec<InitialCreatureSpec>` to `GameConfig`.
- Add `initial_ground_piles: Vec<InitialGroundPileSpec>` to `GameConfig`.
- Add `InitialCreatureSpec` struct.
- Add `InitialGroundPileSpec` struct.
- Defaults reproduce current hardcoded values from `main.gd`.

**`elven_canopy_gdext/src/sim_bridge.rs`**
- Replace `sim: Option<SimState>` with `session: GameSession`.
- Add `local_relay: Option<LocalRelay>`.
- Add `local_player_id: SessionPlayerId`.
- Remove `is_multiplayer_mode`, `is_host`, `game_started`,
  `mp_ticks_per_turn` fields.
- Rewrite `apply_or_send()` -> `session.process(SimCommand { ... })`
  (single-player) or `net_client.send_command()` (multiplayer).
- Rewrite `step_to_tick()` -> `session.process(AdvanceTo { tick })`.
- Rewrite `set_sim_speed()` -> `session.process(SetSpeed { ... })`.
- Remove `set_creature_food()`, `set_creature_rest()`,
  `add_creature_item()`, `add_ground_pile_item()` (or convert to
  session message wrappers if needed for debug/testing).
- Add `frame_update(delta) -> f64` method that handles tick pacing
  and returns render_tick.
- Rewrite `poll_network()` to translate wire messages ->
  SessionMessages.
- All sim query methods delegate through `session.sim`.
- Change `designate_build_rect()` to return a simple success indicator
  instead of reading `last_build_message` synchronously.
- Update module docstring.

**`elven_canopy_gdext/src/sim_bridge.rs` (new struct)**
- Add `LocalRelay` struct (accumulator-based tick pacer).

### Modified: GDScript

**`godot/scripts/main.gd`**
- Unify `_process()`: call `bridge.frame_update(delta)` instead of
  separate SP/MP branches.
- Remove `_sim_accumulator`, `_mp_time_since_turn`, `_seconds_per_tick`.
- Remove initial creature spawning from `_ready()` and
  `_on_mp_game_started()` (now in `spawn_initial_creatures()`).
- Remove `set_creature_food()`, `set_creature_rest()`,
  `add_creature_item()`, `add_ground_pile_item()` calls.
- `_ready()` simplified: configure session, send StartGame or LoadSim,
  then `_setup_common()`.
- Update `designate_build_rect()` callers to not use the return value
  for validation (use `validate_platform_preview()` instead).

**`godot/scripts/action_toolbar.gd`**
- No changes. Still emits `speed_changed` signal. Handler in main.gd
  calls `bridge.set_sim_speed()`, which calls
  `session.process(SetSpeed)`.

**`godot/scripts/game_session.gd`**
- No structural changes. Still carries menu -> game transition data.

### Modified: Documentation

**`CLAUDE.md`**
- Update project structure to mention `session.rs`.
- Update "Implementation Status" to reflect session architecture.
- Update "Codebase Patterns and Gotchas" to replace `apply_or_send`
  side-effect note with session message model.
- Remove `SimSpeed::Paused` references.

**`docs/design_doc.md`**
- Update section 4 to describe the session message model.
- Note that speed/pause are session concerns.

**`docs/tracker.md`**
- Update F-session-sm status as work progresses.

---

## 16. Migration Phases

### Phase 1: GameSession struct + tests

Create `session.rs` with `GameSession`, `SessionMessage`,
`SessionEvent`, `SessionSpeed`, `SessionPlayerId`, `PendingAction`,
`PlayerSlot`. Implement `process()` with the backward tick guard.
Write all tests from section 14.1-14.6, 14.10-14.11. Add `PartialEq`
derivations on `SessionEvent` and `SessionMessage`.

`SimState.speed` and `SimAction::SetSimSpeed` still exist but are
unused by the session. The session manages speed independently.

This is a pure addition -- no existing code changes. Everything compiles
and tests pass alongside the old code.

### Phase 2: Initial creature spawning + direct mutation commands

Add `initial_creatures` and `initial_ground_piles` to `GameConfig`.
Add `InitialCreatureSpec` and `InitialGroundPileSpec`.
Implement `spawn_initial_creatures()` on `SimState`.
Add `SimAction::SetCreatureFood`, `SetCreatureRest`, `AddCreatureItem`,
`AddGroundPileItem` variants (even though `spawn_initial_creatures()`
handles setup internally, having them as proper commands is correct for
the determinism contract).
Update session tests to verify initial creatures (section 14.6 spawn
determinism tests).
Still no integration with SimBridge -- old code paths work.

### Phase 3: Wire GameSession into SimBridge + LocalRelay

Replace `SimBridge.sim` with `SimBridge.session`. Add `LocalRelay`.
Rewrite the key methods (`apply_or_send`, `step_to_tick`,
`set_sim_speed`, `poll_network`). All sim queries go through
`session.sim`. Add `frame_update()`.
Remove `set_creature_food()`, `set_creature_rest()`,
`add_creature_item()`, `add_ground_pile_item()` from SimBridge.
Change `designate_build_rect()` to not read `last_build_message`.
Add LocalRelay tests (section 14.9).
Add multiplayer translation tests (section 14.8).

This is the big integration step. Both SP and MP paths now go through
the session.

### Phase 4: Simplify GDScript

Unify `_process()` in main.gd. Remove accumulator fields. Remove
initial creature spawning from GDScript. Remove direct-mutation calls.

### Phase 5: Remove SimSpeed from sim

Remove `speed: SimSpeed` from `SimState`. Remove
`SimAction::SetSimSpeed`. Remove `SimEventKind::SpeedChanged`. Remove
the `SimSpeed` type. Update all references. Clean up dead code in
SimBridge.

### Phase 6: Documentation

Update CLAUDE.md, design_doc.md, tracker.md.

---

## 17. Relationship to Existing Multiplayer Design

This draft **refines and simplifies** the relay design in
`docs/drafts/multiplayer_relay.md`.

The key simplification: the relay becomes a pure message orderer with
no game-level semantics. It doesn't need to understand pause/resume,
speed changes, or game start -- it just orders and delivers messages.
The session handles all game semantics through its `process()` method.

Key relationships:
- The relay's `ServerMessage::Turn` maps to a batch of
  `SessionMessage::SimCommand` + `SessionMessage::AdvanceTo`.
- The relay's `Paused`/`Resumed` messages map to
  `SessionMessage::Pause`/`Resume` (these are delivered through the
  normal message stream, not special relay-level mechanisms).
- The relay's `GameStart` maps to `SessionMessage::StartGame` (with
  seed and config as parameters).
- Mid-game join (`SnapshotLoad`) maps to `SessionMessage::LoadSim`.
- Desync detection (`DesyncDetected`) becomes a `SessionEvent`.

The wire protocol (`ClientMessage`/`ServerMessage`) may evolve to
more closely match `SessionMessage` as the relay simplifies, but the
translation layer in `SimBridge` handles any mismatch.
