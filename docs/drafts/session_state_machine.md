# Session and Simulator State Machines

Draft design for formalizing the game session and simulator into explicit
state machines where all state transitions happen in response to typed
messages. This replaces the current pattern of scattered mutable flags
across `game_session.gd`, `SimBridge`, and `SimState` with a single,
Rust-side `GameSession` that owns all session-level state and the
simulator.

**Key principle:** Single-player and multiplayer use the same
`GameSession` state machine. They differ only in *where* messages come
from — locally queued vs. relayed from a coordinator. The state machine
itself is identical.

---

## 1. Motivation

### Current state of affairs

Session-level state is spread across three layers:

| State                         | Current owner          | Problem                                |
|-------------------------------|------------------------|----------------------------------------|
| Seed, tree profile            | `game_session.gd`      | Bag of mutable fields, no lifecycle    |
| `is_multiplayer_mode`         | `SimBridge`            | Boolean flag, no transitions           |
| `is_host`, `game_started`    | `SimBridge`            | Implicit state encoded in booleans     |
| `mp_ticks_per_turn`          | `SimBridge`            | Session config mixed with bridge       |
| `net_client`, `relay_handle` | `SimBridge`            | Networking mixed with sim queries      |
| `speed: SimSpeed`            | `SimState`             | Session concern stored in sim          |
| Sim tick advancement         | `main.gd` accumulator  | Driven by GDScript frame loop          |

This creates several problems:

1. **No single source of truth.** "Is the game paused?" requires checking
   `SimState.speed`, the relay's paused flag, and GDScript's accumulator.
2. **Invalid states are possible.** Nothing prevents calling
   `step_to_tick()` while in the lobby, or spawning creatures before the
   sim is initialized. These are prevented by runtime `if` checks, not
   structural guarantees.
3. **`apply_or_send()` has side effects.** In single-player, it advances
   the sim by 1 tick as a side effect of applying a command. This couples
   "enqueue a command" with "advance time."
4. **Single-player and multiplayer are separate code paths.** `main.gd`
   has two branches in `_process()` — one for single-player (accumulator
   + `step_to_tick()`) and one for multiplayer (`poll_network()`). Any
   session-level feature (pause, speed, save) needs two implementations.
5. **Sim speed is a sim concern but should be a session concern.** Pausing
   means "stop advancing time," which is a decision the session makes.
   The sim shouldn't need to know it's paused — it just doesn't get ticked.

### What formalization gives us

- **Structural correctness.** The Rust type system prevents invalid
  transitions. You can't apply a command in the `Lobby` state because
  the `Lobby` variant doesn't contain a `SimState`.
- **Unified single/multiplayer.** Both modes feed `SessionMessage`s into
  the same state machine. Testing, save/load, replay, and spectator mode
  become session-level features that work identically in both modes.
- **Cleaner command/time separation.** Commands are buffered. Time
  advancement is a separate operation driven by the session's state.
- **Easier reasoning about multiplayer correctness.** The relay
  synchronizes `SessionMessage`s. Desync detection can hash the entire
  session state, not just the sim.

---

## 2. GameSession State Machine

### States

```
                        ┌──────────────────┐
                        │     Uninit       │
                        └────────┬─────────┘
                     CreateSession│
                                 ▼
                        ┌────────────────┐
                        │     Lobby      │
                        │                │
                        │ • players[]    │
                        │ • seed         │
                        │ • config       │
                        │ • host_id      │
                        └────────┬───────┘
                       StartGame │
                                 ▼
              ┌──────────────────────────────────┐
              │            Playing               │
              │                                  │
              │ • sim: SimState                  │
              │ • speed: SessionSpeed            │
              │ • players[]                      │
              │ • host_id                        │
              │ • pending_commands: Vec<Command> │
              └───────┬──────────────────┬───────┘
                      │                  │
              Pause   │                  │ EndGame
                      ▼                  │
              ┌───────────────┐          │
              │    Paused     │          │
              │               │          │
              │ • sim (frozen)│          │
              │ • speed (prev)│          │
              │ • paused_by   │          │
              └───────┬───────┘          │
              Resume  │                  │
                      ▼                  │
                  (→ Playing)            │
                                         ▼
                                ┌─────────────┐
                                │   GameOver   │
                                └─────────────┘
```

### Rust sketch

```rust
/// The top-level session state machine. Owns the sim (when one exists)
/// and all session metadata. This is the single source of truth.
///
/// Lives in `elven_canopy_sim` (no Godot dependency). The gdext bridge
/// wraps it and translates GDScript calls into SessionMessages.
pub enum SessionState {
    /// Pre-game: waiting for players, configuring the game.
    Lobby {
        seed: u64,
        config: GameConfig,
        players: Vec<PlayerSlot>,
        host_id: PlayerId,
    },

    /// Game in progress: sim is running (or paused).
    Playing {
        sim: SimState,
        speed: SessionSpeed,
        players: Vec<PlayerSlot>,
        host_id: PlayerId,
        /// Commands received but not yet applied. Flushed on each tick
        /// advancement.
        pending_commands: Vec<SimCommand>,
    },

    /// Game is paused: sim state is frozen, no time advancement.
    /// Modeled as a distinct state rather than a speed variant so that
    /// the type system prevents tick advancement while paused.
    Paused {
        sim: SimState,
        previous_speed: SessionSpeed,
        paused_by: PlayerId,
        players: Vec<PlayerSlot>,
        host_id: PlayerId,
    },

    /// Game has ended (victory, defeat, or session closed).
    GameOver {
        final_tick: u64,
    },
}

/// Session speed — how fast the sim advances relative to wall clock.
/// Unlike SimSpeed, this does NOT include a Paused variant; pausing is
/// a state transition, not a speed setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionSpeed {
    Normal,   // 1x
    Fast,     // 2x
    VeryFast, // 5x
}

/// A player in the session (local or remote).
pub struct PlayerSlot {
    pub id: PlayerId,
    pub name: String,
    pub is_local: bool,
}
```

### Why Paused is a state, not a speed

In the current code, `SimSpeed::Paused` is a variant of the speed enum
inside `SimState`. This means the sim "knows" it's paused and carries
that state. But pausing is really a session-level decision: "stop
advancing time." The sim shouldn't care — it just doesn't receive ticks.

Making `Paused` a separate `SessionState` variant has structural benefits:

- You can't accidentally advance the sim while paused — the `Playing`
  state (which has the `advance_to()` method) isn't accessible.
- The previous speed is preserved and restored on resume without extra
  bookkeeping.
- In multiplayer, pause/resume are session messages that all clients
  process identically — no ambiguity about whether `SetSimSpeed(Paused)`
  is a sim command or a session command.

The tradeoff is that transitions between `Playing` and `Paused` involve
moving `SimState` ownership between enum variants. In Rust this is a
zero-cost move (no copying), so this is purely a code ergonomics
question, and `std::mem::take()`-style patterns handle it cleanly.

---

## 3. SessionMessage — The Input Alphabet

All session state transitions are driven by `SessionMessage`. In
single-player, these are produced locally. In multiplayer, the relay
orders and broadcasts them.

```rust
/// A message that drives the session state machine.
/// This is what the relay synchronizes in multiplayer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionMessage {
    // --- Lobby phase ---
    /// A player has joined the session.
    PlayerJoined { player: PlayerSlot },
    /// A player has left the session.
    PlayerLeft { player_id: PlayerId },
    /// Start the game (lobby → playing transition). Host only.
    StartGame,

    // --- Playing phase ---
    /// A simulation command from a player.
    SimCommand {
        player_id: PlayerId,
        action: SimAction,
    },
    /// Advance the simulation to the given tick.
    /// In single-player, generated by the local clock.
    /// In multiplayer, generated by the relay's turn cadence.
    AdvanceTo { tick: u64 },
    /// Change the simulation speed.
    SetSpeed { speed: SessionSpeed },
    /// Pause the game.
    Pause { by: PlayerId },
    /// Resume the game.
    Resume { by: PlayerId },

    // --- Any phase ---
    /// Save the game state.
    SaveRequest { path: String },
    /// Load a game state, replacing the current sim.
    LoadState { json: String },
    /// End the session.
    EndSession,
}
```

### Message processing

```rust
impl GameSession {
    /// Process a message and return any output events.
    /// This is the core state machine transition function.
    pub fn process(&mut self, msg: SessionMessage) -> Vec<SessionEvent> {
        match (&mut self.state, msg) {
            // Lobby transitions
            (SessionState::Lobby { .. }, SessionMessage::StartGame) => {
                // Construct SimState from seed + config, transition to Playing.
            }

            // Playing transitions
            (SessionState::Playing { sim, pending_commands, .. },
             SessionMessage::SimCommand { player_id, action }) => {
                // Buffer the command for the next AdvanceTo.
                pending_commands.push(SimCommand { player_id, tick: sim.tick + 1, action });
            }
            (SessionState::Playing { sim, pending_commands, .. },
             SessionMessage::AdvanceTo { tick }) => {
                // Drain pending_commands, call sim.step(), return events.
            }
            (SessionState::Playing { .. }, SessionMessage::Pause { by }) => {
                // Move sim from Playing → Paused.
            }

            // Paused transitions
            (SessionState::Paused { .. }, SessionMessage::Resume { by }) => {
                // Move sim from Paused → Playing, restore previous speed.
            }

            // Invalid transitions are no-ops (or return an error event).
            _ => {}
        }
    }
}
```

---

## 4. Sim Changes: Separating Commands from Time

Currently, `SimState::step()` interleaves command application and time
advancement in a single function. The `apply_or_send()` method in
`SimBridge` makes this worse by stepping the sim by 1 tick as a side
effect of applying a command.

### Proposed sim API

```rust
impl SimState {
    /// Buffer a command for application at the next tick advancement.
    /// Pure — does not mutate sim state or advance time.
    pub fn enqueue_command(&mut self, cmd: SimCommand) {
        self.pending_commands.push(cmd);
    }

    /// Advance the simulation to `target_tick`, applying any buffered
    /// commands at their designated ticks and processing all scheduled
    /// events.
    ///
    /// This is the only method that advances `self.tick`.
    pub fn advance_to(&mut self, target_tick: u64) -> StepResult {
        // Sort pending commands by tick.
        // Interleave command application and event processing (same
        // algorithm as current step(), but commands come from the
        // internal buffer instead of a parameter).
    }
}
```

The existing `step(&[SimCommand], target_tick)` can remain as a
convenience method that calls `enqueue_command` + `advance_to`, so
existing tests and the multiplayer turn application path don't break.

### What moves out of SimState

- **`speed: SimSpeed`** — moves to `SessionState`. The sim doesn't need
  to know its speed; the session decides how many ticks to advance per
  frame.
- **`SetSimSpeed` command variant** — removed from `SimAction`. Speed
  changes become `SessionMessage::SetSpeed` instead.
- **`SimSpeed` type** — replaced by `SessionSpeed` (no `Paused` variant)
  at the session level.
- **`SpeedChanged` event** — moves from `SimEventKind` to
  `SessionEvent`. Not a sim event, since the sim doesn't know about
  speed.

### What stays in SimState

Everything else. The sim remains the pure `(state, commands) → (new_state, events)` function. It just no longer manages its own speed or
pause state.

---

## 5. Single-Player Flow (Unified)

In single-player, the game creates a `GameSession` with a single local
player. There is no relay — messages are processed directly.

### Startup

```
1. main.gd _ready():
     session = GameSession::new(seed, config, local_player)
       → state = Lobby { ... }
     session.process(SessionMessage::StartGame)
       → state = Playing { sim: SimState::new(seed, config), ... }
```

### Per-frame tick advancement

```
2. main.gd _process(delta):
     // Session computes ticks from delta and current speed.
     let ticks = session.ticks_for_delta(delta)
     if ticks > 0:
         session.process(SessionMessage::AdvanceTo { tick: current + ticks })
           → sim.advance_to(target_tick) internally
```

### Player actions

```
3. Player clicks "spawn elf":
     session.process(SessionMessage::SimCommand {
         player_id: local,
         action: SimAction::SpawnCreature { ... },
     })
       → sim.enqueue_command(cmd) — no side effects
       // Command is applied on the next AdvanceTo
```

### Pause

```
4. Player clicks "Pause":
     session.process(SessionMessage::Pause { by: local })
       → state transitions Playing → Paused
     // main.gd sees state is Paused, stops calling AdvanceTo
```

### Benefits over current approach

- No more `apply_or_send()` with its 1-tick side effect.
- No more separate "is this single-player or multiplayer?" branching in
  `_process()`.
- Speed is a session concern — the sim doesn't store or check it.
- All state queries go through the session, which knows whether a sim
  exists and what state it's in.

---

## 6. Multiplayer Flow (Unified)

In multiplayer, the relay orders `SessionMessage`s and broadcasts them
to all clients. Each client processes the same messages in the same
order, producing identical state transitions.

### Mapping relay messages to SessionMessages

The relay protocol (`ClientMessage`/`ServerMessage` in
`elven_canopy_protocol`) is a wire format. The gdext bridge translates
between the wire protocol and `SessionMessage`s:

| Wire message (ServerMessage)       | SessionMessage equivalent              |
|------------------------------------|----------------------------------------|
| `GameStart { seed, config_json }`  | `StartGame`                            |
| `Turn { tick, commands }`          | `SimCommand` (×N) + `AdvanceTo { tick }` |
| `Paused { by }`                    | `Pause { by }`                         |
| `Resumed { by }`                   | `Resume { by }`                        |
| `SpeedChanged { ticks_per_turn }`  | `SetSpeed { speed }` (after mapping)   |
| `PlayerJoined { player }`          | `PlayerJoined { player }`              |
| `PlayerLeft { player_id, .. }`     | `PlayerLeft { player_id }`             |
| `SnapshotLoad { data }`           | `LoadState { json }`                   |

A `Turn` message maps to N `SimCommand` messages (one per command in the
turn) followed by one `AdvanceTo` message. The session processes them in
order: commands are buffered, then `AdvanceTo` flushes them all.

### Per-frame loop (multiplayer)

```
1. main.gd _process(delta):
     // Poll the network — translate ServerMessages to SessionMessages.
     let messages = bridge.poll_network()
     for msg in messages:
         session.process(msg)
     // Compute render_tick for interpolation (same as single-player).
     render_tick = session.render_tick(delta)
```

The `_process()` loop is now *identical* in structure for single-player
and multiplayer. The only difference is the source of messages: local
clock + local input vs. relay.

---

## 7. SimBridge Refactoring

The `SimBridge` gdext node becomes a thin wrapper around `GameSession`
rather than owning `SimState` directly. Most of its fields move into
`GameSession`:

### What moves into GameSession (Rust, pure)

- `sim: Option<SimState>` → owned by `SessionState::Playing`
- `is_multiplayer_mode`, `is_host`, `game_started` → implicit in
  `SessionState` variant + presence of relay
- `mp_ticks_per_turn` → session config inside `SessionState`
- `speed` (currently in `SimState`) → `SessionState::Playing.speed`

### What stays in SimBridge (gdext, Godot-dependent)

- `base: Base<Node>` — Godot node plumbing
- `session: GameSession` — the state machine (pure Rust)
- `mesh_cache: Option<MeshCache>` — rendering concern
- `net_client: Option<NetClient>` — network I/O
- `relay_handle: Option<RelayHandle>` — embedded relay lifecycle
- `mp_events: Vec<String>` — buffered events for GDScript

### Method changes

| Current method             | New approach                                 |
|----------------------------|----------------------------------------------|
| `init_sim(seed)`           | `session.process(StartGame)` after lobby     |
| `step_to_tick(tick)`       | `session.process(AdvanceTo { tick })`        |
| `set_sim_speed(name)`      | `session.process(SetSpeed { speed })`        |
| `sim_speed_multiplier()`   | `session.speed_multiplier()`                 |
| `get_sim_speed()`          | `session.speed_name()`                       |
| `apply_or_send(action)`    | `session.process(SimCommand { action })`     |
| `spawn_elf(x,y,z)`        | Same, but calls session.process internally   |
| `poll_network()`           | Translate ServerMessages → SessionMessages   |
| `host_game(...)`           | Create session in Lobby state + start relay  |
| `join_game(...)`           | Connect to relay, session starts in Lobby    |
| `is_initialized()`         | `session.has_sim()`                          |
| `current_tick()`           | `session.current_tick()`                     |
| `save_game_json()`         | `session.serialize()`                        |
| `load_game_json(json)`     | `session.process(LoadState { json })`        |

### Query delegation

All sim queries (creature positions, nav nodes, world data, etc.) are
delegated unchanged. `SimBridge` still provides the GDScript-facing
`#[func]` methods, but they reach the sim through `session.sim()` (which
returns `Option<&SimState>`) instead of `self.sim.as_ref()`.

---

## 8. GDScript Changes

### game_session.gd simplification

The autoload singleton shrinks significantly. Session state no longer
lives here — it's inside the Rust `GameSession`. The autoload retains
only:

- `sim_seed: int` — set by the new-game menu, read once at startup.
- `tree_profile: Dictionary` — same.
- `load_save_path: String` — same.
- `multiplayer_mode: String` — still needed for the initial branch in
  `_ready()` (host vs. join vs. single-player), but no longer used
  per-frame.

The multiplayer config fields (`mp_port`, `mp_session_name`,
`mp_password`, `mp_max_players`, `mp_ticks_per_turn`,
`mp_relay_address`, `mp_player_name`) stay here for the menu → game
transition.

### main.gd simplification

The biggest win: `_process()` no longer has separate single-player and
multiplayer branches. The unified loop:

```gdscript
func _process(delta: float) -> void:
    var bridge: SimBridge = $SimBridge
    if not bridge.has_sim():
        return

    # In multiplayer, poll network first (translates to SessionMessages).
    if bridge.is_multiplayer():
        bridge.poll_network()

    # Advance time (single-player: from delta, multiplayer: from turns).
    var render_tick := bridge.compute_render_tick(delta)

    # Distribute render_tick to renderers (unchanged).
    $ElfRenderer.set_render_tick(render_tick)
    # ...
```

The `compute_render_tick(delta)` method on `SimBridge` encapsulates
the accumulator logic:
- In single-player: `delta * speed_mult → ticks → AdvanceTo →
  render_tick`.
- In multiplayer: interpolation between the last applied turn tick and
  the expected next turn tick.

This eliminates the `_sim_accumulator`, `_mp_time_since_turn`, and
`_seconds_per_tick` fields from `main.gd` — they move into the Rust
`GameSession` or `SimBridge`.

---

## 9. Session Events (Output)

The session produces output events for the UI, analogous to how the sim
produces `SimEvent`s. These are notifications about session-level state
changes:

```rust
pub enum SessionEvent {
    /// A player joined the session.
    PlayerJoined { name: String },
    /// A player left the session.
    PlayerLeft { name: String },
    /// The game started (sim initialized).
    GameStarted,
    /// Sim speed changed.
    SpeedChanged { speed: SessionSpeed },
    /// Game was paused.
    Paused { by: String },
    /// Game was resumed.
    Resumed { by: String },
    /// A sim event (passed through from the sim).
    Sim(SimEvent),
    /// Desync detected at tick N.
    DesyncDetected { tick: u64 },
    /// Game state was loaded from a save.
    StateLoaded,
}
```

`SessionEvent::Sim(SimEvent)` wraps sim events so the UI gets a single
event stream from the session. GDScript doesn't need to poll both the
session and the sim.

---

## 10. Determinism Implications

### What's synchronized in multiplayer

The relay synchronizes `SessionMessage`s, not raw sim commands. This
means:

- All clients see the same `StartGame`, `SimCommand`, `AdvanceTo`,
  `Pause`, `Resume`, and `SetSpeed` messages in the same order.
- Session state (including speed and pause) is identical across clients
  without extra bookkeeping.
- Desync detection can hash the full `GameSession` state (including
  session speed, player list, etc.), not just the sim.

### SimState determinism (unchanged)

The sim's determinism contract is unchanged: same seed + same command
stream = same state. The only change is that `speed` and `SetSimSpeed`
are removed from the sim, simplifying its state space.

### Save/load

Save files serialize the full `GameSession` state (or at minimum the
`SessionState::Playing` contents). This captures speed, player info, and
sim state in one blob. On load, the session transitions directly to the
loaded state.

---

## 11. Checksum and Desync Detection

Currently, desync detection hashes `SimState` via
`state_checksum()` (FNV-1a of JSON serialization). With the session
state machine, we have a choice:

**Option A: Keep sim-only checksums.** The sim is the only part with
complex determinism requirements. Session-level state (speed, pause,
players) is small and directly controlled by relay messages — if those
are delivered correctly, session state can't diverge independently.

**Option B: Hash the full session.** More thorough, catches any
divergence. Slightly more expensive (but the overhead is negligible
compared to sim serialization).

**Recommendation:** Option A for now (sim-only checksums), since that's
what's already implemented and tested. If we discover session-level
desync bugs, upgrade to Option B.

---

## 12. Migration Path

### Phase 1: Create `GameSession` struct in `elven_canopy_sim`

- Define `SessionState`, `SessionMessage`, `SessionEvent`,
  `SessionSpeed`.
- Implement `GameSession::process()` for all transitions.
- Add `GameSession` to the sim crate (it has no Godot dependencies).
- Write unit tests (see section 13).
- `SimState.speed` and `SimAction::SetSimSpeed` remain for backward
  compatibility during migration.

### Phase 2: Wire `GameSession` into `SimBridge`

- Replace `SimBridge.sim: Option<SimState>` with
  `SimBridge.session: GameSession`.
- Translate all `SimBridge` methods to go through the session.
- `apply_or_send()` becomes `session.process(SimCommand { ... })`.
- `step_to_tick()` becomes `session.process(AdvanceTo { tick })`.
- `set_sim_speed()` becomes `session.process(SetSpeed { ... })`.
- Multiplayer `poll_network()` translates `ServerMessage`s to
  `SessionMessage`s.

### Phase 3: Remove `SimSpeed` from SimState

- Remove `speed: SimSpeed` from `SimState`.
- Remove `SimAction::SetSimSpeed` from `SimAction`.
- Remove `SimEventKind::SpeedChanged` from `SimEventKind`.
- Speed is now purely a session concern.
- Update save/load to serialize session state, not just sim state.

### Phase 4: Simplify GDScript

- Unify `_process()` in `main.gd` (single loop, no branching).
- Move accumulator logic into Rust (`compute_render_tick(delta)`).
- Slim down `game_session.gd`.

### Phase 5: Clean up

- Remove dead code (`is_multiplayer_mode`, `is_host`, `game_started`
  fields from `SimBridge`).
- Update documentation (CLAUDE.md, design_doc.md).

---

## 13. Test Plan

All tests live in `elven_canopy_sim` since `GameSession` is a pure Rust
struct with no Godot dependencies. Tests use the existing `test_config()`
pattern (small 64³ world, fast construction).

### 13.1 State transition tests

These verify the state machine's transition logic — that each message
produces the expected state change and that invalid transitions are
rejected.

**Lobby → Playing transition:**
- Create a `GameSession` in `Lobby` state with seed and config.
- Send `StartGame`. Assert state is now `Playing` with a `SimState` at
  tick 0.
- Assert the sim's seed matches the lobby seed.
- Assert the sim's config matches the lobby config.

**Playing → Paused → Playing:**
- Start a game, send a few `AdvanceTo` messages to advance the sim.
- Send `Pause { by }`. Assert state is `Paused` with the sim frozen at
  the last tick.
- Attempt to send `AdvanceTo`. Assert it's rejected (state doesn't
  change, or an error event is returned).
- Attempt to send `SimCommand`. Assert it's rejected while paused.
- Send `Resume { by }`. Assert state is `Playing` with the previous
  speed restored.
- Send another `AdvanceTo`. Assert the sim advances normally.

**Speed changes:**
- In `Playing` state, send `SetSpeed { Fast }`. Assert the session
  speed changes. Assert `speed_multiplier()` returns 2.0.
- Send `SetSpeed { VeryFast }`. Assert multiplier is 5.0.

**Invalid transitions:**
- In `Lobby` state, send `SimCommand`. Assert no state change (no-op or
  error event).
- In `Lobby` state, send `AdvanceTo`. Assert no state change.
- In `Lobby` state, send `Pause`. Assert no state change.
- In `Paused` state, send `Pause`. Assert no state change (already
  paused).
- In `Playing` state, send `StartGame`. Assert no state change (already
  started).

**Player management:**
- Send `PlayerJoined` in `Lobby`. Assert player appears in the player
  list.
- Send `PlayerLeft` for a connected player. Assert they're removed.
- Send `PlayerLeft` for a nonexistent player. Assert no crash or state
  change.

### 13.2 Command buffering and flushing

These verify that commands are properly buffered and applied during
tick advancement.

**Commands apply on AdvanceTo:**
- In `Playing` state, send `SimCommand(SpawnCreature)`. Assert the
  creature does NOT yet exist in the sim (command is buffered).
- Send `AdvanceTo { tick: current + 1 }`. Assert the creature now
  exists.

**Multiple commands flush in order:**
- Buffer two `SimCommand`s (spawn elf A, spawn elf B).
- Send `AdvanceTo`. Assert both creatures exist and were created in the
  order they were buffered.

**Commands from multiple players:**
- Buffer commands from player 1 and player 2.
- Send `AdvanceTo`. Assert both are applied. Assert deterministic
  ordering (by player ID or insertion order — match the relay's
  canonical ordering).

### 13.3 Determinism tests

These verify that two sessions processing the same message stream
produce identical results.

**Two sessions, same messages:**
- Create two `GameSession`s with the same seed and config.
- Feed an identical sequence of `SessionMessage`s to both (start game,
  spawn creatures, advance ticks, change speed, pause, resume, more
  ticks).
- Assert `state_checksum()` is identical after each message.

**Determinism across speed changes:**
- Run a session at Normal speed for N ticks.
- Run another session: Normal for N/2, change to Fast, then advance to
  the same final tick.
- Assert checksums are identical (speed doesn't affect sim logic, only
  wall-clock pacing).

### 13.4 Save/load round-trip

- Create a session, advance to tick 1000, spawn some creatures,
  build some structures.
- Serialize the session state.
- Create a new session, load the serialized state.
- Assert the loaded session's sim has the same tick, creatures,
  structures, and checksum.
- Advance both sessions by the same amount. Assert checksums match.

### 13.5 Single-player equivalence

- Run a "single-player-style" message sequence (local player, no relay,
  direct `process()` calls).
- Run the same sequence through a simulated relay (serialize commands
  to wire format, translate back to `SessionMessage`s).
- Assert the final states are identical.

### 13.6 Edge cases

- **Empty AdvanceTo:** Send `AdvanceTo { tick: current_tick }` (no
  advancement). Assert no crash, no state change.
- **Backward tick:** Send `AdvanceTo { tick: current_tick - 10 }`.
  Assert rejection or no-op.
- **AdvanceTo with no commands:** Advance 10,000 ticks with no commands.
  Assert the sim processes events normally (creature heartbeats, tree
  growth, etc.).
- **StartGame with zero players:** Assert handled gracefully (single-
  player has exactly one player, but the session should still start).
- **Rapid pause/resume:** Pause and resume several times in quick
  succession. Assert the sim state is consistent and tick doesn't
  advance during paused periods.

---

## 14. File-by-File Change Plan

### New files

| File                                      | Purpose                                    |
|-------------------------------------------|--------------------------------------------|
| `elven_canopy_sim/src/session.rs`         | `GameSession`, `SessionState`, `SessionMessage`, `SessionEvent`, `SessionSpeed`, `PlayerSlot`. The core state machine. |
| `elven_canopy_sim/src/session.rs` (tests) | Inline `#[cfg(test)] mod tests` with the test plan from section 13. |

### Modified files (Rust)

**`elven_canopy_sim/src/lib.rs`**
- Add `pub mod session;` module declaration.
- Re-export key types (`GameSession`, `SessionMessage`, `SessionSpeed`,
  `SessionEvent`).

**`elven_canopy_sim/src/sim.rs`**
- Phase 1: Add `pending_commands: Vec<SimCommand>` field (serde-skip).
- Phase 1: Add `enqueue_command()` and `advance_to()` methods. Keep
  existing `step()` as a convenience wrapper.
- Phase 3: Remove `speed: SimSpeed` field. Remove the `SetSimSpeed`
  match arm from `apply_command()`.
- Phase 3: Remove `SpeedChanged` from the events emitted by
  `apply_command()`.
- Phase 3: Remove `sim_speed_multiplier()` (or any speed-related method
  if one exists on `SimState` — currently `speed.multiplier()` is called
  on the `SimSpeed` type, not on `SimState`, so no method to remove).

**`elven_canopy_sim/src/command.rs`**
- Phase 3: Remove `SimAction::SetSimSpeed` variant.
- Update module docstring.

**`elven_canopy_sim/src/event.rs`**
- Phase 3: Remove `SimEventKind::SpeedChanged` variant.
- Update module docstring.

**`elven_canopy_sim/src/types.rs`**
- Phase 3: `SimSpeed` may either be removed entirely (if nothing else
  uses it) or kept as a re-export of `SessionSpeed`. Check all
  references first.

**`elven_canopy_gdext/src/sim_bridge.rs`**
- Phase 2: Replace `sim: Option<SimState>` with `session: GameSession`.
- Phase 2: Remove `is_multiplayer_mode`, `is_host`, `game_started`,
  `mp_ticks_per_turn` fields — replaced by session state queries.
- Phase 2: Rewrite `apply_or_send()` →
  `session.process(SessionMessage::SimCommand { ... })`.
- Phase 2: Rewrite `step_to_tick()` →
  `session.process(SessionMessage::AdvanceTo { ... })`.
- Phase 2: Rewrite `set_sim_speed()` →
  `session.process(SessionMessage::SetSpeed { ... })`.
- Phase 2: Rewrite `poll_network()` to translate `ServerMessage`s to
  `SessionMessage`s and feed them to `session.process()`.
- Phase 2: All sim query methods (`get_creature_positions`, etc.)
  delegate through `session.sim()` instead of `self.sim.as_ref()`.
- Phase 5: Remove dead fields and methods.
- Update module docstring.

**`elven_canopy_gdext/src/lib.rs`**
- No changes expected (just re-exports the gdext entry point).

### Modified files (GDScript)

**`godot/scripts/main.gd`**
- Phase 4: Unify `_process()` — remove single-player vs. multiplayer
  branching. Call `bridge.compute_render_tick(delta)` which handles
  both modes internally.
- Phase 4: Remove `_sim_accumulator`, `_mp_time_since_turn`,
  `_seconds_per_tick` fields.
- Phase 4: Simplify `_ready()` — the session handles lobby → playing
  transition, so `main.gd` just tells the session to start.

**`godot/scripts/game_session.gd`**
- Phase 4: Remove fields that are now in the Rust session (no structural
  changes needed in Phase 1–3, since GDScript still reads these during
  startup).

**`godot/scripts/action_toolbar.gd`**
- No changes to the toolbar itself — it still emits `speed_changed`
  signals. The handler in `main.gd` calls `bridge.set_sim_speed()`,
  which goes through the session.

### Documentation updates

**`CLAUDE.md`**
- Update "Implementation Status" to reflect the new session architecture.
- Update "Codebase Patterns and Gotchas" to replace the `apply_or_send`
  side-effect note.
- Update project structure to mention `session.rs`.

**`docs/design_doc.md`**
- Update §4 to describe the session state machine model.
- Note that speed/pause are session concerns, not sim concerns.

**`docs/tracker.md`**
- Mark F-session-sm as In Progress when work begins, Done when complete.

---

## 15. Relationship to Existing Multiplayer Design

This draft **refines and extends** the relay design in
`docs/drafts/multiplayer_relay.md`. It does not contradict or replace it.
The relay remains the coordinator — this draft formalizes what happens
*inside each client* when relay messages arrive.

Key relationships:

- The relay's `ServerMessage::Turn` maps to a batch of
  `SessionMessage::SimCommand` + `SessionMessage::AdvanceTo`.
- The relay's `Paused`/`Resumed` messages map to
  `SessionMessage::Pause`/`Resume`.
- The relay's `GameStart` maps to `SessionMessage::StartGame`.
- Mid-game join (`SnapshotLoad`) maps to `SessionMessage::LoadState`.
- Desync detection (`DesyncDetected`) becomes a `SessionEvent`.
- The relay protocol (`ClientMessage`/`ServerMessage`) is unchanged —
  it's a wire format, not a state machine input.

The gdext bridge translates between wire protocol and session messages.
This translation layer is explicit and testable.

---

## 16. Open Questions

1. **Should `Paused` be a state or a field on `Playing`?** Making it a
   state provides the strongest structural guarantee (can't tick while
   paused). Making it a field is simpler code (no ownership shuffling).
   The draft uses a separate state; revisit if the ergonomics are painful.

2. **Command tick assignment.** Currently, `apply_or_send` assigns
   `tick: sim.tick + 1` to commands. In the session model, when are
   command ticks assigned? Options:
   - At enqueue time: `sim.tick + 1` (current behavior).
   - At flush time: all pending commands get the target tick from
     `AdvanceTo`.
   The second option is simpler and matches multiplayer semantics (the
   relay's Turn assigns the tick). Recommend the second approach.

3. **Accumulator ownership.** The wall-clock accumulator that converts
   delta time to ticks currently lives in `main.gd`. Should it move into
   the Rust `GameSession` or `SimBridge`? Moving it to Rust makes the
   GDScript simpler (just call `compute_render_tick(delta)`) but means
   the Rust side needs to know about frame deltas. Recommend moving it
   to `SimBridge` (not `GameSession`, since wall-clock concerns are
   Godot-specific, not pure sim logic).

4. **Session serialization format.** Should save files change format?
   Currently they serialize `SimState` directly. Adding session metadata
   (speed, players) requires either wrapping the sim JSON in a session
   envelope or extending the sim's serialization. Recommend a session
   envelope:
   ```json
   {
     "version": 1,
     "session_speed": "Normal",
     "sim": { ... existing sim JSON ... }
   }
   ```
   With backward compatibility: if the top-level JSON has a `tick` field,
   it's a legacy sim-only save; if it has a `version` field, it's a
   session save.
