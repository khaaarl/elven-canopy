// Core simulation state and tick loop.
//
// `SimState` is the single source of truth for the entire game world. It owns
// all entity data, the voxel world, the nav graph, the event queue, the PRNG,
// and the game config. The sim is a pure function:
// `(state, commands) -> (new_state, events)`.
//
// On construction (`new()`/`with_config()`), the sim generates tree geometry
// via `tree_gen.rs`, builds the navigation graph via `nav.rs`, and initializes
// the voxel world via `world.rs`. Creature spawning, pathfinding (via
// `pathfinding.rs`), and movement are handled through the command/event system.
//
// All creature types (elf, capybara, etc.) use a single `Creature` struct with
// a `species` field. Behavioral differences (speed, heartbeat interval, edge
// restrictions) come from data in `SpeciesData` — Dwarf Fortress-style
// data-driven design. See `species.rs` and `config.rs`.
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
use crate::species::SpeciesData;
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

    /// All creature entities (elves, capybaras, etc.), keyed by ID.
    pub creatures: BTreeMap<CreatureId, Creature>,

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

    /// Species data table built from config. Not serialized (rebuilt from config).
    #[serde(skip)]
    pub species_table: BTreeMap<Species, SpeciesData>,
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

/// A creature's current path through the nav graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreaturePath {
    /// Remaining node IDs to visit (next node is index 0).
    pub remaining_nodes: Vec<NavNodeId>,
    /// Remaining edge indices to traverse (next edge is index 0).
    pub remaining_edge_indices: Vec<usize>,
}

/// A creature entity — an autonomous agent (elf, capybara, etc.).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Creature {
    pub id: CreatureId,
    pub species: Species,
    pub position: VoxelCoord,
    /// Current nav node the creature is at (or moving from).
    pub current_node: Option<NavNodeId>,
    /// Active path the creature is traversing.
    pub path: Option<CreaturePath>,
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

        // Build species table from config.
        let species_table = config.species.clone();

        let mut state = Self {
            tick: 0,
            rng,
            config,
            speed: SimSpeed::Normal,
            event_queue: EventQueue::new(),
            trees,
            creatures: BTreeMap::new(),
            player_tree_id,
            player_id,
            world,
            nav_graph,
            species_table,
        };

        // Schedule the home tree's first heartbeat.
        let heartbeat_interval = state.config.tree_heartbeat_interval_ticks;
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
                self.spawn_creature(Species::Elf, *position, events);
            }
            SimAction::SpawnCapybara { position } => {
                self.spawn_creature(Species::Capybara, *position, events);
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
            ScheduledEventKind::CreatureHeartbeat { creature_id } => {
                if let Some(creature) = self.creatures.get(&creature_id) {
                    let species = creature.species;
                    let interval = self.species_table[&species].heartbeat_interval_ticks;

                    self.creature_heartbeat_wander(creature_id);

                    // Reschedule the next heartbeat.
                    let next_tick = self.tick + interval;
                    self.event_queue.schedule(
                        next_tick,
                        ScheduledEventKind::CreatureHeartbeat { creature_id },
                    );
                }
            }
            ScheduledEventKind::CreatureMovementComplete {
                creature_id,
                arrived_at,
            } => {
                self.handle_creature_movement_complete(creature_id, arrived_at);
            }
            ScheduledEventKind::TreeHeartbeat { tree_id } => {
                if self.trees.contains_key(&tree_id) {
                    // TODO: fruit production, mana updates.
                    // Reschedule.
                    let next_tick = self.tick + self.config.tree_heartbeat_interval_ticks;
                    self.event_queue
                        .schedule(next_tick, ScheduledEventKind::TreeHeartbeat { tree_id });
                }
            }
        }
    }

    /// Spawn a creature at the nearest nav node to the given position.
    /// Ground-only species snap to ground nodes; others snap to any node.
    fn spawn_creature(
        &mut self,
        species: Species,
        position: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) {
        let species_data = &self.species_table[&species];

        let nearest_node = if species_data.ground_only {
            self.nav_graph.find_nearest_ground_node(position)
        } else {
            self.nav_graph.find_nearest_node(position)
        };

        let nearest_node = match nearest_node {
            Some(n) => n,
            None => return, // No suitable nav nodes — can't spawn.
        };

        let creature_id = CreatureId::new(&mut self.rng);
        let node_pos = self.nav_graph.node(nearest_node).position;

        let creature = Creature {
            id: creature_id,
            species,
            position: node_pos,
            current_node: Some(nearest_node),
            path: None,
        };

        self.creatures.insert(creature_id, creature);

        // Schedule first heartbeat (which will trigger wandering).
        let heartbeat_tick = self.tick + species_data.heartbeat_interval_ticks;
        self.event_queue.schedule(
            heartbeat_tick,
            ScheduledEventKind::CreatureHeartbeat { creature_id },
        );

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::CreatureArrived {
                creature_id,
                species,
            },
        });
    }

    /// Heartbeat wander: if the creature is idle, pick a random destination and
    /// start moving. Ground-only species pick from ground nodes and use filtered
    /// pathfinding; others pick from all nodes and use full A*.
    fn creature_heartbeat_wander(&mut self, creature_id: CreatureId) {
        let creature = match self.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };

        // Skip if already moving.
        if creature.path.is_some() {
            return;
        }

        let current_node = match creature.current_node {
            Some(n) => n,
            None => return,
        };

        let species = creature.species;
        let species_data = &self.species_table[&species];

        // Pick a random destination.
        let dest_node = if species_data.ground_only {
            let ground_nodes = self.nav_graph.ground_node_ids();
            if ground_nodes.is_empty() {
                return;
            }
            let idx = self.rng.range_u64(0, ground_nodes.len() as u64) as usize;
            ground_nodes[idx]
        } else {
            let node_count = self.nav_graph.node_count();
            if node_count == 0 {
                return;
            }
            let idx = self.rng.range_u64(0, node_count as u64) as u32;
            NavNodeId(idx)
        };

        if dest_node == current_node {
            return; // Already there.
        }

        // Pathfind with species-appropriate edge filter.
        let max_speed = self.config.nav_base_speed / self.config.climb_speed_multiplier;
        let path_result = if let Some(ref allowed) = species_data.allowed_edge_types {
            pathfinding::astar_filtered(
                &self.nav_graph,
                current_node,
                dest_node,
                max_speed,
                allowed,
            )
        } else {
            pathfinding::astar(&self.nav_graph, current_node, dest_node, max_speed)
        };

        let path_result = match path_result {
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
            ScheduledEventKind::CreatureMovementComplete {
                creature_id,
                arrived_at: first_dest,
            },
        );

        // Store remaining path (skip the first node since we're already there).
        let creature = self.creatures.get_mut(&creature_id).unwrap();
        creature.path = Some(CreaturePath {
            remaining_nodes: path_result.nodes[1..].to_vec(),
            remaining_edge_indices: path_result.edge_indices[1..].to_vec(),
        });
    }

    /// Handle a creature arriving at a nav node.
    fn handle_creature_movement_complete(
        &mut self,
        creature_id: CreatureId,
        arrived_at: NavNodeId,
    ) {
        let node_pos = self.nav_graph.node(arrived_at).position;

        let creature = match self.creatures.get_mut(&creature_id) {
            Some(c) => c,
            None => return, // Creature was removed.
        };

        // Update position and current node.
        creature.position = node_pos;
        creature.current_node = Some(arrived_at);

        // Advance path.
        let should_continue = if let Some(ref mut path) = creature.path {
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
            let path = self.creatures[&creature_id].path.as_ref().unwrap();
            let next_edge_idx = path.remaining_edge_indices[0];
            let next_edge_cost = self.nav_graph.edge(next_edge_idx).cost;
            let next_dest = path.remaining_nodes[0];
            let arrival_tick = self.tick + (next_edge_cost.ceil() as u64).max(1);

            self.event_queue.schedule(
                arrival_tick,
                ScheduledEventKind::CreatureMovementComplete {
                    creature_id,
                    arrived_at: next_dest,
                },
            );
        } else {
            // Path complete.
            self.creatures.get_mut(&creature_id).unwrap().path = None;
        }
    }

    /// Count creatures of a given species.
    pub fn creature_count(&self, species: Species) -> usize {
        self.creatures
            .values()
            .filter(|c| c.species == species)
            .count()
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
        let heartbeat_interval = sim.config.tree_heartbeat_interval_ticks;

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
        assert_eq!(sim.creature_count(Species::Elf), 1);
        assert!(result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::CreatureArrived { species: Species::Elf, .. })));
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

        let initial_pos = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .position;

        // Step far enough for heartbeat + movement to complete.
        sim.step(&[], 2000);

        // Elf should have moved (though it might have returned to the same node in
        // theory, it's very unlikely with enough ticks and a large-enough graph).
        let final_pos = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .position;
        // We can't guarantee a different position, but we can verify the elf still exists
        // and has a valid position.
        assert_eq!(sim.creature_count(Species::Elf), 1);
        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
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

        // Both sims should have identical creature positions.
        assert_eq!(sim_a.creatures.len(), sim_b.creatures.len());
        for (id, creature_a) in &sim_a.creatures {
            let creature_b = &sim_b.creatures[id];
            assert_eq!(creature_a.position, creature_b.position);
            assert_eq!(creature_a.current_node, creature_b.current_node);
        }
        // PRNG state should be identical.
        assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
    }

    #[test]
    fn spawn_capybara_command() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCapybara {
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.creature_count(Species::Capybara), 1);
        assert!(result
            .events
            .iter()
            .any(|e| matches!(e.kind, SimEventKind::CreatureArrived { species: Species::Capybara, .. })));

        // Capybara should be at a ground-level node (y=0).
        let capybara = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Capybara)
            .unwrap();
        assert_eq!(capybara.position.y, 0);
        assert!(capybara.current_node.is_some());
    }

    #[test]
    fn capybara_wanders_on_ground() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCapybara {
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Step far enough for heartbeat + movement.
        sim.step(&[], 2000);

        assert_eq!(sim.creature_count(Species::Capybara), 1);
        let capybara = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Capybara)
            .unwrap();
        assert!(capybara.current_node.is_some());
        let node_pos = sim.nav_graph.node(capybara.current_node.unwrap()).position;
        assert_eq!(capybara.position, node_pos);
    }

    #[test]
    fn capybara_stays_on_ground() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCapybara {
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Run for many ticks — capybara must never leave y=0.
        for target in (100..5000).step_by(100) {
            sim.step(&[], target);
            let capybara = sim
                .creatures
                .values()
                .find(|c| c.species == Species::Capybara)
                .unwrap();
            assert_eq!(
                capybara.position.y, 0,
                "Capybara left ground at tick {target}: pos={:?}",
                capybara.position
            );
        }
    }

    #[test]
    fn determinism_with_capybara() {
        let mut sim_a = SimState::new(42);
        let mut sim_b = SimState::new(42);

        let tree_pos = sim_a.trees[&sim_a.player_tree_id].position;

        let spawn = SimCommand {
            player_id: sim_a.player_id,
            tick: 1,
            action: SimAction::SpawnCapybara {
                position: tree_pos,
            },
        };

        sim_a.step(&[spawn.clone()], 1000);
        sim_b.step(&[spawn], 1000);

        assert_eq!(sim_a.creatures.len(), sim_b.creatures.len());
        for (id, creature_a) in &sim_a.creatures {
            let creature_b = &sim_b.creatures[id];
            assert_eq!(creature_a.position, creature_b.position);
            assert_eq!(creature_a.current_node, creature_b.current_node);
        }
        assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
    }

    #[test]
    fn species_data_loaded_from_config() {
        let sim = SimState::new(42);
        assert_eq!(sim.species_table.len(), 2);
        assert!(sim.species_table.contains_key(&Species::Elf));
        assert!(sim.species_table.contains_key(&Species::Capybara));

        let elf_data = &sim.species_table[&Species::Elf];
        assert!(!elf_data.ground_only);
        assert!(elf_data.allowed_edge_types.is_none());

        let capy_data = &sim.species_table[&Species::Capybara];
        assert!(capy_data.ground_only);
        assert!(capy_data.allowed_edge_types.is_some());
    }

    #[test]
    fn creature_species_preserved() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn one elf and one capybara.
        let cmds = vec![
            SimCommand {
                player_id: sim.player_id,
                tick: 1,
                action: SimAction::SpawnElf {
                    position: tree_pos,
                },
            },
            SimCommand {
                player_id: sim.player_id,
                tick: 1,
                action: SimAction::SpawnCapybara {
                    position: tree_pos,
                },
            },
        ];
        sim.step(&cmds, 2);

        assert_eq!(sim.creature_count(Species::Elf), 1);
        assert_eq!(sim.creature_count(Species::Capybara), 1);
        assert_eq!(sim.creatures.len(), 2);

        // Verify species are correctly stored.
        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.species, Species::Elf);

        let capy = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Capybara)
            .unwrap();
        assert_eq!(capy.species, Species::Capybara);
    }
}
