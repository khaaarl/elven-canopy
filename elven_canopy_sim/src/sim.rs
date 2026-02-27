// Core simulation state and tick loop.
//
// `SimState` is the single source of truth for the entire game world. It owns
// all entity data, the voxel world, the nav graph, the event queue, the PRNG,
// and the game config. The sim is a pure function:
// `(state, commands) -> (new_state, events)`.
//
// On construction (`new()`/`with_config()`), the sim generates tree geometry
// via `tree_gen.rs`, builds the navigation graph via `nav.rs`, and initializes
// the voxel world via `world.rs`. Creature spawning and movement are handled
// through the command/event system.
//
// ## Activation chain
//
// Creature movement uses an **activation chain**: each creature has a
// `CreatureActivation` event that fires, performs one action (walk 1 nav edge
// or do 1 unit of task work), and schedules the next activation based on how
// long the action takes. The sim runs at **1000 ticks per simulated second**
// (`tick_duration_ms = 1`). Edge traversal time is computed as
// `ceil(edge.distance * species_ticks_per_voxel)` where ticks_per_voxel is
// `walk_ticks_per_voxel` for flat edges or `climb_ticks_per_voxel` for
// TrunkClimb/GroundToTrunk edges (from `species.rs`).
//
// `CreatureHeartbeat` still exists but is decoupled from movement; it handles
// periodic non-movement checks (mood, mana, food decay, etc.).
//
// The activation loop (`process_creature_activation`) runs this logic:
//
//   1. If the creature has no task (`current_task == None`), check for an
//      available task to claim. If none found, **wander**: pick a random
//      adjacent nav node, move there, and schedule the next activation at
//      `now + ceil(edge.distance * ticks_per_voxel)`.
//   2. If the creature has a task, run its **behavior script** (see below).
//
// Wandering is intentionally local and aimless — no pathfinding, just 1 random
// neighbor per activation. This creates natural-looking milling about.
//
// ## Task system
//
// Tasks are the core assignment mechanism. The sim maintains a task registry
// (`BTreeMap<TaskId, Task>`) and each creature stores an optional `current_task`.
//
// ### Task entity (`task.rs`)
//
// A `Task` has:
// - `kind: TaskKind` — determines behavior (currently only `GoTo`).
// - `state: TaskState` — lifecycle: `Available` → `InProgress` → `Complete`.
// - `location: NavNodeId` — where creatures go to work on the task.
// - `assignees: Vec<CreatureId>` — supports multiple workers.
// - `progress: f32` and `total_cost: f32` — for tasks that require work units
//   (0.0 total_cost means instant completion, e.g. GoTo).
// - `required_species: Option<Species>` — species restriction (if `Some`,
//   only that species can claim it).
//
// ### Task lifecycle
//
// 1. A `CreateTask` command (from the UI via `sim_bridge.rs`) creates a task
//    in `Available` state, snapped to the nearest nav node.
// 2. On its next activation, an idle creature whose species matches calls
//    `find_available_task`, which returns the first `Available` task in
//    deterministic BTreeMap order. The creature calls `claim_task`, which
//    sets the task to `InProgress`, adds the creature to `assignees`, and
//    computes an A* path to `task.location`.
// 3. Each subsequent activation runs the task's behavior script (see below).
// 4. On completion, `complete_task` sets the task to `Complete` and clears
//    `current_task` for all assignees, returning them to wandering.
//
// Only one creature can transition a task from `Available` → `InProgress`.
// Once `InProgress`, `find_available_task` skips it, preventing pile-ons.
// (Multi-worker tasks are structurally supported via `assignees` but not yet
// used — a future task kind could transition back to `Available` to recruit
// more workers.)
//
// ### Task behavior scripts
//
// Each `TaskKind` defines behavior evaluated per activation via match dispatch
// in `execute_task_behavior`:
//
//   GoTo:
//     - If at `task.location` → complete instantly (total_cost is 0).
//     - Otherwise → walk 1 edge along the A* path toward the location.
//
// Future kinds (Build, Harvest, etc.) would follow the same pattern: walk
// toward location if not there, otherwise do one increment of work per
// activation, completing when `progress >= total_cost`.
//
// ### Task assignment details
//
// `find_available_task` filters by: (1) `TaskState::Available`, (2) species
// compatibility (`required_species` is `None` or matches the creature's
// species). It returns the first match in BTreeMap iteration order, which is
// deterministic by `TaskId`.
//
// Task checking happens on every activation of an idle creature. This is
// simple and correct; optimization (checking less frequently) is deferred.
//
// ## Species
//
// All creature types (elf, capybara, etc.) use a single `Creature` struct with
// a `species` field. Behavioral differences (speed, heartbeat interval, edge
// restrictions) come from data in `SpeciesData` — Dwarf Fortress-style
// data-driven design. See `species.rs` and `config.rs`.
//
// ## Save/load
//
// `SimState` derives `Serialize`/`Deserialize` via serde. Three transient
// fields (`world`, `nav_graph`, `species_table`) are `#[serde(skip)]` and
// must be rebuilt after deserialization via `rebuild_transient_state()`.
// Convenience methods `to_json()` and `from_json()` handle the full
// serialize/deserialize + rebuild cycle. `rebuild_world()` reconstructs the
// voxel grid from stored tree voxel lists and config (forest floor extent).
//
// See also: `event.rs` for the event queue, `command.rs` for `SimCommand`,
// `config.rs` for `GameConfig`, `types.rs` for entity IDs, `world.rs` for
// the voxel grid, `nav.rs` for navigation, `pathfinding.rs` for A*,
// `task.rs` for `Task`/`TaskKind`/`TaskState`, `blueprint.rs` for the
// blueprint data model used by `DesignateBuild`/`CancelBuild` commands.
//
// **Critical constraint: determinism.** All state mutations flow through
// `SimCommand` or internal scheduled events. No external input (system time,
// thread state, etc.) may influence the simulation.

use crate::blueprint::Blueprint;
use crate::blueprint::BlueprintState;
use crate::command::{SimAction, SimCommand};
use crate::config::GameConfig;
use crate::event::{EventQueue, ScheduledEventKind, SimEvent, SimEventKind};
use crate::nav::{self, NavGraph};
use crate::task;
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

    /// All tasks (go-to, build, harvest, etc.), keyed by ID.
    pub tasks: BTreeMap<TaskId, task::Task>,

    /// All build blueprints (designated or complete), keyed by ProjectId.
    #[serde(default)]
    pub blueprints: BTreeMap<ProjectId, Blueprint>,

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
    /// Leaf voxel positions (blobs at branch terminals).
    pub leaf_voxels: Vec<VoxelCoord>,
    /// Root voxel positions (at or below ground level).
    pub root_voxels: Vec<VoxelCoord>,
    /// Positions of fruit hanging below leaf voxels.
    pub fruit_positions: Vec<VoxelCoord>,
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
    /// Active path the creature is traversing (used when walking toward a task).
    pub path: Option<CreaturePath>,
    /// The task this creature is currently assigned to, or `None` for wandering.
    pub current_task: Option<TaskId>,
    /// Food gauge. Starts at species `food_max`, decreases by
    /// `food_decay_per_tick` each tick (batch-applied at heartbeat). 0 = starving.
    #[serde(default = "default_food")]
    pub food: i64,
}

fn default_food() -> i64 {
    1_000_000_000_000_000
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
            leaf_voxels: tree_result.leaf_voxels,
            root_voxels: tree_result.root_voxels,
            fruit_positions: Vec::new(),
        };

        // Build nav graph from voxel world geometry.
        let nav_graph = nav::build_nav_graph(&world);

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
            tasks: BTreeMap::new(),
            blueprints: BTreeMap::new(),
            player_tree_id,
            player_id,
            world,
            nav_graph,
            species_table,
        };

        // Fast-forward fruit spawning: run the same attempt_fruit_spawn code
        // path N times, as if N heartbeats had already passed for fruit.
        let initial_attempts = state.config.fruit_initial_attempts;
        for _ in 0..initial_attempts {
            state.attempt_fruit_spawn(player_tree_id);
        }

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
            SimAction::DesignateBuild {
                build_type,
                voxels,
                priority,
            } => {
                self.designate_build(*build_type, voxels, *priority, events);
            }
            SimAction::CancelBuild { project_id } => {
                self.cancel_build(*project_id, events);
            }
            SimAction::SetTaskPriority { .. } => {
                // TODO: Phase 2 — task system.
            }
            SimAction::CreateTask {
                kind,
                position,
                required_species,
            } => {
                self.create_task(kind.clone(), *position, *required_species);
            }
        }
    }

    /// Validate and create a blueprint from a `DesignateBuild` command.
    ///
    /// Validation (silent no-op on failure, consistent with other commands):
    /// - Voxels must be non-empty.
    /// - All voxels must be in-bounds.
    /// - All voxels must be Air.
    /// - At least one voxel must have a solid face neighbor.
    fn designate_build(
        &mut self,
        build_type: BuildType,
        voxels: &[VoxelCoord],
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        if voxels.is_empty() {
            return;
        }
        for &coord in voxels {
            if !self.world.in_bounds(coord) {
                return;
            }
            if self.world.get(coord) != VoxelType::Air {
                return;
            }
        }
        let any_adjacent = voxels
            .iter()
            .any(|&coord| self.world.has_solid_face_neighbor(coord));
        if !any_adjacent {
            return;
        }

        let project_id = ProjectId::new(&mut self.rng);
        let bp = Blueprint {
            id: project_id,
            build_type,
            voxels: voxels.to_vec(),
            priority,
            state: BlueprintState::Designated,
        };
        self.blueprints.insert(project_id, bp);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Cancel a blueprint by ProjectId. Emits `BuildCancelled` if found.
    /// Silent no-op if the ProjectId doesn't exist (idempotent for multiplayer).
    fn cancel_build(
        &mut self,
        project_id: ProjectId,
        events: &mut Vec<SimEvent>,
    ) {
        if self.blueprints.remove(&project_id).is_some() {
            events.push(SimEvent {
                tick: self.tick,
                kind: SimEventKind::BuildCancelled { project_id },
            });
        }
    }

    /// Create a task at the nearest nav node to the given position.
    fn create_task(
        &mut self,
        kind: task::TaskKind,
        position: VoxelCoord,
        required_species: Option<Species>,
    ) {
        let location = match self.nav_graph.find_nearest_node(position) {
            Some(n) => n,
            None => return,
        };
        let task_id = TaskId::new(&mut self.rng);
        let new_task = task::Task {
            id: task_id,
            kind,
            state: task::TaskState::Available,
            location,
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: 0.0,
            required_species,
        };
        self.tasks.insert(task_id, new_task);
    }

    /// Process a single scheduled event.
    fn process_event(&mut self, kind: ScheduledEventKind, _events: &mut Vec<SimEvent>) {
        match kind {
            ScheduledEventKind::CreatureHeartbeat { creature_id } => {
                // Heartbeat is for periodic non-movement checks (mood, mana, etc.).
                // Movement is driven by CreatureActivation, not heartbeats.
                if let Some(creature) = self.creatures.get_mut(&creature_id) {
                    let species = creature.species;
                    let species_data = &self.species_table[&species];
                    let interval = species_data.heartbeat_interval_ticks;
                    let decay = species_data.food_decay_per_tick * interval as i64;
                    creature.food = (creature.food - decay).max(0);

                    // TODO: mood decay, mana generation.

                    // Reschedule the next heartbeat.
                    let next_tick = self.tick + interval;
                    self.event_queue.schedule(
                        next_tick,
                        ScheduledEventKind::CreatureHeartbeat { creature_id },
                    );
                }
            }
            ScheduledEventKind::CreatureActivation { creature_id } => {
                self.process_creature_activation(creature_id);
            }
            ScheduledEventKind::CreatureMovementComplete {
                creature_id,
                arrived_at,
            } => {
                self.handle_creature_movement_complete(creature_id, arrived_at);
            }
            ScheduledEventKind::TreeHeartbeat { tree_id } => {
                if self.trees.contains_key(&tree_id) {
                    // Fruit production.
                    self.attempt_fruit_spawn(tree_id);

                    // TODO: mana updates.

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
            current_task: None,
            food: species_data.food_max,
        };

        self.creatures.insert(creature_id, creature);

        // Schedule first activation (drives movement — wander or task work).
        // Fires 1 tick after spawn so the creature starts moving immediately.
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );

        // Schedule first heartbeat (periodic non-movement checks).
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

    /// Creature activation: the creature does one action and schedules its next
    /// activation based on how long the action takes.
    ///
    /// If the creature has a task, run the task's behavior script (walk toward
    /// location or complete on arrival). If idle (no task), check for available
    /// tasks and claim one, or wander randomly.
    ///
    /// Species edge restrictions are respected for wandering; task pathfinding
    /// uses species-filtered A*.
    fn process_creature_activation(&mut self, creature_id: CreatureId) {
        let creature = match self.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };

        let current_node = match creature.current_node {
            Some(n) => n,
            None => return,
        };

        let current_task = creature.current_task;

        if let Some(task_id) = current_task {
            // --- Has task: run task behavior ---
            self.execute_task_behavior(creature_id, task_id, current_node);
        } else {
            // --- No task: try to claim one, or wander ---
            if let Some(task_id) = self.find_available_task(creature_id) {
                self.claim_task(creature_id, task_id);
                // Run task behavior immediately on the same activation.
                self.execute_task_behavior(creature_id, task_id, current_node);
            } else {
                self.wander(creature_id, current_node);
            }
        }
    }

    /// Find the first available task this creature can work on.
    /// Respects species restrictions: tasks with `required_species` are only
    /// visible to matching creatures.
    fn find_available_task(&self, creature_id: CreatureId) -> Option<TaskId> {
        let creature = self.creatures.get(&creature_id)?;
        let species = creature.species;

        self.tasks
            .values()
            .find(|t| {
                t.state == task::TaskState::Available
                    && t.required_species.map_or(true, |s| s == species)
            })
            .map(|t| t.id)
    }

    /// Assign a creature to a task.
    fn claim_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        if let Some(task) = self.tasks.get_mut(&task_id) {
            task.assignees.push(creature_id);
            task.state = task::TaskState::InProgress;
        }
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.current_task = Some(task_id);
        }
    }

    /// Execute one activation's worth of task behavior.
    fn execute_task_behavior(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        current_node: NavNodeId,
    ) {
        let task_location = match self.tasks.get(&task_id) {
            Some(t) => t.location,
            None => {
                // Task was removed — unassign and wander.
                if let Some(c) = self.creatures.get_mut(&creature_id) {
                    c.current_task = None;
                    c.path = None;
                }
                self.wander(creature_id, current_node);
                return;
            }
        };

        if current_node == task_location {
            // At task location — run the kind-specific completion/work logic.
            self.execute_task_at_location(creature_id, task_id);
        } else {
            // Not at location — walk one edge toward it.
            self.walk_toward_task(creature_id, task_location, current_node);
        }
    }

    /// Execute task-kind-specific logic when the creature is at the task location.
    fn execute_task_at_location(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let task = match self.tasks.get(&task_id) {
            Some(t) => t,
            None => return,
        };

        match task.kind {
            task::TaskKind::GoTo => {
                // GoTo completes instantly on arrival.
                self.complete_task(task_id);
            }
            // Future: Build → do_work_increment, etc.
        }

        // Schedule next activation (creature is now idle, will wander or pick
        // up another task).
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Complete a task: set state to Complete, unassign all workers.
    fn complete_task(&mut self, task_id: TaskId) {
        let assignee_ids: Vec<CreatureId> = if let Some(task) = self.tasks.get_mut(&task_id) {
            task.state = task::TaskState::Complete;
            task.assignees.clone()
        } else {
            return;
        };

        for cid in &assignee_ids {
            if let Some(creature) = self.creatures.get_mut(cid) {
                creature.current_task = None;
                creature.path = None;
            }
        }
    }

    /// Walk one edge toward a task location using a stored or computed A* path.
    fn walk_toward_task(
        &mut self,
        creature_id: CreatureId,
        task_location: NavNodeId,
        current_node: NavNodeId,
    ) {
        let creature = match self.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;
        let species_data = &self.species_table[&species];

        // Check if we already have a path. If so, advance one step.
        // If not (or path is exhausted), compute a new one.
        let next_step = if let Some(ref path) = creature.path {
            if !path.remaining_edge_indices.is_empty() {
                Some((path.remaining_edge_indices[0], path.remaining_nodes[0]))
            } else {
                None
            }
        } else {
            None
        };

        let walk_tpv = species_data.walk_ticks_per_voxel;
        let climb_tpv = species_data.climb_ticks_per_voxel;

        let (edge_idx, dest_node) = if let Some(step) = next_step {
            step
        } else {
            // Compute path to task location.
            let path_result = if let Some(ref allowed) = species_data.allowed_edge_types {
                pathfinding::astar_filtered(
                    &self.nav_graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                    allowed,
                )
            } else {
                pathfinding::astar(
                    &self.nav_graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                )
            };

            let path_result = match path_result {
                Some(r) if r.nodes.len() >= 2 => r,
                _ => {
                    // Can't reach task — unassign and wander.
                    self.unassign_creature_from_task(creature_id);
                    self.wander(creature_id, current_node);
                    return;
                }
            };

            let first_edge = path_result.edge_indices[0];
            let first_dest = path_result.nodes[1];

            // Store remaining path for future activations.
            let creature = self.creatures.get_mut(&creature_id).unwrap();
            creature.path = Some(CreaturePath {
                remaining_nodes: path_result.nodes[1..].to_vec(),
                remaining_edge_indices: path_result.edge_indices.to_vec(),
            });

            (first_edge, first_dest)
        };

        // Move one edge. Compute traversal time from distance * ticks-per-voxel.
        let edge = self.nav_graph.edge(edge_idx);
        let tpv = match edge.edge_type {
            crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                climb_tpv.unwrap_or(walk_tpv)
            }
            _ => walk_tpv,
        };
        let delay = ((edge.distance * tpv as f32).ceil() as u64).max(1);
        let dest_pos = self.nav_graph.node(dest_node).position;

        let creature = self.creatures.get_mut(&creature_id).unwrap();
        creature.position = dest_pos;
        creature.current_node = Some(dest_node);

        // Advance stored path.
        if let Some(ref mut path) = creature.path {
            if !path.remaining_nodes.is_empty() {
                path.remaining_nodes.remove(0);
            }
            if !path.remaining_edge_indices.is_empty() {
                path.remaining_edge_indices.remove(0);
            }
        }

        // Schedule next activation.
        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Remove a creature from its assigned task.
    fn unassign_creature_from_task(&mut self, creature_id: CreatureId) {
        let task_id = match self.creatures.get(&creature_id) {
            Some(c) => c.current_task,
            None => return,
        };
        if let Some(tid) = task_id {
            if let Some(task) = self.tasks.get_mut(&tid) {
                task.assignees.retain(|&id| id != creature_id);
                if task.assignees.is_empty()
                    && matches!(task.state, task::TaskState::InProgress)
                {
                    task.state = task::TaskState::Available;
                }
            }
        }
        if let Some(creature) = self.creatures.get_mut(&creature_id) {
            creature.current_task = None;
            creature.path = None;
        }
    }

    /// Wander: pick a random adjacent nav node and move there.
    fn wander(&mut self, creature_id: CreatureId, current_node: NavNodeId) {
        let creature = match self.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;
        let species_data = &self.species_table[&species];

        let edge_indices = self.nav_graph.neighbors(current_node);
        if edge_indices.is_empty() {
            self.event_queue.schedule(
                self.tick + 1000,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        }

        // Filter edges by species restrictions.
        let eligible_edges: Vec<usize> = if let Some(ref allowed) = species_data.allowed_edge_types
        {
            edge_indices
                .iter()
                .copied()
                .filter(|&idx| allowed.contains(&self.nav_graph.edge(idx).edge_type))
                .collect()
        } else {
            edge_indices.to_vec()
        };

        if eligible_edges.is_empty() {
            self.event_queue.schedule(
                self.tick + 1000,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        }

        // Pick a random eligible edge.
        let chosen_idx = self.rng.range_u64(0, eligible_edges.len() as u64) as usize;
        let edge_idx = eligible_edges[chosen_idx];
        let edge = self.nav_graph.edge(edge_idx);
        let dest_node = edge.to;

        // Compute traversal time from distance * species ticks-per-voxel.
        let tpv = match edge.edge_type {
            crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                species_data.climb_ticks_per_voxel.unwrap_or(species_data.walk_ticks_per_voxel)
            }
            _ => species_data.walk_ticks_per_voxel,
        };
        let delay = ((edge.distance * tpv as f32).ceil() as u64).max(1);

        // Move creature to the destination.
        let dest_pos = self.nav_graph.node(dest_node).position;
        let creature = self.creatures.get_mut(&creature_id).unwrap();
        creature.position = dest_pos;
        creature.current_node = Some(dest_node);

        // Schedule next activation based on edge traversal time.
        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
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

        let species = creature.species;

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
            // Schedule next movement using distance * species ticks-per-voxel.
            let species_data = &self.species_table[&species];
            let path = self.creatures[&creature_id].path.as_ref().unwrap();
            let next_edge_idx = path.remaining_edge_indices[0];
            let next_edge = self.nav_graph.edge(next_edge_idx);
            let tpv = match next_edge.edge_type {
                crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                    species_data.climb_ticks_per_voxel.unwrap_or(species_data.walk_ticks_per_voxel)
                }
                _ => species_data.walk_ticks_per_voxel,
            };
            let delay = ((next_edge.distance * tpv as f32).ceil() as u64).max(1);
            let next_dest = path.remaining_nodes[0];
            let arrival_tick = self.tick + delay;

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

    /// Attempt to spawn one fruit on the given tree. Rolls the RNG for spawn
    /// chance and picks a random leaf voxel to hang fruit below. Returns `true`
    /// if a fruit was actually placed.
    ///
    /// This is the single code path for all fruit spawning — both the initial
    /// fast-forward during `with_config()` and the periodic `TreeHeartbeat`.
    fn attempt_fruit_spawn(&mut self, tree_id: TreeId) -> bool {
        let tree = match self.trees.get(&tree_id) {
            Some(t) => t,
            None => return false,
        };

        if tree.fruit_positions.len() >= self.config.fruit_max_per_tree as usize {
            return false;
        }

        // Roll spawn chance.
        let roll = self.rng.next_f32();
        if roll >= self.config.fruit_production_base_rate {
            return false;
        }

        if tree.leaf_voxels.is_empty() {
            return false;
        }

        // Pick a random leaf voxel; fruit hangs one voxel below it.
        let leaf_count = tree.leaf_voxels.len();
        let leaf_idx = self.rng.range_u64(0, leaf_count as u64) as usize;
        let leaf_pos = tree.leaf_voxels[leaf_idx];
        let fruit_pos = VoxelCoord::new(leaf_pos.x, leaf_pos.y - 1, leaf_pos.z);

        // Position must be in-bounds, currently air, and not already fruited.
        if !self.world.in_bounds(fruit_pos) {
            return false;
        }
        if self.world.get(fruit_pos) != VoxelType::Air {
            return false;
        }
        if tree.fruit_positions.contains(&fruit_pos) {
            return false;
        }

        // Place the fruit.
        self.world.set(fruit_pos, VoxelType::Fruit);
        let tree = self.trees.get_mut(&tree_id).unwrap();
        tree.fruit_positions.push(fruit_pos);
        true
    }

    // -----------------------------------------------------------------------
    // Save/load helpers
    // -----------------------------------------------------------------------

    /// Rebuild the voxel world from config and stored tree entity data.
    ///
    /// Recreates the `VoxelWorld` from scratch: lays the forest floor at y=0
    /// using `config.floor_extent`, then places every tree's trunk, branch,
    /// root, leaf, and fruit voxels. This is the inverse of tree generation —
    /// instead of growing the tree procedurally, we replay the stored voxel
    /// lists.
    pub fn rebuild_world(config: &GameConfig, trees: &BTreeMap<TreeId, Tree>) -> VoxelWorld {
        let (ws_x, ws_y, ws_z) = config.world_size;
        let mut world = VoxelWorld::new(ws_x, ws_y, ws_z);

        // Lay forest floor.
        let center_x = ws_x as i32 / 2;
        let center_z = ws_z as i32 / 2;
        let floor_extent = config.floor_extent;
        for dx in -floor_extent..=floor_extent {
            for dz in -floor_extent..=floor_extent {
                let coord = VoxelCoord::new(center_x + dx, 0, center_z + dz);
                world.set(coord, VoxelType::ForestFloor);
            }
        }

        // Place tree voxels. Priority order: Trunk > Branch > Root > Leaf > Fruit.
        for tree in trees.values() {
            for &coord in &tree.trunk_voxels {
                world.set(coord, VoxelType::Trunk);
            }
            for &coord in &tree.branch_voxels {
                world.set(coord, VoxelType::Branch);
            }
            for &coord in &tree.root_voxels {
                world.set(coord, VoxelType::Root);
            }
            for &coord in &tree.leaf_voxels {
                world.set(coord, VoxelType::Leaf);
            }
            for &coord in &tree.fruit_positions {
                world.set(coord, VoxelType::Fruit);
            }
        }

        world
    }

    /// Rebuild all transient (`#[serde(skip)]`) fields after deserialization.
    ///
    /// Restores: `world` (voxel grid from stored tree voxels + config),
    /// `nav_graph` (from rebuilt world geometry), `species_table` (from config).
    pub fn rebuild_transient_state(&mut self) {
        self.world = Self::rebuild_world(&self.config, &self.trees);
        self.nav_graph = nav::build_nav_graph(&self.world);
        self.species_table = self.config.species.clone();
    }

    /// Serialize the simulation state to a JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize a simulation state from a JSON string and rebuild
    /// transient fields (world, nav_graph, species_table).
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut state: SimState = serde_json::from_str(json)?;
        state.rebuild_transient_state();
        Ok(state)
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
    use crate::task::{Task, TaskKind, TaskState};

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
    fn elf_wanders_after_spawn() {
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

        // Step far enough for many activations (each ground edge costs ~500
        // ticks at walk_ticks_per_voxel=500).
        sim.step(&[], 200000);

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

        // Capybara should be at a ground-level node (y=1, air above ForestFloor).
        let capybara = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Capybara)
            .unwrap();
        assert_eq!(capybara.position.y, 1);
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
        sim.step(&[], 200000);

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

        // Run for many ticks — capybara must never leave y=1 (air above ForestFloor).
        for target in (10000..500000).step_by(10000) {
            sim.step(&[], target);
            let capybara = sim
                .creatures
                .values()
                .find(|c| c.species == Species::Capybara)
                .unwrap();
            assert_eq!(
                capybara.position.y, 1,
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
    fn creature_wanders_via_activation_chain() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        let initial_node = elf.current_node.unwrap();
        let initial_pos = elf.position;

        // Step enough for many activations (each moves 1 edge; ground edges
        // cost ~500 ticks at walk_ticks_per_voxel=500).
        sim.step(&[], 200000);

        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        let final_node = elf.current_node.unwrap();

        // After many activations, creature should have moved.
        assert_ne!(
            initial_node, final_node,
            "Elf should have moved after activation chain"
        );
        // Position should match current node.
        let node_pos = sim.nav_graph.node(final_node).position;
        assert_eq!(elf.position, node_pos);
        // Creature should not have a stored path (wandering doesn't use paths).
        assert!(
            elf.path.is_none(),
            "Wandering creature should not have a stored path"
        );
        let _ = initial_pos;
    }

    #[test]
    fn wandering_creature_stays_on_nav_graph() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Run for many ticks, periodically checking node validity.
        for target in (10000..500000).step_by(10000) {
            sim.step(&[], target);
            let elf = sim
                .creatures
                .values()
                .find(|c| c.species == Species::Elf)
                .unwrap();
            let node = elf.current_node.expect("Elf should always have a current node");
            assert!(
                (node.0 as usize) < sim.nav_graph.node_count(),
                "Node ID {} out of range at tick {}",
                node.0,
                target
            );
            let node_pos = sim.nav_graph.node(node).position;
            assert_eq!(
                elf.position, node_pos,
                "Position mismatch at tick {}",
                target
            );
        }
    }

    /// Helper: spawn an elf and return its CreatureId.
    fn spawn_elf(sim: &mut SimState) -> CreatureId {
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
        sim.creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id
    }

    /// Helper: insert a GoTo task at the given nav node (elf-only).
    fn insert_goto_task(sim: &mut SimState, location: NavNodeId) -> TaskId {
        let task_id = TaskId::new(&mut sim.rng);
        let task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location,
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
        };
        sim.tasks.insert(task_id, task);
        task_id
    }

    #[test]
    fn creature_claims_available_task() {
        let mut sim = SimState::new(42);
        let elf_id = spawn_elf(&mut sim);

        // Pick a task location far from the elf — a branch tip node requires
        // climbing the trunk and walking a branch, many hops away.
        let far_node = NavNodeId((sim.nav_graph.node_count() - 1) as u32);
        let task_id = insert_goto_task(&mut sim, far_node);

        // Tick enough for one activation (~500 ticks for a ground edge at
        // walk_ticks_per_voxel=500).
        sim.step(&[], sim.tick + 10000);

        let elf = &sim.creatures[&elf_id];
        assert_eq!(
            elf.current_task,
            Some(task_id),
            "Elf should have claimed the available task"
        );
        let task = &sim.tasks[&task_id];
        assert!(
            task.assignees.contains(&elf_id),
            "Task assignees should include the elf"
        );
        assert_eq!(task.state, TaskState::InProgress);
    }

    #[test]
    fn creature_walks_to_task_location() {
        let mut sim = SimState::new(42);
        let elf_id = spawn_elf(&mut sim);

        // Pick a far task location (branch tip) so the elf has a long walk.
        let far_node = NavNodeId((sim.nav_graph.node_count() - 1) as u32);
        let task_location = sim.nav_graph.node(far_node).position;
        let _task_id = insert_goto_task(&mut sim, far_node);

        let initial_dist = sim.creatures[&elf_id]
            .position
            .manhattan_distance(task_location);

        // Step a moderate amount — creature should be closer to the target.
        sim.step(&[], sim.tick + 50000);

        let mid_dist = sim.creatures[&elf_id]
            .position
            .manhattan_distance(task_location);

        assert!(
            mid_dist < initial_dist,
            "Elf should be closer to task after walking (initial={initial_dist}, mid={mid_dist})"
        );
    }

    #[test]
    fn goto_task_completes_on_arrival() {
        let mut sim = SimState::new(42);
        let elf_id = spawn_elf(&mut sim);

        // Put the task at the elf's current location for instant completion.
        let elf_node = sim.creatures[&elf_id].current_node.unwrap();
        let task_id = insert_goto_task(&mut sim, elf_node);

        // One activation should be enough: elf claims task, is already there, completes.
        sim.step(&[], sim.tick + 10000);

        let task = &sim.tasks[&task_id];
        assert_eq!(task.state, TaskState::Complete, "GoTo task should be complete");
        let elf = &sim.creatures[&elf_id];
        assert_eq!(
            elf.current_task, None,
            "Elf should be unassigned after task completion"
        );
    }

    #[test]
    fn completed_task_creature_resumes_wandering() {
        let mut sim = SimState::new(42);
        let elf_id = spawn_elf(&mut sim);

        // Put the task at the elf's current location for instant completion.
        let elf_node = sim.creatures[&elf_id].current_node.unwrap();
        let _task_id = insert_goto_task(&mut sim, elf_node);

        // Complete the task.
        sim.step(&[], sim.tick + 10000);
        let pos_after_task = sim.creatures[&elf_id].position;

        // Continue ticking — elf should resume wandering (position changes).
        sim.step(&[], sim.tick + 200000);

        let pos_after_wander = sim.creatures[&elf_id].position;
        assert_ne!(
            pos_after_task, pos_after_wander,
            "Elf should have wandered after task completion"
        );
        assert!(
            sim.creatures[&elf_id].current_task.is_none(),
            "Elf should still have no task"
        );
    }

    #[test]
    fn create_task_command_adds_task() {
        let mut sim = SimState::new(42);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::CreateTask {
                kind: TaskKind::GoTo,
                position: VoxelCoord::new(128, 0, 128),
                required_species: Some(Species::Elf),
            },
        };
        sim.step(&[cmd], 2);

        assert_eq!(sim.tasks.len(), 1, "Should have 1 task");
        let task = sim.tasks.values().next().unwrap();
        assert_eq!(task.state, TaskState::Available);
        assert!(matches!(task.kind, TaskKind::GoTo));
    }

    #[test]
    fn end_to_end_summon_task() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn an elf.
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[spawn_cmd], 2);

        // Create a GoTo task at a ground position near the tree.
        let task_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 3,
            action: SimAction::CreateTask {
                kind: TaskKind::GoTo,
                position: VoxelCoord::new(tree_pos.x + 10, 0, tree_pos.z),
                required_species: Some(Species::Elf),
            },
        };
        sim.step(&[task_cmd], 4);

        assert_eq!(sim.tasks.len(), 1);
        let task_id = *sim.tasks.keys().next().unwrap();

        // Tick until the elf completes the task.
        sim.step(&[], 1000000);

        let task = &sim.tasks[&task_id];
        assert_eq!(
            task.state,
            TaskState::Complete,
            "Task should be complete after enough ticks"
        );

        // Elf should be unassigned and wandering again.
        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert!(elf.current_task.is_none());
    }

    #[test]
    fn only_one_creature_claims_goto_task() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn multiple elves and capybaras.
        for _ in 0..3 {
            let cmd = SimCommand {
                player_id: sim.player_id,
                tick: sim.tick + 1,
                action: SimAction::SpawnElf {
                    position: tree_pos,
                },
            };
            sim.step(&[cmd], sim.tick + 2);
        }
        for _ in 0..2 {
            let cmd = SimCommand {
                player_id: sim.player_id,
                tick: sim.tick + 1,
                action: SimAction::SpawnCapybara {
                    position: tree_pos,
                },
            };
            sim.step(&[cmd], sim.tick + 2);
        }

        // Create an elf-only GoTo task.
        let far_node = NavNodeId((sim.nav_graph.node_count() - 1) as u32);
        let task_id = insert_goto_task(&mut sim, far_node);

        // Tick enough for all creatures to have several activations.
        sim.step(&[], sim.tick + 50000);

        // Exactly one elf should have claimed it.
        let task = &sim.tasks[&task_id];
        assert_eq!(
            task.assignees.len(),
            1,
            "Exactly one creature should claim the task, got {}",
            task.assignees.len()
        );

        // The assignee must be an elf.
        let assignee = &sim.creatures[&task.assignees[0]];
        assert_eq!(assignee.species, Species::Elf);

        // No capybara should have a task.
        for creature in sim.creatures.values() {
            if creature.species == Species::Capybara {
                assert!(
                    creature.current_task.is_none(),
                    "Capybara should not have claimed an elf-only task"
                );
            }
        }
    }

    #[test]
    fn new_sim_has_initial_fruit() {
        let sim = SimState::new(42);
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.fruit_positions.is_empty(),
            "Tree should have some initial fruit (got 0)"
        );
    }

    #[test]
    fn fruit_hangs_below_leaf_voxels() {
        let sim = SimState::new(42);
        let tree = &sim.trees[&sim.player_tree_id];
        for fruit_pos in &tree.fruit_positions {
            // The leaf above the fruit should be in the tree's leaf_voxels.
            let leaf_above = VoxelCoord::new(fruit_pos.x, fruit_pos.y + 1, fruit_pos.z);
            assert!(
                tree.leaf_voxels.contains(&leaf_above),
                "Fruit at {} should hang below a leaf voxel, but no leaf at {}",
                fruit_pos, leaf_above
            );
        }
    }

    #[test]
    fn fruit_set_in_world_grid() {
        let sim = SimState::new(42);
        let tree = &sim.trees[&sim.player_tree_id];
        for fruit_pos in &tree.fruit_positions {
            assert_eq!(
                sim.world.get(*fruit_pos),
                VoxelType::Fruit,
                "World should have Fruit voxel at {}",
                fruit_pos
            );
        }
    }

    #[test]
    fn fruit_grows_during_heartbeat() {
        // Use a config with no initial fruit but high spawn rate so heartbeats produce fruit.
        let mut config = GameConfig::default();
        config.fruit_initial_attempts = 0;
        config.fruit_production_base_rate = 1.0; // Always spawn
        config.fruit_max_per_tree = 100;
        let mut sim = SimState::with_config(42, config);
        let tree_id = sim.player_tree_id;

        assert!(
            sim.trees[&tree_id].fruit_positions.is_empty(),
            "Should start with no fruit when initial_attempts = 0"
        );

        // Step past several heartbeats (interval = 10000 ticks).
        sim.step(&[], 50000);

        assert!(
            !sim.trees[&tree_id].fruit_positions.is_empty(),
            "Fruit should grow during tree heartbeats"
        );
    }

    #[test]
    fn fruit_respects_max_count() {
        let mut config = GameConfig::default();
        config.fruit_max_per_tree = 3;
        config.fruit_initial_attempts = 100; // Many attempts, but max is 3.
        config.fruit_production_base_rate = 1.0;
        let sim = SimState::with_config(42, config);
        let tree = &sim.trees[&sim.player_tree_id];

        assert!(
            tree.fruit_positions.len() <= 3,
            "Fruit count {} should not exceed max 3",
            tree.fruit_positions.len()
        );
    }

    #[test]
    fn fruit_deterministic() {
        let sim_a = SimState::new(42);
        let sim_b = SimState::new(42);
        let tree_a = &sim_a.trees[&sim_a.player_tree_id];
        let tree_b = &sim_b.trees[&sim_b.player_tree_id];
        assert_eq!(tree_a.fruit_positions, tree_b.fruit_positions);
    }

    // -----------------------------------------------------------------------
    // Save/load roundtrip tests
    // -----------------------------------------------------------------------

    #[test]
    fn rebuild_world_matches_original() {
        let sim = SimState::new(42);
        let tree = &sim.trees[&sim.player_tree_id];

        // Rebuild world from stored tree voxels and config.
        let rebuilt = SimState::rebuild_world(&sim.config, &sim.trees);

        // Check trunk voxels.
        for coord in &tree.trunk_voxels {
            assert_eq!(
                rebuilt.get(*coord),
                VoxelType::Trunk,
                "Rebuilt world missing trunk voxel at {coord}"
            );
        }
        // Check branch voxels.
        for coord in &tree.branch_voxels {
            assert_eq!(
                rebuilt.get(*coord),
                VoxelType::Branch,
                "Rebuilt world missing branch voxel at {coord}"
            );
        }
        // Check root voxels.
        for coord in &tree.root_voxels {
            assert_eq!(
                rebuilt.get(*coord),
                VoxelType::Root,
                "Rebuilt world missing root voxel at {coord}"
            );
        }
        // Check leaf voxels.
        for coord in &tree.leaf_voxels {
            assert_eq!(
                rebuilt.get(*coord),
                VoxelType::Leaf,
                "Rebuilt world missing leaf voxel at {coord}"
            );
        }
        // Check forest floor.
        let (ws_x, _, ws_z) = sim.config.world_size;
        let center_x = ws_x as i32 / 2;
        let center_z = ws_z as i32 / 2;
        let floor_coord = VoxelCoord::new(center_x, 0, center_z);
        assert_eq!(rebuilt.get(floor_coord), VoxelType::ForestFloor);
    }

    #[test]
    fn rebuild_transient_state_restores_nav_graph() {
        let sim = SimState::new(42);
        let json = sim.to_json().unwrap();

        // Deserialize — transient fields are default (empty).
        let mut restored: SimState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.nav_graph.node_count(), 0, "Before rebuild, nav_graph should be empty");
        assert_eq!(restored.world.size_x, 0, "Before rebuild, world should be empty");

        // Rebuild transient state.
        restored.rebuild_transient_state();
        assert!(
            restored.nav_graph.node_count() > 0,
            "After rebuild, nav_graph should have nodes"
        );
        assert_eq!(
            restored.nav_graph.node_count(),
            sim.nav_graph.node_count(),
            "Rebuilt nav_graph should match original node count"
        );
    }

    #[test]
    fn json_roundtrip_preserves_state() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn creatures and advance ticks.
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
        sim.step(&cmds, 200);

        let restored = SimState::from_json(&sim.to_json().unwrap()).unwrap();

        assert_eq!(sim.tick, restored.tick);
        assert_eq!(sim.creatures.len(), restored.creatures.len());
        for (id, creature) in &sim.creatures {
            let restored_creature = &restored.creatures[id];
            assert_eq!(creature.position, restored_creature.position);
            assert_eq!(creature.species, restored_creature.species);
        }
        assert_eq!(sim.player_tree_id, restored.player_tree_id);
        assert_eq!(sim.player_id, restored.player_id);
    }

    #[test]
    fn json_roundtrip_continues_deterministically() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn creatures and advance.
        let spawn = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[spawn], 200);

        // Save and restore.
        let mut restored = SimState::from_json(&sim.to_json().unwrap()).unwrap();

        // Advance both 500 more ticks.
        sim.step(&[], 700);
        restored.step(&[], 700);

        // Creature positions must match.
        for (id, creature) in &sim.creatures {
            let restored_creature = &restored.creatures[id];
            assert_eq!(
                creature.position, restored_creature.position,
                "Creature {id:?} position diverged after roundtrip + 500 ticks"
            );
        }
        // PRNG state must match.
        assert_eq!(sim.rng.next_u64(), restored.rng.next_u64());
    }

    #[test]
    fn from_json_rejects_invalid_json() {
        let result = SimState::from_json("not valid json {{{");
        assert!(result.is_err());
    }

    #[test]
    fn from_json_rejects_wrong_schema() {
        let result = SimState::from_json(r#"{"tick": "not_a_number"}"#);
        assert!(result.is_err());
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

    #[test]
    fn food_decreases_over_heartbeats() {
        let mut sim = SimState::new(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let food_max = sim.species_table[&Species::Elf].food_max;
        let decay_per_tick = sim.species_table[&Species::Elf].food_decay_per_tick;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        // Verify food starts at food_max.
        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.food, food_max);

        // Advance past 3 heartbeats.
        let target_tick = 1 + heartbeat_interval * 3 + 1;
        sim.step(&[], target_tick);

        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        let expected_decay = decay_per_tick * heartbeat_interval as i64 * 3;
        assert_eq!(elf.food, food_max - expected_decay);
    }

    #[test]
    fn food_does_not_go_below_zero() {
        // Use a custom config with aggressive decay so food depletes quickly.
        let mut config = GameConfig::default();
        config.species.get_mut(&Species::Elf).unwrap().food_decay_per_tick =
            1_000_000_000_000_000; // Depletes in 1 tick
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnElf {
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        // Advance well past full depletion (many heartbeats).
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        let target_tick = 1 + heartbeat_interval * 5;
        sim.step(&[], target_tick);

        let elf = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.food, 0);
    }

    // -----------------------------------------------------------------------
    // Blueprint / construction tests
    // -----------------------------------------------------------------------

    /// Find an Air voxel that is face-adjacent to a trunk voxel.
    /// Panics if none found (should never happen with a generated tree).
    fn find_air_adjacent_to_trunk(sim: &SimState) -> VoxelCoord {
        let tree = &sim.trees[&sim.player_tree_id];
        for &trunk_coord in &tree.trunk_voxels {
            for &(dx, dy, dz) in &[
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let neighbor = VoxelCoord::new(
                    trunk_coord.x + dx,
                    trunk_coord.y + dy,
                    trunk_coord.z + dz,
                );
                if sim.world.in_bounds(neighbor)
                    && sim.world.get(neighbor) == VoxelType::Air
                {
                    return neighbor;
                }
            }
        }
        panic!("No air voxel adjacent to trunk found");
    }

    #[test]
    fn designate_build_creates_blueprint() {
        let mut sim = SimState::new(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        let result = sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        assert_eq!(bp.voxels, vec![air_coord]);
        assert_eq!(bp.state, BlueprintState::Designated);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::BlueprintDesignated { .. }
        )));
    }

    #[test]
    fn designate_build_rejects_out_of_bounds() {
        let mut sim = SimState::new(42);
        let oob = VoxelCoord::new(-1, 0, 0);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![oob],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
    }

    #[test]
    fn designate_build_rejects_non_air() {
        let mut sim = SimState::new(42);
        let tree = &sim.trees[&sim.player_tree_id];
        let trunk_coord = tree.trunk_voxels[0];

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![trunk_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
    }

    #[test]
    fn designate_build_rejects_no_adjacency() {
        let mut sim = SimState::new(42);
        // Pick a coord far from any solid geometry.
        let isolated = VoxelCoord::new(0, 50, 0);
        assert_eq!(sim.world.get(isolated), VoxelType::Air);
        assert!(!sim.world.has_solid_face_neighbor(isolated));

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![isolated],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
    }

    #[test]
    fn designate_build_rejects_empty_voxels() {
        let mut sim = SimState::new(42);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
    }

    #[test]
    fn cancel_build_removes_blueprint() {
        let mut sim = SimState::new(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // First designate.
        let cmd1 = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd1], 1);
        assert_eq!(sim.blueprints.len(), 1);
        let project_id = *sim.blueprints.keys().next().unwrap();

        // Now cancel.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        let result = sim.step(&[cmd2], 2);

        assert!(sim.blueprints.is_empty());
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::BuildCancelled { .. }
        )));
    }

    #[test]
    fn cancel_build_nonexistent_is_noop() {
        let mut sim = SimState::new(42);
        let fake_id = ProjectId::new(&mut GameRng::new(999));

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::CancelBuild {
                project_id: fake_id,
            },
        };
        let result = sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
        assert!(!result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::BuildCancelled { .. }
        )));
    }

    #[test]
    fn sim_serialization_with_blueprints() {
        let mut sim = SimState::new(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert_eq!(sim.blueprints.len(), 1);

        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        assert_eq!(restored.blueprints.len(), 1);
        let bp = restored.blueprints.values().next().unwrap();
        assert_eq!(bp.voxels, vec![air_coord]);
        assert_eq!(bp.state, BlueprintState::Designated);
    }

    #[test]
    fn blueprint_determinism() {
        let mut sim_a = SimState::new(42);
        let mut sim_b = SimState::new(42);

        let air_a = find_air_adjacent_to_trunk(&sim_a);
        let air_b = find_air_adjacent_to_trunk(&sim_b);
        assert_eq!(air_a, air_b);

        let cmd_a = SimCommand {
            player_id: sim_a.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_a],
                priority: Priority::Normal,
            },
        };
        let cmd_b = SimCommand {
            player_id: sim_b.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_b],
                priority: Priority::Normal,
            },
        };
        sim_a.step(&[cmd_a], 1);
        sim_b.step(&[cmd_b], 1);

        let id_a = *sim_a.blueprints.keys().next().unwrap();
        let id_b = *sim_b.blueprints.keys().next().unwrap();
        assert_eq!(id_a, id_b);
        assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
    }
}
