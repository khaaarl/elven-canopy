// Session management — the message-driven game session struct.
//
// `GameSession` encapsulates all session-level state: connected players, the
// optional simulation, pause/speed settings, and a pending command buffer. All
// mutation goes through `process(msg)`, which takes a `SessionMessage` and
// returns `Vec<SessionEvent>`.
//
// This design separates session-level concerns (who's connected, what speed,
// is it paused) from sim-level concerns (world, creatures, events). The sim
// is an `Option<SimState>` — a session can exist without a running game
// (pre-start, between games, after unload).
//
// Key design decisions:
// - **Message-driven mutation.** No direct field writes from outside. Want to
//   change speed? Send `SetSpeed`. Want to start a game? Send `StartGame`.
// - **Command buffering.** Player commands are buffered in `pending_commands`
//   and flushed on the next `AdvanceTo`, receiving their tick from the
//   AdvanceTo target. This matches multiplayer semantics where the relay
//   batches commands into turns.
// - **Pause is a boolean, not a state.** A paused session rejects `AdvanceTo`
//   but is otherwise identical. No duplicated fields or variants needed.
// - **Deterministic.** Two sessions processing identical message streams from
//   the same initial state produce identical results.
//
// See also: `sim.rs` for the simulation state machine, `command.rs` for
// `SimAction`/`SimCommand` types, `event.rs` for `SimEvent`.
//
// The design follows `docs/drafts/session_state_machine_v4.md`.
//
// **Critical constraint: determinism.** `pending_commands` is a `Vec` —
// insertion order matters. All collections use `BTreeMap` for deterministic
// iteration.

use crate::command::{SimAction, SimCommand};
use crate::config::GameConfig;
use crate::event::SimEvent;
use crate::sim::SimState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Session-level player identifier. In single-player, the local player gets
/// ID 0. In multiplayer, IDs are assigned by the relay (matching
/// RelayPlayerId values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionPlayerId(pub u32);

impl SessionPlayerId {
    pub const LOCAL: SessionPlayerId = SessionPlayerId(0);
}

impl fmt::Display for SessionPlayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Player({})", self.0)
    }
}

/// Session speed. No "Paused" variant — pausing is a separate boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionSpeed {
    Normal,
    Fast,
    VeryFast,
}

/// A buffered action waiting for flush. Carries the session player ID for
/// attribution and the sim action to apply.
pub struct PendingAction {
    pub from: SessionPlayerId,
    pub action: SimAction,
}

/// A connected player's session-level information.
pub struct PlayerSlot {
    pub id: SessionPlayerId,
    pub name: String,
    pub is_local: bool,
}

/// A typed message that drives the session. In single-player, produced locally.
/// In multiplayer, ordered and broadcast by the relay.
///
/// Note: `PartialEq` is NOT derived. `SimAction` deliberately does not derive
/// `PartialEq` (it contains `Vec` fields where element-wise comparison is
/// unnecessary overhead), and `GameConfig` contains `f32`/`f64` fields where
/// `PartialEq` would be misleading.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionMessage {
    /// A player has connected to the session.
    PlayerJoined { id: SessionPlayerId, name: String },
    /// A player has disconnected.
    PlayerLeft { id: SessionPlayerId },
    /// Start a new game with the given seed and config.
    StartGame { seed: u64, config: Box<GameConfig> },
    /// Load a sim from serialized state (save file or snapshot).
    LoadSim { json: String },
    /// Unload the current sim.
    UnloadSim,
    /// A simulation command from a player. Buffered until the next AdvanceTo.
    SimCommand {
        from: SessionPlayerId,
        action: SimAction,
    },
    /// Advance the simulation to the given tick, flushing all buffered commands.
    AdvanceTo { tick: u64 },
    /// Change the simulation speed.
    SetSpeed { speed: SessionSpeed },
    /// Pause the session (stop advancing time).
    Pause { by: SessionPlayerId },
    /// Resume the session.
    Resume { by: SessionPlayerId },
    /// Desync detected by the relay.
    DesyncDetected { tick: u64 },
}

/// Events produced by processing a `SessionMessage`. For UI display, logging,
/// and notification.
///
/// `PartialEq` is derived here (unlike `SessionMessage`) because
/// `SessionEvent`'s variants don't contain `SimAction` or `GameConfig`.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    PlayerJoined {
        name: String,
    },
    PlayerLeft {
        name: String,
    },
    GameStarted,
    SimLoaded,
    SimUnloaded,
    SpeedChanged {
        speed: SessionSpeed,
    },
    Paused {
        by: SessionPlayerId,
    },
    Resumed {
        by: SessionPlayerId,
    },
    /// A sim-level event (creature arrived, build completed, etc.).
    Sim(SimEvent),
    /// Desync detected at the given tick.
    DesyncDetected {
        tick: u64,
    },
    /// Something went wrong.
    Error {
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Speed ↔ ticks-per-turn helpers (design doc §9.1)
// ---------------------------------------------------------------------------

/// Base ticks per relay turn at Normal speed.
const BASE_TICKS_PER_TURN: u32 = 50;

/// Convert a `SessionSpeed` to the relay's `ticks_per_turn` value.
///
/// Normal → 50, Fast → 100, VeryFast → 250. Matches the multipliers
/// in `speed_multiplier()` (1x, 2x, 5x).
pub fn speed_to_ticks_per_turn(speed: SessionSpeed) -> u32 {
    match speed {
        SessionSpeed::Normal => BASE_TICKS_PER_TURN,
        SessionSpeed::Fast => BASE_TICKS_PER_TURN * 2,
        SessionSpeed::VeryFast => BASE_TICKS_PER_TURN * 5,
    }
}

/// Convert a relay `ticks_per_turn` value to a `SessionSpeed`.
///
/// Uses thresholds: ≤75 → Normal, ≤175 → Fast, else VeryFast.
/// This handles non-exact values from the relay gracefully.
pub fn ticks_per_turn_to_speed(tpt: u32) -> SessionSpeed {
    if tpt <= 75 {
        SessionSpeed::Normal
    } else if tpt <= 175 {
        SessionSpeed::Fast
    } else {
        SessionSpeed::VeryFast
    }
}

// ---------------------------------------------------------------------------
// GameSession
// ---------------------------------------------------------------------------

/// A game session: the shared context among players. Contains zero or one
/// simulators, player information, and session-level settings.
///
/// All mutation goes through `process()`. Networking, rendering, and I/O are
/// external concerns that feed messages in and read fields out.
pub struct GameSession {
    /// Connected players. Always non-empty (at least the local player in
    /// single-player). BTreeMap for deterministic iteration.
    pub players: BTreeMap<SessionPlayerId, PlayerSlot>,
    /// The host (whoever can start/load/unload games).
    host_id: SessionPlayerId,
    /// The simulation, if one is loaded.
    pub sim: Option<SimState>,
    /// Whether the session is paused.
    paused: bool,
    /// Who paused the session (for UI display).
    paused_by: Option<SessionPlayerId>,
    /// Current sim speed.
    speed: SessionSpeed,
    /// Commands received but not yet applied to the sim. Flushed on the next
    /// `AdvanceTo`. Insertion order matters for determinism.
    pub pending_commands: Vec<PendingAction>,
}

impl GameSession {
    /// Create a new single-player session with one local player.
    pub fn new_singleplayer() -> Self {
        let local = SessionPlayerId::LOCAL;
        let mut players = BTreeMap::new();
        players.insert(
            local,
            PlayerSlot {
                id: local,
                name: "Player".to_string(),
                is_local: true,
            },
        );
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

    /// Create a new multiplayer session. `host_id` is the session player who
    /// can start/load/unload games.
    pub fn new_multiplayer(local_id: SessionPlayerId, host_id: SessionPlayerId) -> Self {
        let mut players = BTreeMap::new();
        players.insert(
            local_id,
            PlayerSlot {
                id: local_id,
                name: String::new(),
                is_local: true,
            },
        );
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

    /// Process a message. Returns events for the UI/log.
    ///
    /// This is the single entry point for all mutation.
    pub fn process(&mut self, msg: SessionMessage) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        match msg {
            SessionMessage::PlayerJoined { id, name } => {
                self.players.insert(
                    id,
                    PlayerSlot {
                        id,
                        name: name.clone(),
                        is_local: false,
                    },
                );
                events.push(SessionEvent::PlayerJoined { name });
            }

            SessionMessage::PlayerLeft { id } => {
                if let Some(slot) = self.players.remove(&id) {
                    events.push(SessionEvent::PlayerLeft { name: slot.name });
                }
            }

            SessionMessage::StartGame { seed, config } => {
                if self.sim.is_some() {
                    self.sim = None;
                    events.push(SessionEvent::SimUnloaded);
                }
                let mut sim = SimState::with_config(seed, *config);
                let mut spawn_events = Vec::new();
                sim.spawn_initial_creatures(&mut spawn_events);
                self.sim = Some(sim);
                self.paused = false;
                self.pending_commands.clear();
                events.push(SessionEvent::GameStarted);
                for se in spawn_events {
                    events.push(SessionEvent::Sim(se));
                }
            }

            SessionMessage::LoadSim { json } => match SimState::from_json(&json) {
                Ok(sim) => {
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
            },

            SessionMessage::UnloadSim => {
                self.sim = None;
                self.paused = false;
                self.pending_commands.clear();
                events.push(SessionEvent::SimUnloaded);
            }

            SessionMessage::SimCommand { from, action } => {
                if self.sim.is_some() {
                    self.pending_commands.push(PendingAction { from, action });
                }
            }

            SessionMessage::AdvanceTo { tick } => {
                if self.paused {
                    return events;
                }
                if let Some(sim) = &mut self.sim {
                    if tick <= sim.tick {
                        return events;
                    }
                    let sim_player_id = sim.player_id;
                    let commands: Vec<SimCommand> = self
                        .pending_commands
                        .drain(..)
                        .map(|pa| SimCommand {
                            player_id: sim_player_id,
                            tick,
                            action: pa.action,
                        })
                        .collect();
                    let result = sim.step(&commands, tick);
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

    /// Speed multiplier for wall-clock to sim-tick conversion.
    pub fn speed_multiplier(&self) -> f64 {
        if self.paused {
            return 0.0;
        }
        match self.speed {
            SessionSpeed::Normal => 1.0,
            SessionSpeed::Fast => 2.0,
            SessionSpeed::VeryFast => 5.0,
        }
    }

    /// Whether the session is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Current sim speed setting.
    pub fn current_speed(&self) -> SessionSpeed {
        self.speed
    }

    /// Whether the given player is the session host.
    pub fn is_host(&self, player_id: SessionPlayerId) -> bool {
        self.host_id == player_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SimEventKind;
    use crate::types::{Species, VoxelCoord};
    use std::sync::LazyLock;

    /// Small test config matching sim.rs test_config() — 64x64x64 world,
    /// low-energy tree, flat terrain.
    fn session_test_config() -> GameConfig {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        config.tree_profile.growth.initial_energy = 50.0;
        config.terrain_max_height = 0;
        config.initial_creatures = vec![];
        config.initial_ground_piles = vec![];
        config
    }

    /// Cached sim JSON for tests that just need a sim present. Creating a
    /// SimState is expensive (tree generation), but loading from JSON is fast.
    static CACHED_SIM_JSON: LazyLock<String> = LazyLock::new(|| {
        let sim = SimState::with_config(42, session_test_config());
        sim.to_json()
            .expect("SimState serialization should not fail")
    });

    /// Create a singleplayer session with a loaded sim (via cached JSON).
    fn test_session_with_loaded_sim() -> GameSession {
        let mut session = GameSession::new_singleplayer();
        let events = session.process(SessionMessage::LoadSim {
            json: CACHED_SIM_JSON.clone(),
        });
        assert!(events.iter().any(|e| matches!(e, SessionEvent::SimLoaded)));
        session
    }

    /// Spawn position near world center at walking level for 64x64x64 world.
    const TEST_SPAWN_POS: VoxelCoord = VoxelCoord::new(32, 1, 32);

    // -----------------------------------------------------------------------
    // 15.1 Message processing basics
    // -----------------------------------------------------------------------

    #[test]
    fn player_joined() {
        let mut session = GameSession::new_singleplayer();
        let id = SessionPlayerId(5);
        let events = session.process(SessionMessage::PlayerJoined {
            id,
            name: "Alice".to_string(),
        });
        assert!(session.players.contains_key(&id));
        assert_eq!(session.players[&id].name, "Alice");
        assert!(!session.players[&id].is_local);
        assert_eq!(
            events,
            vec![SessionEvent::PlayerJoined {
                name: "Alice".to_string()
            }]
        );
    }

    #[test]
    fn player_left() {
        let mut session = GameSession::new_singleplayer();
        let id = SessionPlayerId(5);
        session.process(SessionMessage::PlayerJoined {
            id,
            name: "Alice".to_string(),
        });
        let events = session.process(SessionMessage::PlayerLeft { id });
        assert!(!session.players.contains_key(&id));
        assert_eq!(
            events,
            vec![SessionEvent::PlayerLeft {
                name: "Alice".to_string()
            }]
        );
    }

    #[test]
    fn player_left_nonexistent() {
        let mut session = GameSession::new_singleplayer();
        let events = session.process(SessionMessage::PlayerLeft {
            id: SessionPlayerId(99),
        });
        assert!(events.is_empty());
    }

    #[test]
    fn start_game_creates_sim() {
        let mut session = GameSession::new_singleplayer();
        let events = session.process(SessionMessage::StartGame {
            seed: 42,
            config: Box::new(session_test_config()),
        });
        assert!(session.has_sim());
        assert_eq!(session.current_tick(), 0);
        assert_eq!(events, vec![SessionEvent::GameStarted]);
    }

    #[test]
    fn start_game_replaces_existing() {
        let mut session = test_session_with_loaded_sim();
        session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert_eq!(session.current_tick(), 100);

        let events = session.process(SessionMessage::StartGame {
            seed: 99,
            config: Box::new(session_test_config()),
        });
        // SimUnloaded should come before GameStarted.
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], SessionEvent::SimUnloaded);
        assert_eq!(events[1], SessionEvent::GameStarted);
        assert_eq!(session.current_tick(), 0);
    }

    #[test]
    fn load_unload_roundtrip() {
        let mut session = test_session_with_loaded_sim();
        session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert_eq!(session.current_tick(), 100);

        let json = session.sim.as_ref().unwrap().to_json().unwrap();

        let events = session.process(SessionMessage::UnloadSim);
        assert!(!session.has_sim());
        assert_eq!(events, vec![SessionEvent::SimUnloaded]);

        let events = session.process(SessionMessage::LoadSim { json });
        assert!(session.has_sim());
        assert_eq!(session.current_tick(), 100);
        assert_eq!(events, vec![SessionEvent::SimLoaded]);
    }

    #[test]
    fn unload_clears_pending() {
        let mut session = test_session_with_loaded_sim();
        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });
        assert_eq!(session.pending_commands.len(), 1);

        session.process(SessionMessage::UnloadSim);
        assert!(session.pending_commands.is_empty());
    }

    // -----------------------------------------------------------------------
    // 15.2 Command buffering and tick advancement
    // -----------------------------------------------------------------------

    #[test]
    fn commands_dont_apply_until_advance_to() {
        let mut session = test_session_with_loaded_sim();
        let initial_count = session.sim.as_ref().unwrap().creatures.len();

        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });
        // Command is buffered, not applied.
        assert_eq!(session.sim.as_ref().unwrap().creatures.len(), initial_count);

        session.process(SessionMessage::AdvanceTo { tick: 1 });
        assert_eq!(
            session.sim.as_ref().unwrap().creatures.len(),
            initial_count + 1
        );
    }

    #[test]
    fn multiple_commands_flush() {
        let mut session = test_session_with_loaded_sim();
        let initial_count = session.sim.as_ref().unwrap().creatures.len();

        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });
        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: TEST_SPAWN_POS,
            },
        });

        session.process(SessionMessage::AdvanceTo { tick: 1 });
        assert_eq!(
            session.sim.as_ref().unwrap().creatures.len(),
            initial_count + 2
        );
    }

    #[test]
    fn commands_get_tick_from_advance_to() {
        let mut session = test_session_with_loaded_sim();

        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });

        let events = session.process(SessionMessage::AdvanceTo { tick: 500 });

        // The spawn event should have tick 500 (assigned from AdvanceTo target).
        let spawn_events: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::Sim(se) => Some(se),
                _ => None,
            })
            .filter(|se| matches!(se.kind, SimEventKind::CreatureArrived { .. }))
            .collect();
        assert_eq!(spawn_events.len(), 1);
        assert_eq!(spawn_events[0].tick, 500);
    }

    #[test]
    fn advance_to_no_sim_noop() {
        let mut session = GameSession::new_singleplayer();
        let events = session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert!(events.is_empty());
    }

    #[test]
    fn concurrent_commands_different_players() {
        let p1 = SessionPlayerId(1);
        let p2 = SessionPlayerId(2);
        let mut session = GameSession::new_multiplayer(p1, p1);

        session.process(SessionMessage::PlayerJoined {
            id: p2,
            name: "Player2".to_string(),
        });
        session.process(SessionMessage::LoadSim {
            json: CACHED_SIM_JSON.clone(),
        });

        let initial_count = session.sim.as_ref().unwrap().creatures.len();

        session.process(SessionMessage::SimCommand {
            from: p1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });
        session.process(SessionMessage::SimCommand {
            from: p2,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: TEST_SPAWN_POS,
            },
        });

        session.process(SessionMessage::AdvanceTo { tick: 1 });
        assert_eq!(
            session.sim.as_ref().unwrap().creatures.len(),
            initial_count + 2
        );
    }

    // -----------------------------------------------------------------------
    // 15.3 Pause / resume
    // -----------------------------------------------------------------------

    #[test]
    fn pause_blocks_advance_to() {
        let mut session = test_session_with_loaded_sim();

        session.process(SessionMessage::Pause {
            by: SessionPlayerId::LOCAL,
        });
        assert!(session.is_paused());

        session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert_eq!(session.current_tick(), 0);

        session.process(SessionMessage::Resume {
            by: SessionPlayerId::LOCAL,
        });
        session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert_eq!(session.current_tick(), 100);
    }

    #[test]
    fn commands_buffer_while_paused() {
        let mut session = test_session_with_loaded_sim();
        let initial_count = session.sim.as_ref().unwrap().creatures.len();

        session.process(SessionMessage::Pause {
            by: SessionPlayerId::LOCAL,
        });
        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });
        assert_eq!(session.pending_commands.len(), 1);

        session.process(SessionMessage::Resume {
            by: SessionPlayerId::LOCAL,
        });
        session.process(SessionMessage::AdvanceTo { tick: 1 });
        assert_eq!(
            session.sim.as_ref().unwrap().creatures.len(),
            initial_count + 1
        );
    }

    #[test]
    fn pause_command_reject_resume_accept() {
        let mut session = test_session_with_loaded_sim();
        let initial_count = session.sim.as_ref().unwrap().creatures.len();

        session.process(SessionMessage::Pause {
            by: SessionPlayerId::LOCAL,
        });
        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });

        // AdvanceTo while paused — rejected.
        session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert_eq!(session.current_tick(), 0);
        assert_eq!(session.sim.as_ref().unwrap().creatures.len(), initial_count);

        // Resume, then AdvanceTo — accepted.
        session.process(SessionMessage::Resume {
            by: SessionPlayerId::LOCAL,
        });
        session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert_eq!(session.current_tick(), 100);
        assert_eq!(
            session.sim.as_ref().unwrap().creatures.len(),
            initial_count + 1
        );
    }

    #[test]
    fn double_pause_noop() {
        let mut session = GameSession::new_singleplayer();
        let events1 = session.process(SessionMessage::Pause {
            by: SessionPlayerId::LOCAL,
        });
        let events2 = session.process(SessionMessage::Pause {
            by: SessionPlayerId::LOCAL,
        });
        assert!(session.is_paused());
        assert_eq!(events1.len(), 1);
        assert!(events2.is_empty());
    }

    #[test]
    fn resume_not_paused_noop() {
        let mut session = GameSession::new_singleplayer();
        let events = session.process(SessionMessage::Resume {
            by: SessionPlayerId::LOCAL,
        });
        assert!(events.is_empty());
        assert!(!session.is_paused());
    }

    // -----------------------------------------------------------------------
    // 15.4 AdvanceTo backward tick guard
    // -----------------------------------------------------------------------

    #[test]
    fn same_tick_rejected() {
        let mut session = test_session_with_loaded_sim();

        // Buffer a command to verify it survives rejection.
        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });

        // Sim starts at tick 0. AdvanceTo { tick: 0 } is tick <= sim.tick.
        let events = session.process(SessionMessage::AdvanceTo { tick: 0 });
        assert!(events.is_empty());
        assert_eq!(session.current_tick(), 0);
        assert_eq!(session.pending_commands.len(), 1);
    }

    #[test]
    fn backward_tick_rejected() {
        let mut session = test_session_with_loaded_sim();
        session.process(SessionMessage::AdvanceTo { tick: 100 });
        assert_eq!(session.current_tick(), 100);

        // Buffer a command to verify it survives rejection.
        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });

        let events = session.process(SessionMessage::AdvanceTo { tick: 50 });
        assert!(events.is_empty());
        assert_eq!(session.current_tick(), 100);
        assert_eq!(session.pending_commands.len(), 1);
    }

    #[test]
    fn guard_preserves_pending() {
        let mut session = test_session_with_loaded_sim();
        let initial_count = session.sim.as_ref().unwrap().creatures.len();

        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });

        // Rejected — tick 0 <= sim.tick 0.
        session.process(SessionMessage::AdvanceTo { tick: 0 });
        assert_eq!(session.pending_commands.len(), 1);

        // Accepted.
        session.process(SessionMessage::AdvanceTo { tick: 1 });
        assert_eq!(
            session.sim.as_ref().unwrap().creatures.len(),
            initial_count + 1
        );
        assert!(session.pending_commands.is_empty());
    }

    // -----------------------------------------------------------------------
    // 15.5 Speed
    // -----------------------------------------------------------------------

    #[test]
    fn set_speed_changes_multiplier() {
        let mut session = GameSession::new_singleplayer();

        session.process(SessionMessage::SetSpeed {
            speed: SessionSpeed::Normal,
        });
        assert_eq!(session.speed_multiplier(), 1.0);

        session.process(SessionMessage::SetSpeed {
            speed: SessionSpeed::Fast,
        });
        assert_eq!(session.speed_multiplier(), 2.0);

        session.process(SessionMessage::SetSpeed {
            speed: SessionSpeed::VeryFast,
        });
        assert_eq!(session.speed_multiplier(), 5.0);

        // While paused, speed_multiplier is 0.0 regardless.
        session.process(SessionMessage::Pause {
            by: SessionPlayerId::LOCAL,
        });
        assert_eq!(session.speed_multiplier(), 0.0);
    }

    #[test]
    fn speed_doesnt_affect_sim() {
        let mut session_a = test_session_with_loaded_sim();
        let mut session_b = test_session_with_loaded_sim();

        // Session A: advance straight to tick 1000.
        session_a.process(SessionMessage::AdvanceTo { tick: 1000 });

        // Session B: advance with speed changes interspersed.
        session_b.process(SessionMessage::SetSpeed {
            speed: SessionSpeed::Fast,
        });
        session_b.process(SessionMessage::AdvanceTo { tick: 500 });
        session_b.process(SessionMessage::SetSpeed {
            speed: SessionSpeed::VeryFast,
        });
        session_b.process(SessionMessage::AdvanceTo { tick: 1000 });

        assert_eq!(
            session_a.sim.as_ref().unwrap().state_checksum(),
            session_b.sim.as_ref().unwrap().state_checksum(),
        );
    }

    // -----------------------------------------------------------------------
    // 15.6 Determinism
    // -----------------------------------------------------------------------

    #[test]
    fn same_messages_same_checksums() {
        let mut session_a = test_session_with_loaded_sim();
        let mut session_b = test_session_with_loaded_sim();

        let messages: Vec<SessionMessage> = vec![
            SessionMessage::SimCommand {
                from: SessionPlayerId::LOCAL,
                action: SimAction::SpawnCreature {
                    species: Species::Elf,
                    position: TEST_SPAWN_POS,
                },
            },
            SessionMessage::AdvanceTo { tick: 100 },
            SessionMessage::SimCommand {
                from: SessionPlayerId::LOCAL,
                action: SimAction::SpawnCreature {
                    species: Species::Capybara,
                    position: TEST_SPAWN_POS,
                },
            },
            SessionMessage::AdvanceTo { tick: 500 },
            SessionMessage::AdvanceTo { tick: 1000 },
        ];

        for msg in &messages {
            session_a.process(msg.clone());
            session_b.process(msg.clone());
        }

        assert_eq!(
            session_a.sim.as_ref().unwrap().state_checksum(),
            session_b.sim.as_ref().unwrap().state_checksum(),
        );
    }

    #[test]
    fn spawn_initial_creatures_determinism() {
        use crate::config::{InitialCreatureSpec, InitialGroundPileSpec};

        let mut config = session_test_config();
        config.initial_creatures = vec![
            InitialCreatureSpec {
                species: Species::Elf,
                count: 2,
                spawn_position: VoxelCoord::new(32, 1, 32),
                food_pcts: vec![100, 60],
                rest_pcts: vec![90, 50],
                bread_counts: vec![0, 2],
            },
            InitialCreatureSpec {
                species: Species::Capybara,
                count: 1,
                spawn_position: VoxelCoord::new(32, 1, 32),
                food_pcts: vec![],
                rest_pcts: vec![],
                bread_counts: vec![],
            },
        ];
        config.initial_ground_piles = vec![InitialGroundPileSpec {
            position: VoxelCoord::new(32, 1, 34),
            item_kind: crate::inventory::ItemKind::Bread,
            quantity: 5,
        }];

        let mut session_a = GameSession::new_singleplayer();
        let mut session_b = GameSession::new_singleplayer();

        session_a.process(SessionMessage::StartGame {
            seed: 42,
            config: Box::new(config.clone()),
        });
        session_b.process(SessionMessage::StartGame {
            seed: 42,
            config: Box::new(config),
        });

        let sim_a = session_a.sim.as_ref().unwrap();
        let sim_b = session_b.sim.as_ref().unwrap();

        // Same number of creatures.
        assert_eq!(sim_a.creatures.len(), sim_b.creatures.len());
        assert_eq!(sim_a.creatures.len(), 3);

        // Same creature IDs (deterministic PRNG).
        let ids_a: Vec<_> = sim_a.creatures.keys().collect();
        let ids_b: Vec<_> = sim_b.creatures.keys().collect();
        assert_eq!(ids_a, ids_b);

        // Same state checksums.
        assert_eq!(sim_a.state_checksum(), sim_b.state_checksum());
    }

    #[test]
    fn determinism_across_pause_resume() {
        let mut session_a = test_session_with_loaded_sim();
        let mut session_b = test_session_with_loaded_sim();

        // Session A: advance straight.
        session_a.process(SessionMessage::AdvanceTo { tick: 1000 });

        // Session B: advance with pause/resume/speed changes.
        session_b.process(SessionMessage::AdvanceTo { tick: 500 });
        session_b.process(SessionMessage::Pause {
            by: SessionPlayerId::LOCAL,
        });
        session_b.process(SessionMessage::Resume {
            by: SessionPlayerId::LOCAL,
        });
        session_b.process(SessionMessage::SetSpeed {
            speed: SessionSpeed::Fast,
        });
        session_b.process(SessionMessage::AdvanceTo { tick: 1000 });

        assert_eq!(
            session_a.sim.as_ref().unwrap().state_checksum(),
            session_b.sim.as_ref().unwrap().state_checksum(),
        );
    }

    // -----------------------------------------------------------------------
    // 15.7 Save/load round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn save_load_roundtrip_then_advance() {
        let mut session_a = test_session_with_loaded_sim();

        // Advance and add a creature.
        session_a.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });
        session_a.process(SessionMessage::AdvanceTo { tick: 100 });

        // Snapshot.
        let json = session_a.sim.as_ref().unwrap().to_json().unwrap();

        // Load snapshot into a new session.
        let mut session_b = GameSession::new_singleplayer();
        session_b.process(SessionMessage::LoadSim { json });

        // Advance both by the same amount.
        session_a.process(SessionMessage::AdvanceTo { tick: 600 });
        session_b.process(SessionMessage::AdvanceTo { tick: 600 });

        assert_eq!(
            session_a.sim.as_ref().unwrap().state_checksum(),
            session_b.sim.as_ref().unwrap().state_checksum(),
        );
    }

    // -----------------------------------------------------------------------
    // 15.11 SessionEvent PartialEq
    // -----------------------------------------------------------------------

    #[test]
    fn session_event_partial_eq() {
        assert_eq!(SessionEvent::GameStarted, SessionEvent::GameStarted);
        assert_ne!(
            SessionEvent::SpeedChanged {
                speed: SessionSpeed::Normal
            },
            SessionEvent::SpeedChanged {
                speed: SessionSpeed::Fast
            },
        );
        assert_eq!(
            SessionEvent::SpeedChanged {
                speed: SessionSpeed::Fast
            },
            SessionEvent::SpeedChanged {
                speed: SessionSpeed::Fast
            },
        );
    }

    // -----------------------------------------------------------------------
    // 15.12 Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn sim_command_no_sim_dropped() {
        let mut session = GameSession::new_singleplayer();
        assert!(!session.has_sim());

        session.process(SessionMessage::SimCommand {
            from: SessionPlayerId::LOCAL,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: TEST_SPAWN_POS,
            },
        });
        // Silently dropped — no pending commands since there's no sim.
        assert!(session.pending_commands.is_empty());
    }

    #[test]
    fn load_sim_invalid_json() {
        let mut session = GameSession::new_singleplayer();
        let events = session.process(SessionMessage::LoadSim {
            json: "not valid json".to_string(),
        });
        assert!(!session.has_sim());
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SessionEvent::Error { .. }));
    }

    #[test]
    fn desync_emits_event() {
        let mut session = GameSession::new_singleplayer();
        let events = session.process(SessionMessage::DesyncDetected { tick: 5000 });
        assert_eq!(events, vec![SessionEvent::DesyncDetected { tick: 5000 }]);
    }

    // -----------------------------------------------------------------------
    // 15.9 Speed ↔ ticks-per-turn helpers
    // -----------------------------------------------------------------------

    #[test]
    fn speed_tpt_roundtrip() {
        // Each speed maps to a specific tpt, and back.
        for speed in [
            SessionSpeed::Normal,
            SessionSpeed::Fast,
            SessionSpeed::VeryFast,
        ] {
            let tpt = speed_to_ticks_per_turn(speed);
            assert_eq!(ticks_per_turn_to_speed(tpt), speed);
        }
        // Verify specific values.
        assert_eq!(speed_to_ticks_per_turn(SessionSpeed::Normal), 50);
        assert_eq!(speed_to_ticks_per_turn(SessionSpeed::Fast), 100);
        assert_eq!(speed_to_ticks_per_turn(SessionSpeed::VeryFast), 250);
    }

    #[test]
    fn tpt_threshold_boundaries() {
        // At the boundary: 75 → Normal, 76 → Fast, 175 → Fast, 176 → VeryFast.
        assert_eq!(ticks_per_turn_to_speed(75), SessionSpeed::Normal);
        assert_eq!(ticks_per_turn_to_speed(76), SessionSpeed::Fast);
        assert_eq!(ticks_per_turn_to_speed(175), SessionSpeed::Fast);
        assert_eq!(ticks_per_turn_to_speed(176), SessionSpeed::VeryFast);
        // Extreme values.
        assert_eq!(ticks_per_turn_to_speed(1), SessionSpeed::Normal);
        assert_eq!(ticks_per_turn_to_speed(1000), SessionSpeed::VeryFast);
    }
}
