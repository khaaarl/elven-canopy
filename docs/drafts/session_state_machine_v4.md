# Session as a Message-Driven Struct (v4)

Draft design for formalizing the game session as a message-driven struct
whose fields only change in response to typed messages processed through
a single entry point.

**Supersedes:** `session_state_machine_v3.md`, `session_state_machine_v2.md`,
and `session_state_machine.md` (v1). All kept for comparison.

**Key changes from v3:**

- All pseudocode uses real API signatures from the codebase, or
  explicitly notes the refactoring required. `spawn_creature_internal()`
  replaced with the real `spawn_creature(&mut self, species, position,
  events)` signature, plus a documented refactor to return `CreatureId`.
  `inventory.add()` replaced with the real free function `add_item()`.
  `GroundPile::new()` replaced with struct literal construction.
  `find_surface_position()` specified as a new method.
- Explicit analysis of why buffered-command-same-tick semantics are
  acceptable for a management sim (not just acknowledged as a change).
- `PartialEq` not derived on `SessionMessage` (due to `SimAction` and
  `GameConfig` lacking it). Derived on `SessionEvent` only. Test
  assertions on messages use pattern matching or serialize-then-compare.
- Relay described honestly: it manages turn pacing (decides when to emit
  turns and what tick target they carry) but does not understand pause
  semantics. Pause/resume are `SessionMessage`s that clients process;
  the relay just delivers them.
- Speed synchronization with the relay uses explicit `SessionSpeed` to
  `ticks_per_turn` translation in `SimBridge`.
- PRNG sequence change explicitly acknowledged: new seeds produce
  different results after this change.
- `DesyncDetected` added as a `SessionMessage` variant.
- `StartGame` on an already-loaded sim emits `SimUnloaded` before
  `GameStarted`.
- `new_multiplayer()` accepts `host_id` as a parameter.
- `LocalRelay` lives in its own file (`local_relay.rs` in gdext crate).
- Accumulator cap (0.1s) is the sole spiral-of-death protection;
  `max_ticks_per_frame` removed as redundant.
- Chat messages noted as handled by SimBridge outside the session.
- Checksum logic stays in SimBridge, not in the session.
- `is_host()` replacement method specified.
- Phase 2 includes sim-level unit tests for new `SimAction` variants.
- Phase 5 lists existing tests that reference `SimSpeed`/`SetSimSpeed`.
- Test case added for pause -> command -> rejected AdvanceTo -> resume
  -> accepted AdvanceTo.
- Quantity types corrected to `u32` throughout (matching real inventory
  API).
- Internal cross-references verified.

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

    /// Create a new multiplayer session. `host_id` is the session
    /// player who can start/load/unload games (typically the player
    /// who created the lobby). The local player is added to the
    /// players map; additional players arrive via PlayerJoined.
    pub fn new_multiplayer(
        local_id: SessionPlayerId,
        host_id: SessionPlayerId,
    ) -> Self {
        let mut players = BTreeMap::new();
        players.insert(local_id, PlayerSlot {
            id: local_id,
            name: String::new(), // filled by PlayerJoined
            is_local: true,
        });
        Self {
            players,
            host_id,
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
///
/// Note: PartialEq is NOT derived. SimAction deliberately does not
/// derive PartialEq (it contains Vec fields where element-wise
/// comparison is unnecessary overhead), and GameConfig contains f32/f64
/// fields where PartialEq would be misleading. For test assertions on
/// SessionMessage, use pattern matching or serialize-then-compare.
#[derive(Clone, Debug, Serialize, Deserialize)]
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

    /// Desync detected by the relay. The relay sends this when client
    /// checksums diverge. Translated from ServerMessage::DesyncDetected
    /// by SimBridge.
    DesyncDetected { tick: u64 },
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
                // If a sim is already loaded, emit SimUnloaded first.
                if self.sim.is_some() {
                    self.sim = None;
                    events.push(SessionEvent::SimUnloaded);
                }
                let mut sim = SimState::with_config(seed, config);
                // Initial creature spawning -- data-driven from config
                // so all clients produce identical results.
                // See section 7 for spawn_initial_creatures() details.
                let mut spawn_events = Vec::new();
                sim.spawn_initial_creatures(&mut spawn_events);
                self.sim = Some(sim);
                self.paused = false;
                self.pending_commands.clear();
                events.push(SessionEvent::GameStarted);
                // Include any sim events from initial spawning.
                for se in spawn_events {
                    events.push(SessionEvent::Sim(se));
                }
            }

            SessionMessage::LoadSim { json } => {
                match SimState::from_json(&json) {
                    Ok(sim) => {
                        // If a sim is already loaded, emit SimUnloaded.
                        if self.sim.is_some() {
                            events.push(SessionEvent::SimUnloaded);
                        }
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
                    // time. See section 4.1 for why this is acceptable.
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

            SessionMessage::DesyncDetected { tick } => {
                events.push(SessionEvent::DesyncDetected { tick });
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

### 4.1 Buffered commands all fire at the AdvanceTo tick

When a player issues multiple commands during a single frame (or during
the interval between two `AdvanceTo` messages), all buffered commands
receive the same tick -- the `AdvanceTo` target. This means creature AI
runs for the entire tick interval before any of those commands fire.

**This is acceptable for a management sim.** The player's commands in
Elven Canopy are orders, not real-time actions: `DesignateBuild`,
`SpawnCreature`, `CreateTask`, etc. None of them are time-sensitive to
the individual tick level. A platform designation that fires at tick 500
vs. tick 497 produces no perceptible difference.

**This matches multiplayer semantics.** In multiplayer, the relay
batches commands into turns and assigns them all the same tick target.
Making single-player use the same batching means:

1. Testing single-player tests the same command-timing behavior as
   multiplayer. Bugs that would only appear under batching are caught
   in SP development.
2. The replay system (future) can record `SessionMessage` streams and
   replay them identically regardless of original mode.
3. The code path is simpler -- one buffering model instead of two.

The previous `apply_or_send()` pattern fired each command at `tick + 1`
immediately, which was convenient but created a divergence between SP
and MP command timing that would have needed reconciliation eventually.

### Key details in `AdvanceTo`

**Backward tick guard:** If `tick <= sim.tick`, the message is rejected
silently. Pending commands are preserved (not cleared) so they apply on
the next valid `AdvanceTo`. This prevents accidental time reversal and
handles edge cases like duplicate network messages gracefully.

**Command tick assignment:** All pending commands receive the
`AdvanceTo` message's target tick. This is different from the current
`apply_or_send()` pattern which assigns `sim.tick + 1` at enqueue time.
The new behavior matches multiplayer semantics where the relay's `Turn`
message assigns the tick (see section 4.1).

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
///
/// PartialEq is derived here (unlike SessionMessage) because
/// SessionEvent's variants don't contain SimAction or GameConfig.
/// SimEvent derives Serialize/Deserialize which enables
/// serialize-then-compare for test assertions if needed.
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
    /// Desync detected at the given tick.
    DesyncDetected { tick: u64 },
    /// Something went wrong.
    Error { message: String },
}
```

Note: `PartialEq` is derived on `SessionEvent` but **not** on
`SessionMessage`. `SessionMessage` contains `SimAction` (which
deliberately does not derive `PartialEq` -- see the comment in
`command.rs`) and `GameConfig` (which contains `f32`/`f64` fields
where `PartialEq` would be misleading). For test assertions on
`SessionMessage` values, use pattern matching:

```rust
// Good: pattern matching
let msg = /* ... */;
assert!(matches!(msg, SessionMessage::StartGame { seed: 42, .. }));

// Good: serialize-then-compare for full equality
assert_eq!(
    serde_json::to_string(&msg_a).unwrap(),
    serde_json::to_string(&msg_b).unwrap(),
);
```

`SessionEvent` _can_ derive `PartialEq` because its `Sim(SimEvent)`
variant contains only `SimEvent` (which has `SimEventKind` with simple
data) and the other variants contain `SessionSpeed`, `SessionPlayerId`,
and `String` -- all of which support `PartialEq`. `SimEvent` will need
`PartialEq` derived on it and `SimEventKind`; neither contains
`SimAction` or floating-point fields, so this is safe.

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
| `add_creature_item()` | directly modifies `creature.inventory` | calls `inventory::add_item()` |
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
    quantity: u32, // matches inventory::add_item() signature
},
SimAction::AddGroundPileItem {
    position: VoxelCoord,
    item_kind: ItemKind,
    quantity: u32, // matches inventory::add_item() signature
},
```

In practice, these four actions are only used during initial game setup.
Once `spawn_initial_creatures()` (section 7) is implemented, they could
be simplified to internal setup actions rather than full `SimAction`
variants. However, making them proper commands first ensures they go
through the deterministic command path, which is the right ordering --
we can simplify later if they're truly never needed as player-facing
commands.

The `apply_command()` handlers for these variants use the real APIs:

```rust
// In sim.rs apply_command():
SimAction::AddCreatureItem { creature_id, item_kind, quantity } => {
    if let Some(creature) = self.creatures.get_mut(creature_id) {
        inventory::add_item(
            &mut creature.inventory,
            *item_kind,
            *quantity,
            Some(*creature_id),
            None, // reserved_by
        );
    }
}
SimAction::AddGroundPileItem { position, item_kind, quantity } => {
    let pile = self.ground_piles
        .entry(*position)
        .or_insert_with(|| inventory::GroundPile {
            position: *position,
            items: Vec::new(),
        });
    inventory::add_item(
        &mut pile.items,
        *item_kind,
        *quantity,
        None, // owner
        None, // reserved_by
    );
}
```

---

## 7. Initial Creature Spawning

Currently, initial creature spawning (5 elves, 5 capybaras, 3 each of
boar/deer/monkey/squirrel, plus food/rest variation and initial bread)
is duplicated in `main.gd` (`_ready()` and `_on_mp_game_started()`).
This moves entirely into the sim.

### Required refactoring: spawn_creature must return CreatureId

The current `spawn_creature()` signature is:

```rust
// Current signature in sim.rs (line 1591):
fn spawn_creature(
    &mut self,
    species: Species,
    position: VoxelCoord,
    events: &mut Vec<SimEvent>,
)
```

It does not return the `CreatureId` of the spawned creature.
`spawn_initial_creatures()` needs to set food/rest/inventory on
each creature after spawning, which requires knowing the ID.

**Refactoring:** Change `spawn_creature()` to return `Option<CreatureId>`:

```rust
fn spawn_creature(
    &mut self,
    species: Species,
    position: VoxelCoord,
    events: &mut Vec<SimEvent>,
) -> Option<CreatureId> {
    // ... existing logic (find nav node, generate ID, create Creature,
    //     schedule events, push SimEvent) ...
    // Currently returns () after inserting. Change the two early-return
    // points (no nav node) to return None, and the success path to
    // return Some(creature_id).
    Some(creature_id)
}
```

This is a backward-compatible change -- callers that currently ignore
the return value continue to work. The only call site is
`apply_command()` which discards the result.

### Required new method: find_surface_position

The current GDScript code (main.gd lines 200-202) scans upward to
find a valid surface position for ground piles:

```gdscript
var pile_y := 1
while not bridge.validate_build_air(128, pile_y, 138) and pile_y < 10:
    pile_y += 1
```

This logic needs a Rust equivalent. Add to `SimState`:

```rust
/// Find the lowest Air voxel at (x, z) starting from y=1 (above
/// ForestFloor). Scans upward until Air is found or y exceeds the
/// world height. Returns the position with the found y, or y=1 if
/// no Air is found (fallback).
fn find_surface_position(&self, x: i32, z: i32) -> VoxelCoord {
    for y in 1..self.world.size_y() as i32 {
        let pos = VoxelCoord::new(x, y, z);
        if !self.world.get(pos).is_solid() {
            return pos;
        }
    }
    VoxelCoord::new(x, 1, z) // fallback
}
```

### spawn_initial_creatures implementation

```rust
impl SimState {
    /// Spawn the initial set of creatures for a new game.
    /// Called by GameSession::process(StartGame).
    ///
    /// Uses data from config.initial_creatures list so it's
    /// deterministic and not hardcoded. All spawning uses the sim's
    /// PRNG for determinism.
    pub fn spawn_initial_creatures(&mut self, events: &mut Vec<SimEvent>) {
        let specs = self.config.initial_creatures.clone();
        for spec in &specs {
            for i in 0..spec.count {
                // 1. Spawn creature at default position
                //    (near world center, snapped to nav).
                let creature_id = match self.spawn_creature(
                    spec.species,
                    spec.spawn_position,
                    events,
                ) {
                    Some(id) => id,
                    None => continue, // No valid nav node; skip.
                };

                // 2. Set food level from per-index override or default.
                let food_pct = spec.food_pcts
                    .get(i)
                    .copied()
                    .unwrap_or(100);
                if food_pct < 100 {
                    if let Some(creature) = self.creatures.get_mut(&creature_id) {
                        let max = self.species_table[&spec.species].food_max;
                        creature.food = max * food_pct as i64 / 100;
                    }
                }

                // 3. Set rest level from per-index override or default.
                let rest_pct = spec.rest_pcts
                    .get(i)
                    .copied()
                    .unwrap_or(100);
                if rest_pct < 100 {
                    if let Some(creature) = self.creatures.get_mut(&creature_id) {
                        let max = self.species_table[&spec.species].rest_max;
                        creature.rest = max * rest_pct as i64 / 100;
                    }
                }

                // 4. Add initial items from per-index override.
                let bread_count = spec.bread_counts
                    .get(i)
                    .copied()
                    .unwrap_or(0);
                if bread_count > 0 {
                    if let Some(creature) = self.creatures.get_mut(&creature_id) {
                        inventory::add_item(
                            &mut creature.inventory,
                            inventory::ItemKind::Bread,
                            bread_count,
                            Some(creature_id),
                            None, // reserved_by
                        );
                    }
                }
            }
        }

        // 5. Place initial ground piles from config.
        let pile_specs = self.config.initial_ground_piles.clone();
        for pile_spec in &pile_specs {
            // Find valid surface position (scan upward for air).
            let pos = self.find_surface_position(
                pile_spec.position.x,
                pile_spec.position.z,
            );
            let pile = self.ground_piles
                .entry(pos)
                .or_insert_with(|| inventory::GroundPile {
                    position: pos,
                    items: Vec::new(),
                });
            inventory::add_item(
                &mut pile.items,
                pile_spec.item_kind,
                pile_spec.quantity,
                None, // owner
                None, // reserved_by
            );
        }
    }
}
```

### PRNG sequence change

**Moving `spawn_initial_creatures()` into the session's `StartGame`
processing changes all subsequent PRNG values for every seed.** Starting
a new game with a given seed will produce different creature IDs, names,
and world evolution than before this change. All existing test baselines
must be regenerated. This is acceptable at the current stage of
development -- there are no published seeds, no user-facing replays,
and no cross-version save compatibility guarantees.

### Config additions

`GameConfig` gains:

```rust
/// Specification for initial creature spawning at game start.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitialGroundPileSpec {
    pub position: VoxelCoord,
    pub item_kind: ItemKind,
    pub quantity: u32, // matches inventory::add_item() signature
}

// Added to GameConfig:
pub initial_creatures: Vec<InitialCreatureSpec>,
pub initial_ground_piles: Vec<InitialGroundPileSpec>,
```

The defaults reproduce the current hardcoded behavior from `main.gd`:
5 elves with food_pcts `[100, 90, 70, 60, 48]`, rest_pcts
`[100, 95, 80, 60, 45]`, bread_counts `[0, 1, 2, 3, 4]`; 5 capybaras;
3 each of boar/deer/monkey/squirrel; one ground bread pile of 5 at
position (128, 0, 138) (y resolved by `find_surface_position()`).

---

## 8. Tick Pacing: Local Relay for Single-Player

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

**Multiplayer relay:** The relay manages turn pacing -- it runs on a
timer and decides when to emit turns and what tick target they carry.
Every turn interval (e.g., 50ms at 1x speed), it broadcasts a `Turn`
message with `sim_tick_target`. The gdext bridge translates this to
`AdvanceTo { tick }`.

The relay does **not** understand pause semantics. When a client
pauses, the session processes `SessionMessage::Pause` and rejects
subsequent `AdvanceTo` messages. The relay keeps emitting turns on its
cadence regardless; paused clients simply ignore the `AdvanceTo`
messages (the backward tick guard in `process()` handles stale
`AdvanceTo`s after resume). When the client resumes, it processes
the next `AdvanceTo` normally, catching up to the relay's current
tick target.

The relay also translates `Pause`/`Resume` wire messages into
`SessionMessage`s that it broadcasts to all clients, so all clients
agree on the paused state. But the relay itself doesn't pause its
turn emission -- it's a transport-level tick authority, not a
game-level pause enforcer.

**Empty turns:** When the relay emits a `Turn` with no commands, it
still produces an `AdvanceTo`. This is correct and intentional -- the
sim needs to advance time even when no player commands are issued
(heartbeats, creature activations, tree growth, etc. are internal
scheduled events).

**Local relay (single-player):** A Rust struct that receives
`delta_seconds` each frame, maintains an accumulator, and produces
`AdvanceTo` messages at the appropriate rate. It lives in its own file
(`elven_canopy_gdext/src/local_relay.rs`) and implements the same
tick-pacing logic as the real relay's turn emission.

```rust
// In elven_canopy_gdext/src/local_relay.rs

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
}

impl LocalRelay {
    pub fn new(seconds_per_tick: f64) -> Self {
        Self {
            accumulator: 0.0,
            seconds_per_tick,
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
        // Cap to prevent unbounded growth (e.g. alt-tab away then
        // return). 0.1 seconds = 100 ticks at 1x, 200 at 2x, 500
        // at 5x -- well within the safe processing range. This is
        // the sole spiral-of-death protection; no separate
        // max_ticks_per_frame cap is needed because the accumulator
        // cap already bounds the tick count.
        if self.accumulator > 0.1 {
            self.accumulator = 0.1;
        }
        let ticks = (self.accumulator / self.seconds_per_tick) as u64;
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

## 9. Networking Is External

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
(`ClientMessage`/`ServerMessage`) and `SessionMessage`:

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
                    self.session.process(SessionMessage::SimCommand {
                        from: session_pid,
                        action,
                    });
                }
            }
            // Then advance to the turn's target tick.
            self.session.process(SessionMessage::AdvanceTo {
                tick: sim_tick_target,
            });
            // Checksum logic remains in SimBridge: compute and send
            // checksums after turns, same as today. The checksum
            // logic does not move into the session.
        }
        ServerMessage::GameStart { seed, config_json } => {
            // StartGame carries seed and config as parameters.
            let config = parse_config(&config_json);
            self.session.process(SessionMessage::StartGame {
                seed: seed as u64,
                config,
            });
        }
        ServerMessage::Paused { by } => {
            self.session.process(SessionMessage::Pause {
                by: SessionPlayerId(by.0),
            });
        }
        ServerMessage::Resumed { by } => {
            self.session.process(SessionMessage::Resume {
                by: SessionPlayerId(by.0),
            });
        }
        ServerMessage::SpeedChanged { ticks_per_turn } => {
            // Translate relay ticks_per_turn back to SessionSpeed.
            // See section 9.1 for the mapping.
            let speed = self.ticks_per_turn_to_speed(ticks_per_turn);
            self.session.process(SessionMessage::SetSpeed { speed });
        }
        ServerMessage::DesyncDetected { tick } => {
            self.session.process(SessionMessage::DesyncDetected { tick });
        }
        ServerMessage::SnapshotLoad { data, .. } => {
            if let Ok(json) = String::from_utf8(data) {
                self.session.process(SessionMessage::LoadSim { json });
            }
        }
        ServerMessage::PlayerJoined { player } => {
            self.session.process(SessionMessage::PlayerJoined {
                id: SessionPlayerId(player.id.0),
                name: player.name,
            });
        }
        ServerMessage::PlayerLeft { player_id, .. } => {
            self.session.process(SessionMessage::PlayerLeft {
                id: SessionPlayerId(player_id.0),
            });
        }
        ServerMessage::ChatBroadcast { .. } => {
            // Chat is handled by SimBridge outside the session --
            // it's a UI/social concern, not game state. The session
            // doesn't need a Chat message variant.
            self.mp_events.push(/* format chat for GDScript */);
        }
        _ => { /* Welcome, Rejected, SnapshotRequest handled by SimBridge */ }
    }
}
```

### 9.1 Speed synchronization with the relay

The relay wire protocol uses `ticks_per_turn: u32` to control pacing,
not speed names. `SimBridge` maintains the mapping:

```rust
impl SimBridge {
    /// Base ticks per turn at 1x speed. The relay emits a turn every
    /// 50ms of game time at 1x, containing 50 ticks of sim time.
    const BASE_TICKS_PER_TURN: u32 = 50;

    /// Convert a SessionSpeed to the relay's ticks_per_turn value.
    fn speed_to_ticks_per_turn(speed: SessionSpeed) -> u32 {
        match speed {
            SessionSpeed::Normal => Self::BASE_TICKS_PER_TURN,      // 50
            SessionSpeed::Fast => Self::BASE_TICKS_PER_TURN * 2,    // 100
            SessionSpeed::VeryFast => Self::BASE_TICKS_PER_TURN * 5, // 250
        }
    }

    /// Convert the relay's ticks_per_turn back to a SessionSpeed.
    /// Uses nearest-match since the relay might send arbitrary values.
    fn ticks_per_turn_to_speed(&self, tpt: u32) -> SessionSpeed {
        if tpt >= Self::BASE_TICKS_PER_TURN * 4 {
            SessionSpeed::VeryFast
        } else if tpt >= Self::BASE_TICKS_PER_TURN * 2 {
            SessionSpeed::Fast
        } else {
            SessionSpeed::Normal
        }
    }
}
```

When the session's speed changes (via `set_sim_speed()` from GDScript),
`SimBridge`:

1. Processes `SessionMessage::SetSpeed` locally.
2. If multiplayer, sends `ClientMessage::SetSpeed { ticks_per_turn }`
   to the relay using `speed_to_ticks_per_turn()`.
3. If the speed change involves pause/resume, also sends
   `ClientMessage::RequestPause` or `ClientMessage::RequestResume`.

When `ServerMessage::SpeedChanged { ticks_per_turn }` arrives from the
relay, `SimBridge` translates it back to `SessionSpeed` via
`ticks_per_turn_to_speed()` and processes `SessionMessage::SetSpeed`.

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

## 10. SimBridge Refactoring

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
            // Checksum computation and sending stays here, not in session.
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

    /// Whether this client is the session host. Replaces the old
    /// `is_host` boolean field.
    #[func]
    fn is_host(&self) -> bool {
        self.session.host_id == self.local_player_id
    }

    /// Whether the session currently has a sim loaded.
    #[func]
    fn has_sim(&self) -> bool {
        self.session.has_sim()
    }

    /// Whether we're in multiplayer mode.
    #[func]
    fn is_multiplayer(&self) -> bool {
        self.net_client.is_some()
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

## 11. Delayed Command Application: Audit Results

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
`spawn_initial_creatures()` (section 7). The sim handles all initial
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

## 12. GDScript Changes

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

## 13. Save Files

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

## 14. Determinism

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

**Checksum computation stays in SimBridge**, not in the session. After
each turn is applied, SimBridge computes the checksum and sends
`ClientMessage::Checksum { tick, hash }` to the relay, same as today.
The session doesn't know about checksums -- they're a multiplayer
transport concern.

When the relay detects a mismatch, it sends
`ServerMessage::DesyncDetected { tick }`. SimBridge translates this to
`SessionMessage::DesyncDetected { tick }`, which the session processes
and emits as `SessionEvent::DesyncDetected { tick }`.

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

## 15. Test Plan

All tests in `elven_canopy_sim` -- no Godot dependency needed. Use the
existing `test_config()` pattern (small 64x64x64 world).

### 15.1 Message processing basics

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
- Assert `session.current_tick() == 0`.
- Assert initial creatures exist (from `spawn_initial_creatures()`).

**StartGame when sim already loaded:**
- Start a game. Advance to tick 100.
- Send `StartGame { seed: 99, config }` again.
- Assert `SessionEvent::SimUnloaded` emitted before `GameStarted`.
- Assert new sim is loaded with tick 0.

**LoadSim / UnloadSim:**
- Create a session. Send `StartGame`. Advance to tick 1000.
- Serialize the sim: `let json = session.sim.as_ref().unwrap().to_json()`.
- Send `UnloadSim`. Assert `session.has_sim()` is false.
- Send `LoadSim { json }`. Assert sim is back, tick is 1000.

**UnloadSim resets pending commands:**
- Buffer some SimCommands. Send `UnloadSim`. Assert pending_commands
  is empty.

### 15.2 Command buffering and tick advancement

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

### 15.3 Pause / resume

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

**Pause -> command -> rejected AdvanceTo -> resume -> accepted AdvanceTo:**
- Start a game at tick 0.
- Send `Pause`.
- Send `SimCommand(SpawnCreature)`.
- Send `AdvanceTo { tick: 100 }`. Assert rejected (paused), tick still 0.
- Assert creature does NOT exist yet.
- Send `Resume`.
- Send `AdvanceTo { tick: 100 }`. Assert accepted, tick is now 100.
- Assert creature now exists (buffered command applied at tick 100).

**Double pause is a no-op:**
- Pause. Pause again. Assert `paused` is still true, only one
  `Paused` event emitted.

**Resume while not paused is a no-op:**
- Assert no `Resumed` event.

### 15.4 AdvanceTo backward tick guard

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

### 15.5 Speed

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

### 15.6 Determinism

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

### 15.7 Save/load round-trip

- Create session, start game, advance to tick 5000 with some commands.
- Serialize sim: `json = sim.as_ref().unwrap().to_json()`.
- Create new session, `LoadSim { json }`.
- Advance both by 1000 more ticks (same commands).
- Assert identical checksums.

### 15.8 Multiplayer translation layer

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

**ServerMessage::SpeedChanged -> SetSpeed:**
- Construct `ServerMessage::SpeedChanged { ticks_per_turn: 100 }`.
- Translate via `ticks_per_turn_to_speed(100)`.
- Assert produces `SessionMessage::SetSpeed { speed: SessionSpeed::Fast }`.

**ServerMessage::DesyncDetected -> DesyncDetected:**
- Construct `ServerMessage::DesyncDetected { tick: 5000 }`.
- Translate to `SessionMessage::DesyncDetected { tick: 5000 }`.
- Process through session. Assert `SessionEvent::DesyncDetected { tick: 5000 }`.

### 15.9 LocalRelay accumulator math

**Basic tick advancement:**
- Create a `LocalRelay` with `seconds_per_tick = 0.001`.
- Call `update(delta: 0.016, speed_mult: 1.0, current_tick: 0)`.
- Assert returns `AdvanceTo { tick: 16 }` (16ms at 1000 ticks/sec).

**Speed multiplier:**
- Call `update(delta: 0.016, speed_mult: 2.0, current_tick: 0)`.
- Assert returns `AdvanceTo { tick: 32 }` (2x speed).

**Accumulator carryover:**
- Call `update(delta: 0.0005, speed_mult: 1.0, current_tick: 0)`.
- Assert returns `None` (0.5ms = 0 whole ticks).
- Call again with same delta. Assert returns `AdvanceTo { tick: 1 }`.

**Spiral-of-death cap:**
- Call `update(delta: 1.0, speed_mult: 1.0, current_tick: 0)`.
- Assert tick advancement is capped at 100 (accumulator capped at
  0.1s = 100 ticks at 1x speed).

**Render tick fraction:**
- After `update()`, call `render_tick(current_tick)`.
- Assert it equals `current_tick + accumulator_remainder`.

**Paused (speed_mult = 0.0):**
- Call `update(delta: 0.016, speed_mult: 0.0, current_tick: 100)`.
- Assert returns `None`. Assert `render_tick(100)` == 100.0.

### 15.10 New SimAction variants (sim-level unit tests)

These tests call `sim.step()` directly (not through GameSession),
verifying the new `SimAction` variants work at the sim level:

**SetCreatureFood:**
- Create a sim, spawn a creature. Get its `CreatureId`.
- Call `sim.step()` with `SimAction::SetCreatureFood { creature_id, food: 42 }`.
- Assert `creature.food == 42`.

**SetCreatureRest:**
- Same pattern. Assert `creature.rest` is updated.

**AddCreatureItem:**
- Create a sim, spawn a creature.
- Call `sim.step()` with `SimAction::AddCreatureItem { creature_id, item_kind: Bread, quantity: 5 }`.
- Assert creature inventory contains 5 Bread.

**AddGroundPileItem:**
- Create a sim.
- Call `sim.step()` with `SimAction::AddGroundPileItem { position, item_kind: Bread, quantity: 10 }`.
- Assert `sim.ground_piles` contains a pile at position with 10 Bread.

### 15.11 SessionEvent PartialEq

Verify that `SessionEvent` derives `PartialEq` by using `==` in
test assertions:

```rust
assert_eq!(
    SessionEvent::GameStarted,
    SessionEvent::GameStarted,
);
assert_ne!(
    SessionEvent::SpeedChanged { speed: SessionSpeed::Normal },
    SessionEvent::SpeedChanged { speed: SessionSpeed::Fast },
);
```

Note: `SessionMessage` does NOT derive `PartialEq` (see section 5).
Test assertions on messages use pattern matching:

```rust
assert!(matches!(
    msg,
    SessionMessage::StartGame { seed: 42, .. }
));
```

### 15.12 Edge cases

- **SimCommand with no sim loaded:** Silently dropped.
- **StartGame when sim already loaded:** Emits `SimUnloaded` then
  replaces the sim (see section 15.1).
- **LoadSim with invalid JSON:** Error event, sim unchanged.
- **Rapid pause/resume (10x):** Assert consistent fields.
- **10,000 ticks with no commands:** Sim processes heartbeats, tree
  growth, etc. normally.
- **DesyncDetected:** Assert event is emitted, no state changes.

---

## 16. File-by-File Change Plan

### New files

| File | Purpose |
|------|---------|
| `elven_canopy_sim/src/session.rs` | `GameSession`, `SessionMessage`, `SessionEvent`, `SessionSpeed`, `SessionPlayerId`, `PlayerSlot`, `PendingAction`. Core struct + `process()` + tests. |
| `elven_canopy_gdext/src/local_relay.rs` | `LocalRelay` struct (accumulator-based tick pacer for single-player). |

### Modified: Rust

**`elven_canopy_sim/src/lib.rs`**
- Add `pub mod session;`
- Re-export `GameSession`, `SessionMessage`, `SessionSpeed`,
  `SessionEvent`, `SessionPlayerId`.

**`elven_canopy_sim/src/sim.rs`**
- Remove `speed: SimSpeed` field from `SimState`.
- Remove the `SetSimSpeed` match arm from `apply_command()`.
- Remove `SpeedChanged` event emission from `apply_command()`.
- Change `spawn_creature()` to return `Option<CreatureId>`.
- Add `spawn_initial_creatures(&mut self, events: &mut Vec<SimEvent>)`.
- Add `find_surface_position(&self, x: i32, z: i32) -> VoxelCoord`.
- Add `apply_command` arms for `SetCreatureFood`, `SetCreatureRest`,
  `AddCreatureItem`, `AddGroundPileItem`.
- Keep `step()` unchanged -- it's the right primitive for the session
  to call.

**`elven_canopy_sim/src/command.rs`**
- Remove `SimAction::SetSimSpeed` variant.
- Add `SimAction::SetCreatureFood`, `SetCreatureRest`,
  `AddCreatureItem`, `AddGroundPileItem` variants.
- Update docstring.

**`elven_canopy_sim/src/event.rs`**
- Remove `SimEventKind::SpeedChanged` variant.
- Add `PartialEq` derive to `SimEvent` and `SimEventKind`.
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
- Add `local_relay: Option<LocalRelay>` (imported from `local_relay.rs`).
- Add `local_player_id: SessionPlayerId`.
- Remove `is_multiplayer_mode`, `is_host`, `game_started`,
  `mp_ticks_per_turn` fields.
- Add `#[func] fn is_host(&self) -> bool` (delegates to
  `session.host_id == self.local_player_id`).
- Add `speed_to_ticks_per_turn()` and `ticks_per_turn_to_speed()`.
- Rewrite `apply_or_send()` -> `session.process(SimCommand { ... })`
  (single-player) or `net_client.send_command()` (multiplayer).
- Rewrite `step_to_tick()` -> `session.process(AdvanceTo { tick })`.
- Rewrite `set_sim_speed()` -> `session.process(SetSpeed { ... })` +
  relay speed/pause translation.
- Remove `set_creature_food()`, `set_creature_rest()`,
  `add_creature_item()`, `add_ground_pile_item()` (or convert to
  session message wrappers if needed for debug/testing).
- Add `frame_update(delta) -> f64` method that handles tick pacing
  and returns render_tick.
- Rewrite `poll_network()` to translate wire messages ->
  SessionMessages. Checksum logic stays here.
- Translate `ServerMessage::DesyncDetected` to
  `SessionMessage::DesyncDetected`.
- Translate `ServerMessage::SpeedChanged` to
  `SessionMessage::SetSpeed` via `ticks_per_turn_to_speed()`.
- All sim query methods delegate through `session.sim`.
- Change `designate_build_rect()` to return a simple success indicator
  instead of reading `last_build_message` synchronously.
- Update module docstring.

**`elven_canopy_gdext/src/lib.rs`**
- Add `mod local_relay;`.

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
- Update project structure to mention `session.rs` and `local_relay.rs`.
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

## 17. Migration Phases

### Phase 1: GameSession struct + tests

Create `session.rs` with `GameSession`, `SessionMessage`,
`SessionEvent`, `SessionSpeed`, `SessionPlayerId`, `PendingAction`,
`PlayerSlot`. Implement `process()` with the backward tick guard and
`DesyncDetected` handling.

Write all tests from sections 15.1-15.6, 15.11-15.12. Derive
`PartialEq` on `SessionEvent` (but not `SessionMessage` -- see
section 5). Derive `PartialEq` on `SimEvent` and `SimEventKind` to
support `SessionEvent::Sim(...)` comparison in tests.

`SimState.speed` and `SimAction::SetSimSpeed` still exist but are
unused by the session. The session manages speed independently.

This is a pure addition -- no existing code changes. Everything compiles
and tests pass alongside the old code.

### Phase 2: Initial creature spawning + direct mutation commands

Add `initial_creatures` and `initial_ground_piles` to `GameConfig`.
Add `InitialCreatureSpec` and `InitialGroundPileSpec`.
Change `spawn_creature()` to return `Option<CreatureId>`.
Add `find_surface_position()`.
Implement `spawn_initial_creatures()` on `SimState`.
Add `SimAction::SetCreatureFood`, `SetCreatureRest`, `AddCreatureItem`,
`AddGroundPileItem` variants and their `apply_command()` handlers.

Write sim-level unit tests for the new `SimAction` variants (section
15.10) -- these test `sim.step()` directly, not through GameSession.
Update session tests to verify initial creatures (section 15.6 spawn
determinism tests).

Still no integration with SimBridge -- old code paths work.

### Phase 3: Wire GameSession into SimBridge + LocalRelay

Replace `SimBridge.sim` with `SimBridge.session`. Create
`local_relay.rs` with the `LocalRelay` struct. Add `local_relay` and
`local_player_id` fields.

Rewrite the key methods (`apply_or_send`, `step_to_tick`,
`set_sim_speed`, `poll_network`). All sim queries go through
`session.sim`. Add `frame_update()`. Add `is_host()` method.

Add `speed_to_ticks_per_turn()` and `ticks_per_turn_to_speed()`.
Add `DesyncDetected` translation in `poll_network()`.
Add `SpeedChanged` translation in `poll_network()`.

Remove `set_creature_food()`, `set_creature_rest()`,
`add_creature_item()`, `add_ground_pile_item()` from SimBridge.
Change `designate_build_rect()` to not read `last_build_message`.

Add LocalRelay tests (section 15.9).
Add multiplayer translation tests (section 15.8).

This is the big integration step. Both SP and MP paths now go through
the session.

### Phase 4: Simplify GDScript

Unify `_process()` in main.gd. Remove accumulator fields. Remove
initial creature spawning from GDScript. Remove direct-mutation calls.

### Phase 5: Remove SimSpeed from sim

Remove `speed: SimSpeed` from `SimState`. Remove
`SimAction::SetSimSpeed`. Remove `SimEventKind::SpeedChanged`. Remove
the `SimSpeed` type from `types.rs`. Update all references. Clean up
dead code in SimBridge.

**Existing code that references `SimSpeed`/`SetSimSpeed` and will need
updating:**

In `elven_canopy_sim/src/`:
- `types.rs`: `SimSpeed` enum definition, `SimSpeed::multiplier()`
  method, `sim_speed_multiplier` test.
- `command.rs`: `SimAction::SetSimSpeed` variant, docstring reference.
- `event.rs`: `SimEventKind::SpeedChanged` variant.
- `sim.rs`: `speed: SimSpeed` field in `SimState`, `SimSpeed::Normal`
  in `with_config()`, `SetSimSpeed` match arm in `apply_command()`,
  `set_sim_speed` test, `speed_change_emits_event` test.

In `elven_canopy_gdext/src/`:
- `sim_bridge.rs`: `SimSpeed` import, `get_sim_speed()`,
  `sim_speed_multiplier()`, `set_sim_speed()` (all match on
  `SimSpeed` variants), `apply_or_send(SimAction::SetSimSpeed { ... })`.

### Phase 6: Documentation

Update CLAUDE.md, design_doc.md, tracker.md.

---

## 18. Relationship to Existing Multiplayer Design

This draft **refines and simplifies** the relay design in
`docs/drafts/multiplayer_relay.md`.

The key clarification: the relay manages turn pacing (it decides when to
emit turns and what tick target they carry) but does NOT understand pause
semantics. Pause and resume are `SessionMessage`s that the relay
delivers through its normal message stream. Paused clients ignore
`AdvanceTo` messages; the relay doesn't need to know or care. The
relay's `Paused`/`Resumed` wire messages exist so all clients learn
about the pause, but the relay itself keeps emitting turns on its
cadence.

Key relationships:
- The relay's `ServerMessage::Turn` maps to a batch of
  `SessionMessage::SimCommand` + `SessionMessage::AdvanceTo`.
- The relay's `Paused`/`Resumed` messages map to
  `SessionMessage::Pause`/`Resume`. These are delivered through the
  turn stream and processed by the session's `process()` method. The
  relay does not enforce them -- clients do.
- The relay's `GameStart` maps to `SessionMessage::StartGame` (with
  seed and config as parameters).
- The relay's `SpeedChanged { ticks_per_turn }` maps to
  `SessionMessage::SetSpeed` after `ticks_per_turn_to_speed()`
  translation in `SimBridge`.
- Mid-game join (`SnapshotLoad`) maps to `SessionMessage::LoadSim`.
- Desync detection (`DesyncDetected`) maps to
  `SessionMessage::DesyncDetected`, which the session processes and
  emits as `SessionEvent::DesyncDetected { tick }`.
- Chat (`ChatBroadcast`) is handled by `SimBridge` outside the session
  -- it's a UI/social concern, not game state. The session doesn't
  need a Chat variant.

The wire protocol (`ClientMessage`/`ServerMessage`) may evolve to
more closely match `SessionMessage` as the relay simplifies, but the
translation layer in `SimBridge` handles any mismatch.
