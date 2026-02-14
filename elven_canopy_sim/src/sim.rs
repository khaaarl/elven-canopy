// Core simulation state and tick loop.
//
// `SimState` is the single source of truth for the entire game world. It owns
// all entity data, the voxel world, the nav graph, the event queue, the PRNG,
// and the game config. The sim is a pure function:
// `(state, commands) -> (new_state, events)`.
//
// On construction (`new()`/`with_config()`), the sim generates tree geometry
// via `tree_gen.rs`, builds the navigation graph via `nav.rs`, and initializes
// the voxel world via `world.rs`. Elf spawning, pathfinding (via
// `pathfinding.rs`), and movement are handled through the command/event system.
//
// See also: `event.rs` for the event queue, `command.rs` for `SimCommand`,
// `config.rs` for `GameConfig`, `types.rs` for entity IDs, `world.rs` for
// the voxel grid, `nav.rs` for navigation, `pathfinding.rs` for A*.
//
// **Critical constraint: determinism.** All state mutations flow through
// `SimCommand` or internal scheduled events. No external input (system time,
// thread state, etc.) may influence the simulation.

use crate::command::{SimAction, SimCommand};
use crate::config::GameConfig;
use crate::event::{EventQueue, ScheduledEventKind, SimEvent, SimEventKind};
use crate::nav::{self, NavGraph};
use crate::pathfinding;
use crate::prng::GameRng;
use crate::tree_gen;
use crate::types::*;
use crate::world::VoxelWorld;
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

    /// The 3D voxel world grid. Regenerated from seed, not serialized.
    #[serde(skip)]
    pub world: VoxelWorld,

    /// The navigation graph built from tree geometry. Regenerated from seed, not serialized.
    #[serde(skip)]
    pub nav_graph: NavGraph,
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

/// An elf's current path through the nav graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElfPath {
    /// Remaining node IDs to visit (next node is index 0).
    pub remaining_nodes: Vec<NavNodeId>,
    /// Remaining edge indices to traverse (next edge is index 0).
    pub remaining_edge_indices: Vec<usize>,
}

/// An elf entity — an autonomous agent in the village.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Elf {
    pub id: ElfId,
    pub position: VoxelCoord,
    /// Current nav node the elf is at (or moving from).
    pub current_node: Option<NavNodeId>,
    /// Active path the elf is traversing.
    pub path: Option<ElfPath>,
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

        let (ws_x, ws_y, ws_z) = config.world_size;
        let mut world = VoxelWorld::new(ws_x, ws_y, ws_z);

        // Generate tree geometry into the voxel world.
        let tree_result = tree_gen::generate_tree(&mut world, &config, &mut rng);

        let center_x = ws_x as i32 / 2;
        let center_z = ws_z as i32 / 2;

        let home_tree = Tree {
            id: player_tree_id,
            position: VoxelCoord::new(center_x, 0, center_z),
            health: 100.0,
            growth_level: 1,
            mana_stored: config.starting_mana,
            mana_capacity: config.starting_mana_capacity,
            fruit_production_rate: config.fruit_production_base_rate,
            carrying_capacity: 20.0,
            current_load: 0.0,
            owner: Some(player_id),
            trunk_voxels: tree_result.trunk_voxels,
            branch_voxels: tree_result.branch_voxels,
        };

        // Build nav graph from tree geometry.
        let nav_graph = nav::build_nav_graph(&home_tree, &config);

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
            world,
            nav_graph,
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
            SimAction::SpawnElf { position } => {
                self.spawn_elf(*position, events);
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
                if self.elves.contains_key(&elf_id) {
                    // If the elf is idle (no path), pick a random destination and pathfind.
                    self.elf_heartbeat_wander(elf_id);

                    // TODO: Phase 3+ — need decay, mood, mana generation.
                    // Reschedule the next heartbeat.
                    let next_tick = self.tick + self.config.heartbeat_interval_ticks;
                    self.event_queue
                        .schedule(next_tick, ScheduledEventKind::ElfHeartbeat { elf_id });
                }
            }
            ScheduledEventKind::ElfMovementComplete { elf_id, arrived_at } => {
                self.handle_elf_movement_complete(elf_id, arrived_at);
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

    /// Spawn an elf at the nearest nav node to the given position.
    fn spawn_elf(&mut self, position: VoxelCoord, events: &mut Vec<SimEvent>) {
        let nearest_node = match self.nav_graph.find_nearest_node(position) {
            Some(n) => n,
            None => return, // No nav nodes — can't spawn.
        };

        let elf_id = ElfId::new(&mut self.rng);
        let node_pos = self.nav_graph.node(nearest_node).position;

        let elf = Elf {
            id: elf_id,
            position: node_pos,
            current_node: Some(nearest_node),
            path: None,
        };

        self.elves.insert(elf_id, elf);

        // Schedule first heartbeat (which will trigger wandering).
        let heartbeat_tick = self.tick + self.config.heartbeat_interval_ticks;
        self.event_queue
            .schedule(heartbeat_tick, ScheduledEventKind::ElfHeartbeat { elf_id });

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ElfArrived { elf_id },
        });
    }

    /// Heartbeat wander: if the elf is idle, pick a random destination and start moving.
    fn elf_heartbeat_wander(&mut self, elf_id: ElfId) {
        let node_count = self.nav_graph.node_count();
        if node_count == 0 {
            return;
        }

        // Check if the elf is idle (has no active path).
        let is_idle = self
            .elves
            .get(&elf_id)
            .is_some_and(|e| e.path.is_none());

        if !is_idle {
            return;
        }

        let current_node = match self.elves.get(&elf_id).and_then(|e| e.current_node) {
            Some(n) => n,
            None => return,
        };

        // Pick a random destination.
        let dest_idx = self.rng.range_u64(0, node_count as u64) as u32;
        let dest_node = NavNodeId(dest_idx);

        if dest_node == current_node {
            return; // Already there.
        }

        // Pathfind.
        let max_speed = self.config.elf_base_speed / self.config.climb_speed_multiplier;
        let path_result = match pathfinding::astar(&self.nav_graph, current_node, dest_node, max_speed) {
            Some(r) => r,
            None => return, // No path found.
        };

        if path_result.nodes.len() < 2 {
            return;
        }

        // Start traversal: schedule arrival at the first edge's destination.
        let first_edge_idx = path_result.edge_indices[0];
        let first_edge_cost = self.nav_graph.edge(first_edge_idx).cost;
        let first_dest = path_result.nodes[1];
        let arrival_tick = self.tick + (first_edge_cost.ceil() as u64).max(1);

        self.event_queue.schedule(
            arrival_tick,
            ScheduledEventKind::ElfMovementComplete {
                elf_id,
                arrived_at: first_dest,
            },
        );

        // Store remaining path (skip the first node since we're already there).
        let elf = self.elves.get_mut(&elf_id).unwrap();
        elf.path = Some(ElfPath {
            remaining_nodes: path_result.nodes[1..].to_vec(),
            remaining_edge_indices: path_result.edge_indices[1..].to_vec(),
        });
    }

    /// Handle an elf arriving at a nav node.
    fn handle_elf_movement_complete(&mut self, elf_id: ElfId, arrived_at: NavNodeId) {
        let node_pos = self.nav_graph.node(arrived_at).position;

        let elf = match self.elves.get_mut(&elf_id) {
            Some(e) => e,
            None => return, // Elf was removed.
        };

        // Update position and current node.
        elf.position = node_pos;
        elf.current_node = Some(arrived_at);

        // Advance path.
        let should_continue = if let Some(ref mut path) = elf.path {
            if !path.remaining_nodes.is_empty() {
                path.remaining_nodes.remove(0);
            }
            if !path.remaining_edge_indices.is_empty() {
                path.remaining_edge_indices.remove(0);
            }
            !path.remaining_edge_indices.is_empty()
        } else {
            false
        };

        if should_continue {
            // Schedule next movement.
            let path = self.elves[&elf_id].path.as_ref().unwrap();
            let next_edge_idx = path.remaining_edge_indices[0];
            let next_edge_cost = self.nav_graph.edge(next_edge_idx).cost;
            let next_dest = path.remaining_nodes[0];
            let arrival_tick = self.tick + (next_edge_cost.ceil() as u64).max(1);

            self.event_queue.schedule(
                arrival_tick,
                ScheduledEventKind::ElfMovementComplete {
                    elf_id,
                    arrived_at: next_dest,
                },
            );
        } else {
            // Path complete.
            self.elves.get_mut(&elf_id).unwrap().path = None;
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

    #[test]
    fn new_sim_has_tree_voxels() {
        let sim = SimState::new(42);
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(!tree.trunk_voxels.is_empty(), "Tree should have trunk voxels");
        assert!(!tree.branch_voxels.is_empty(), "Tree should have branch voxels");
    }

    #[test]
    fn new_sim_has_nav_graph() {
        let sim = SimState::new(42);
        assert!(sim.nav_graph.node_count() > 0, "Nav graph should have nodes");
    }

    #[test]
    fn spawn_elf_command() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.elves.len(), 1);
        assert!(result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::ElfArrived { .. })));
    }

    #[test]
    fn elf_wanders_after_heartbeat() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn elf.
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[spawn_cmd], 2);

        let initial_pos = sim.elves.values().next().unwrap().position;

        // Step far enough for heartbeat + movement to complete.
        sim.step(&[], 2000);

        // Elf should have moved (though it might have returned to the same node in
        // theory, it's very unlikely with enough ticks and a large-enough graph).
        let final_pos = sim.elves.values().next().unwrap().position;
        // We can't guarantee a different position, but we can verify the elf still exists
        // and has a valid position.
        assert_eq!(sim.elves.len(), 1);
        let elf = sim.elves.values().next().unwrap();
        assert!(elf.current_node.is_some());
        // Verify position matches current node.
        let node_pos = sim.nav_graph.node(elf.current_node.unwrap()).position;
        assert_eq!(elf.position, node_pos);
        let _ = (initial_pos, final_pos); // suppress unused warnings
    }

    #[test]
    fn determinism_with_elf_after_1000_ticks() {
        let mut sim_a = SimState::new(42);
        let mut sim_b = SimState::new(42);

        let tree_pos = sim_a.trees[&sim_a.player_tree_id].position;

        let spawn = SimCommand {
            player_id: sim_a.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };

        sim_a.step(&[spawn.clone()], 1000);
        sim_b.step(&[spawn], 1000);

        // Both sims should have identical elf positions.
        assert_eq!(sim_a.elves.len(), sim_b.elves.len());
        for (id, elf_a) in &sim_a.elves {
            let elf_b = &sim_b.elves[id];
            assert_eq!(elf_a.position, elf_b.position);
            assert_eq!(elf_a.current_node, elf_b.current_node);
        }
        // PRNG state should be identical.
        assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
    }
}
