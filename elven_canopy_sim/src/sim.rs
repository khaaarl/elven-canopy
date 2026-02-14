// Core simulation state and tick loop.
//
// `SimState` is the single source of truth for the entire game world. It owns
// all entity data, the event queue, the PRNG, and the game config. The sim is
// a pure function: `(state, commands) -> (new_state, events)`.
//
// This file defines the top-level state struct and the main `step()` method
// that processes scheduled events. Specific subsystems (construction, elf AI,
// pathfinding) will live in their own modules and be called from here.
//
// See also: `event.rs` for the event queue, `command.rs` for `SimCommand`,
// `config.rs` for `GameConfig`, `types.rs` for entity IDs.
//
// **Critical constraint: determinism.** All state mutations flow through
// `SimCommand` or internal scheduled events. No external input (system time,
// thread state, etc.) may influence the simulation.

use crate::command::{SimAction, SimCommand};
use crate::config::GameConfig;
use crate::event::{EventQueue, ScheduledEventKind, SimEvent, SimEventKind};
use crate::prng::GameRng;
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level simulation state. This is the entire game world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimState {
    /// Current simulation tick.
    pub tick: u64,

    /// The simulation's deterministic PRNG.
    pub rng: GameRng,

    /// Game configuration (immutable after initialization).
    pub config: GameConfig,

    /// Current simulation speed.
    pub speed: SimSpeed,

    /// The event priority queue driving the discrete event simulation.
    pub event_queue: EventQueue,

    /// All tree entities, keyed by ID. BTreeMap for deterministic iteration.
    pub trees: BTreeMap<TreeId, Tree>,

    /// All elf entities, keyed by ID.
    pub elves: BTreeMap<ElfId, Elf>,

    /// The player's tree ID.
    pub player_tree_id: TreeId,

    /// The player's ID.
    pub player_id: PlayerId,
}

/// A tree entity — the primary world structure.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tree {
    pub id: TreeId,
    pub position: VoxelCoord,
    pub health: f32,
    pub growth_level: u32,
    pub mana_stored: f32,
    pub mana_capacity: f32,
    pub fruit_production_rate: f32,
    pub carrying_capacity: f32,
    pub current_load: f32,
    pub owner: Option<PlayerId>,
    pub trunk_voxels: Vec<VoxelCoord>,
    pub branch_voxels: Vec<VoxelCoord>,
}

/// An elf entity — an autonomous agent in the village.
///
/// Phase 0 defines the minimal structure; personality, mood, social graph,
/// and task state will be added in later phases.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Elf {
    pub id: ElfId,
    pub position: VoxelCoord,
}

/// The result of processing commands and advancing the simulation.
pub struct StepResult {
    /// Narrative events emitted during this step, for the UI / event log.
    pub events: Vec<SimEvent>,
}

impl SimState {
    /// Create a new simulation with default config and the given seed.
    pub fn new(seed: u64) -> Self {
        Self::with_config(seed, GameConfig::default())
    }

    /// Create a new simulation with the given seed and config.
    pub fn with_config(seed: u64, config: GameConfig) -> Self {
        let mut rng = GameRng::new(seed);
        let player_id = PlayerId::new(&mut rng);
        let player_tree_id = TreeId::new(&mut rng);

        let home_tree = Tree {
            id: player_tree_id,
            position: VoxelCoord::new(0, 0, 0),
            health: 100.0,
            growth_level: 1,
            mana_stored: config.starting_mana,
            mana_capacity: config.starting_mana_capacity,
            fruit_production_rate: config.fruit_production_base_rate,
            carrying_capacity: 20.0,
            current_load: 0.0,
            owner: Some(player_id),
            trunk_voxels: Vec::new(),
            branch_voxels: Vec::new(),
        };

        let mut trees = BTreeMap::new();
        trees.insert(player_tree_id, home_tree);

        let mut state = Self {
            tick: 0,
            rng,
            config,
            speed: SimSpeed::Normal,
            event_queue: EventQueue::new(),
            trees,
            elves: BTreeMap::new(),
            player_tree_id,
            player_id,
        };

        // Schedule the home tree's first heartbeat.
        let heartbeat_interval = state.config.heartbeat_interval_ticks;
        state.event_queue.schedule(
            heartbeat_interval,
            ScheduledEventKind::TreeHeartbeat {
                tree_id: player_tree_id,
            },
        );

        state
    }

    /// Apply a batch of commands and advance the sim to the target tick,
    /// processing all scheduled events up to that point.
    ///
    /// Commands must be sorted by tick. Commands with tick > `target_tick`
    /// are ignored (caller error).
    pub fn step(&mut self, commands: &[SimCommand], target_tick: u64) -> StepResult {
        let mut events = Vec::new();

        // Index into the sorted command slice.
        let mut cmd_idx = 0;

        while self.tick < target_tick {
            // Determine the next thing to process: the next scheduled event
            // or the next command, whichever comes first.
            let next_event_tick = self.event_queue.peek_tick();
            let next_cmd_tick = commands
                .get(cmd_idx)
                .filter(|c| c.tick <= target_tick)
                .map(|c| c.tick);

            let next_tick = match (next_event_tick, next_cmd_tick) {
                (Some(et), Some(ct)) => et.min(ct).min(target_tick),
                (Some(et), None) => et.min(target_tick),
                (None, Some(ct)) => ct.min(target_tick),
                (None, None) => target_tick,
            };

            self.tick = next_tick;

            // Apply commands at this tick.
            while cmd_idx < commands.len() && commands[cmd_idx].tick <= self.tick {
                let cmd = &commands[cmd_idx];
                cmd_idx += 1;
                self.apply_command(cmd, &mut events);
            }

            // Process scheduled events at this tick.
            while let Some(event) = self.event_queue.pop_if_ready(self.tick) {
                self.process_event(event.kind, &mut events);
            }
        }

        self.tick = target_tick;
        StepResult { events }
    }

    /// Apply a single command to the simulation.
    fn apply_command(&mut self, cmd: &SimCommand, events: &mut Vec<SimEvent>) {
        match &cmd.action {
            SimAction::SetSimSpeed { speed } => {
                self.speed = *speed;
                events.push(SimEvent {
                    tick: self.tick,
                    kind: SimEventKind::SpeedChanged { speed: *speed },
                });
            }
            // Other commands will be implemented as features are added.
            SimAction::DesignateBuild { .. } => {
                // TODO: Phase 2 — construction system.
            }
            SimAction::CancelBuild { .. } => {
                // TODO: Phase 2 — construction system.
            }
            SimAction::SetTaskPriority { .. } => {
                // TODO: Phase 2 — task system.
            }
        }
    }

    /// Process a single scheduled event.
    fn process_event(&mut self, kind: ScheduledEventKind, _events: &mut Vec<SimEvent>) {
        match kind {
            ScheduledEventKind::ElfHeartbeat { elf_id } => {
                // Only process if the elf still exists.
                if self.elves.contains_key(&elf_id) {
                    // TODO: Phase 3+ — need decay, mood, mana generation.
                    // Reschedule the next heartbeat.
                    let next_tick = self.tick + self.config.heartbeat_interval_ticks;
                    self.event_queue
                        .schedule(next_tick, ScheduledEventKind::ElfHeartbeat { elf_id });
                }
            }
            ScheduledEventKind::ElfMovementComplete { .. } => {
                // TODO: Phase 1 — pathfinding / movement.
            }
            ScheduledEventKind::TreeHeartbeat { tree_id } => {
                if self.trees.contains_key(&tree_id) {
                    // TODO: fruit production, mana updates.
                    // Reschedule.
                    let next_tick = self.tick + self.config.heartbeat_interval_ticks;
                    self.event_queue
                        .schedule(next_tick, ScheduledEventKind::TreeHeartbeat { tree_id });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sim_has_home_tree() {
        let sim = SimState::new(42);
        assert!(sim.trees.contains_key(&sim.player_tree_id));
        let tree = &sim.trees[&sim.player_tree_id];
        assert_eq!(tree.owner, Some(sim.player_id));
        assert_eq!(tree.mana_stored, sim.config.starting_mana);
    }

    #[test]
    fn determinism_two_sims_same_seed() {
        let sim_a = SimState::new(42);
        let sim_b = SimState::new(42);
        assert_eq!(sim_a.player_id, sim_b.player_id);
        assert_eq!(sim_a.player_tree_id, sim_b.player_tree_id);
        assert_eq!(sim_a.tick, sim_b.tick);
    }

    #[test]
    fn step_advances_tick() {
        let mut sim = SimState::new(42);
        sim.step(&[], 100);
        assert_eq!(sim.tick, 100);
    }

    #[test]
    fn step_processes_speed_command() {
        let mut sim = SimState::new(42);
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 10,
            action: SimAction::SetSimSpeed {
                speed: SimSpeed::Paused,
            },
        };
        let result = sim.step(&[cmd], 20);
        assert_eq!(sim.speed, SimSpeed::Paused);
        assert!(
            result
                .events
                .iter()
                .any(|e| matches!(e.kind, SimEventKind::SpeedChanged { speed: SimSpeed::Paused }))
        );
    }

    #[test]
    fn tree_heartbeat_reschedules() {
        let mut sim = SimState::new(42);
        let heartbeat_interval = sim.config.heartbeat_interval_ticks;

        // Step past the first heartbeat.
        sim.step(&[], heartbeat_interval + 1);

        // The tree heartbeat should have rescheduled. There should be a
        // pending event for tick = 2 * heartbeat_interval.
        assert_eq!(sim.event_queue.peek_tick(), Some(heartbeat_interval * 2));
    }

    #[test]
    fn serialization_roundtrip() {
        let mut sim = SimState::new(42);
        sim.step(&[], 50);
        let json = serde_json::to_string(&sim).unwrap();
        let restored: SimState = serde_json::from_str(&json).unwrap();
        assert_eq!(sim.tick, restored.tick);
        assert_eq!(sim.player_id, restored.player_id);
        assert_eq!(sim.player_tree_id, restored.player_tree_id);
    }

    #[test]
    fn determinism_after_stepping() {
        let mut sim_a = SimState::new(42);
        let mut sim_b = SimState::new(42);

        let cmds = vec![SimCommand {
            player_id: sim_a.player_id,
            tick: 50,
            action: SimAction::SetSimSpeed {
                speed: SimSpeed::Fast,
            },
        }];

        sim_a.step(&cmds, 200);
        sim_b.step(&cmds, 200);

        assert_eq!(sim_a.tick, sim_b.tick);
        assert_eq!(sim_a.speed, sim_b.speed);
        // Verify PRNG state is identical by drawing from both.
        assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
    }
}
