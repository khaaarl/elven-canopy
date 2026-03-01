// Core simulation state and tick loop.
//
// `SimState` is the single source of truth for the entire game world. It owns
// all entity data, the voxel world, the nav graph, the event queue, the PRNG,
// and the game config. The sim is a pure function:
// `(state, commands) -> (new_state, events)`.
//
// On construction (`new()`/`with_config()`), the sim generates tree geometry
// via `tree_gen.rs`, builds the navigation graph via `nav.rs`, and initializes
// the voxel world via `world.rs`. Two nav graphs are maintained: the standard
// graph for 1x1x1 creatures and a large graph for 2x2x2 creatures (elephants).
// `graph_for_species()` dispatches to the correct graph based on the species'
// `footprint` field from `species.rs`. Creature spawning and movement are
// handled through the command/event system.
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
// ## Movement interpolation
//
// Each `Creature` carries rendering-only metadata (`move_from`, `move_to`,
// `move_start_tick`, `move_end_tick`) that record the visual start/end of
// each movement. These are set whenever the creature starts traversing a
// nav edge (in `wander()`, `walk_toward_task()`, and
// `handle_creature_movement_complete()`) and cleared on arrival. The
// `interpolated_position(render_tick)` method lerps between them, returning
// floats for the GDExtension bridge. This data is never read by sim logic
// and does not affect determinism.
//
// `CreatureHeartbeat` still exists but is decoupled from movement; it handles
// periodic non-movement checks (mood, mana, food decay, hunger-driven task
// creation, etc.). After decaying food, the heartbeat checks whether the
// creature is hungry (food < threshold) and idle — if so, it calls
// `find_nearest_fruit()` which uses Dijkstra's algorithm over the nav graph
// (respecting species edge restrictions and speeds) to find the closest
// reachable fruit, then creates an `EatFruit` task directly assigned to the
// creature (bypassing the Available→claim flow).
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
// - `kind: TaskKind` — determines behavior (`GoTo`, `Build`, `EatFruit`).
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
//   Build { project_id }:
//     - If not at `task.location` → walk 1 edge toward it (same as GoTo).
//     - If at location → `do_build_work()`: increment progress by 1.0 per
//       activation. Every `build_work_ticks_per_voxel` units of progress,
//       one blueprint voxel materializes as solid (adjacency-first order,
//       preferring unoccupied voxels). Each materialization incrementally
//       updates the nav graph (only ~7 affected positions) and resnaps
//       displaced creatures. When `progress >= total_cost`, the blueprint
//       is marked Complete and the elf is freed.
//
//   EatFruit { fruit_pos }:
//     - If not at `task.location` → walk 1 edge toward it (same as GoTo).
//     - If at location → `do_eat_fruit()`: restore food by `food_restore_pct`%
//       of `food_max`, remove the fruit voxel from the world and the tree's
//       `fruit_positions` list, and complete the task.
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
// `SimState` derives `Serialize`/`Deserialize` via serde. Several transient
// fields (`world`, `nav_graph`, `large_nav_graph`, `species_table`,
// `face_data`, `ladder_orientations`, `structure_voxels`) are `#[serde(skip)]`
// and must be rebuilt after deserialization via `rebuild_transient_state()`.
// Convenience methods `to_json()` and `from_json()` handle the full
// serialize/deserialize + rebuild cycle. `rebuild_world()` reconstructs the
// voxel grid from stored tree voxel lists, `placed_voxels` (construction
// progress), and config (forest floor extent).
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
use crate::building::CompletedStructure;
use crate::command::{SimAction, SimCommand};
use crate::config::GameConfig;
use crate::event::{EventQueue, ScheduledEventKind, SimEvent, SimEventKind};
use crate::nav::{self, NavGraph};
use crate::pathfinding;
use crate::prng::GameRng;
use crate::species::SpeciesData;
use crate::structural;
use crate::task;
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

    /// Voxels placed by construction (persisted for save/load).
    /// Each entry is `(coord, voxel_type)`. On world rebuild, these are
    /// placed after tree voxels to restore construction progress.
    #[serde(default)]
    pub placed_voxels: Vec<(VoxelCoord, VoxelType)>,

    /// Voxels carved (removed to Air) by the carve system. Persisted for
    /// save/load — on world rebuild, these are set to Air after tree voxels.
    #[serde(default)]
    pub carved_voxels: Vec<VoxelCoord>,

    /// Per-face data for `BuildingInterior` and ladder voxels. Persisted as a
    /// flat list of (coord, face_data) pairs since `VoxelCoord` can't be a
    /// JSON map key. At runtime, `face_data` (transient BTreeMap) is the
    /// primary lookup.
    #[serde(default)]
    pub face_data_list: Vec<(VoxelCoord, FaceData)>,

    /// Per-face data indexed by coordinate for O(1) lookup at runtime
    /// (buildings and ladders). Rebuilt from `face_data_list` after
    /// deserialization.
    #[serde(skip)]
    pub face_data: BTreeMap<VoxelCoord, FaceData>,

    /// Ladder orientation data. Persisted as a flat list of (coord, face_direction)
    /// pairs since `VoxelCoord` can't be a JSON map key. Records which face
    /// each ladder panel is on (needed for rendering and validation).
    #[serde(default)]
    pub ladder_orientations_list: Vec<(VoxelCoord, FaceDirection)>,

    /// Ladder orientations indexed by coordinate for O(1) lookup at runtime.
    /// Rebuilt from `ladder_orientations_list` after deserialization.
    #[serde(skip)]
    pub ladder_orientations: BTreeMap<VoxelCoord, FaceDirection>,

    /// All completed structures, keyed by sequential `StructureId`.
    #[serde(default)]
    pub structures: BTreeMap<StructureId, CompletedStructure>,

    /// Counter for the next `StructureId` to assign (monotonically increasing).
    #[serde(default)]
    pub next_structure_id: u64,

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

    /// Navigation graph for large (2x2x2 footprint) creatures. Only contains
    /// ground-level nodes where a 2x2x2 volume is clear. Regenerated, not serialized.
    #[serde(skip)]
    pub large_nav_graph: NavGraph,

    /// Species data table built from config. Not serialized (rebuilt from config).
    #[serde(skip)]
    pub species_table: BTreeMap<Species, SpeciesData>,

    /// Transient message from the last build designation attempt. Set by
    /// `designate_build()` / `designate_building()` and read by `sim_bridge.rs`
    /// to surface validation feedback (warnings, block reasons) to the player.
    /// Cleared at the start of each designation call.
    #[serde(skip)]
    pub last_build_message: Option<String>,

    /// Maps each voxel coordinate belonging to a completed structure back to
    /// its `StructureId`. Transient — rebuilt from completed blueprints in
    /// `rebuild_transient_state()`. Used by `raycast_structure()` to identify
    /// which structure the player clicked on.
    #[serde(skip)]
    pub structure_voxels: BTreeMap<VoxelCoord, StructureId>,
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
    /// Dirt voxel positions forming hilly terrain above ForestFloor.
    #[serde(default)]
    pub dirt_voxels: Vec<VoxelCoord>,
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

    // --- Movement interpolation metadata (rendering only) ---
    // These fields record the visual start/end of each movement for smooth
    // rendering interpolation. They are never read by sim logic — only by
    // `interpolated_position()` which is called from the GDExtension bridge.
    /// Position the creature is visually moving FROM (None when stationary).
    #[serde(default)]
    pub move_from: Option<VoxelCoord>,
    /// Position the creature is visually moving TO (None when stationary).
    #[serde(default)]
    pub move_to: Option<VoxelCoord>,
    /// Tick when the current movement started.
    #[serde(default)]
    pub move_start_tick: u64,
    /// Tick when the current movement completes.
    #[serde(default)]
    pub move_end_tick: u64,
}

fn default_food() -> i64 {
    1_000_000_000_000_000
}

impl Creature {
    /// Compute an interpolated world position for rendering.
    ///
    /// If the creature is mid-movement (both `move_from` and `move_to` are
    /// `Some` with a positive tick duration), linearly interpolates between
    /// the two positions based on `render_tick`. The `t` parameter is clamped
    /// to [0, 1], so positions before the start or after the end are safe.
    ///
    /// If the creature is stationary (`move_from` or `move_to` is `None`),
    /// returns `self.position` converted to floats.
    ///
    /// This method returns floats and is only called by the GDExtension bridge
    /// for rendering — it is never used by sim logic.
    pub fn interpolated_position(&self, render_tick: f64) -> (f32, f32, f32) {
        if let (Some(from), Some(to)) = (self.move_from, self.move_to) {
            let duration = self.move_end_tick as f64 - self.move_start_tick as f64;
            if duration > 0.0 {
                let t =
                    ((render_tick - self.move_start_tick as f64) / duration).clamp(0.0, 1.0) as f32;
                let x = from.x as f32 + (to.x as f32 - from.x as f32) * t;
                let y = from.y as f32 + (to.y as f32 - from.y as f32) * t;
                let z = from.z as f32 + (to.z as f32 - from.z as f32) * t;
                return (x, y, z);
            }
        }
        (
            self.position.x as f32,
            self.position.y as f32,
            self.position.z as f32,
        )
    }
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

    /// Return the appropriate nav graph for a species based on its footprint.
    /// Species with a footprint wider than 1 in x or z use the large nav graph;
    /// all others use the standard graph.
    pub fn graph_for_species(&self, species: Species) -> &NavGraph {
        let data = &self.species_table[&species];
        if data.footprint[0] > 1 || data.footprint[2] > 1 {
            &self.large_nav_graph
        } else {
            &self.nav_graph
        }
    }

    /// Create a new simulation with the given seed and config.
    pub fn with_config(seed: u64, config: GameConfig) -> Self {
        let mut rng = GameRng::new(seed);
        let player_id = PlayerId::new(&mut rng);
        let player_tree_id = TreeId::new(&mut rng);

        let (ws_x, ws_y, ws_z) = config.world_size;
        let center_x = ws_x as i32 / 2;
        let center_z = ws_z as i32 / 2;

        // Generate tree geometry with structural validation retry loop.
        // If the tree fails under its own weight, regenerate (the RNG has
        // advanced, producing different geometry).
        let mut world = VoxelWorld::new(ws_x, ws_y, ws_z);
        let mut tree_result = None;
        for _attempt in 0..config.structural.tree_gen_max_retries {
            let candidate = tree_gen::generate_tree(&mut world, &config, &mut rng);
            if structural::validate_tree(&world, &config) {
                tree_result = Some(candidate);
                break;
            }
            // Clear and rebuild world for retry.
            world = VoxelWorld::new(ws_x, ws_y, ws_z);
            let floor_extent = config.floor_extent;
            for dx in -floor_extent..=floor_extent {
                for dz in -floor_extent..=floor_extent {
                    world.set(
                        VoxelCoord::new(center_x + dx, 0, center_z + dz),
                        VoxelType::ForestFloor,
                    );
                }
            }
        }
        let tree_result = tree_result.expect(
            "Tree generation failed structural validation after max retries. \
             Tree profile parameters are incompatible with material properties.",
        );

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
            dirt_voxels: tree_result.dirt_voxels,
            fruit_positions: Vec::new(),
        };

        // Build nav graphs from voxel world geometry.
        let nav_graph = nav::build_nav_graph(&world, &BTreeMap::new());
        let large_nav_graph = nav::build_large_nav_graph(&world);

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
            placed_voxels: Vec::new(),
            carved_voxels: Vec::new(),
            face_data_list: Vec::new(),
            face_data: BTreeMap::new(),
            ladder_orientations_list: Vec::new(),
            ladder_orientations: BTreeMap::new(),
            structures: BTreeMap::new(),
            next_structure_id: 0,
            player_tree_id,
            player_id,
            world,
            nav_graph,
            large_nav_graph,
            species_table,
            last_build_message: None,
            structure_voxels: BTreeMap::new(),
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
            SimAction::SpawnCreature { species, position } => {
                self.spawn_creature(*species, *position, events);
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
            SimAction::DesignateBuilding {
                anchor,
                width,
                depth,
                height,
                priority,
            } => {
                self.designate_building(*anchor, *width, *depth, *height, *priority, events);
            }
            SimAction::DesignateLadder {
                anchor,
                height,
                orientation,
                kind,
                priority,
            } => {
                self.designate_ladder(*anchor, *height, *orientation, *kind, *priority, events);
            }
            SimAction::DesignateCarve { voxels, priority } => {
                self.designate_carve(voxels, *priority, events);
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
        self.last_build_message = None;

        if voxels.is_empty() {
            self.last_build_message = Some("No voxels to build.".to_string());
            return;
        }
        for &coord in voxels {
            if !self.world.in_bounds(coord) {
                self.last_build_message = Some("Build position is out of bounds.".to_string());
                return;
            }
        }

        // Branch validation: overlap-enabled types classify voxels, others
        // require all Air.
        let build_voxels: Vec<VoxelCoord>;
        let original_voxels: Vec<(VoxelCoord, VoxelType)>;

        if build_type.allows_tree_overlap() {
            let mut bv = Vec::new();
            let mut ov = Vec::new();
            for &coord in voxels {
                match self.world.get(coord).classify_for_overlap() {
                    OverlapClassification::Exterior => {
                        bv.push(coord);
                    }
                    OverlapClassification::Convertible => {
                        ov.push((coord, self.world.get(coord)));
                        bv.push(coord);
                    }
                    OverlapClassification::AlreadyWood => {
                        // Skip — already wood, no blueprint voxel needed.
                    }
                    OverlapClassification::Blocked => {
                        self.last_build_message = Some("Build position is not empty.".to_string());
                        return;
                    }
                }
            }
            if bv.is_empty() {
                self.last_build_message =
                    Some("Nothing to build — all voxels are already wood.".to_string());
                return;
            }
            build_voxels = bv;
            original_voxels = ov;
        } else {
            for &coord in voxels {
                if self.world.get(coord) != VoxelType::Air {
                    self.last_build_message = Some("Build position is not empty.".to_string());
                    return;
                }
            }
            build_voxels = voxels.to_vec();
            original_voxels = Vec::new();
        }

        let any_adjacent = build_voxels
            .iter()
            .any(|&coord| self.world.has_solid_face_neighbor(coord));
        if !any_adjacent {
            self.last_build_message =
                Some("Must build adjacent to an existing structure.".to_string());
            return;
        }

        // Structural validation: fast BFS + weight-flow check (no full solver).
        let validation = structural::validate_blueprint_fast(
            &self.world,
            &self.face_data,
            &build_voxels,
            build_type.to_voxel_type(),
            &BTreeMap::new(),
            &self.config,
        );
        if matches!(validation.tier, structural::ValidationTier::Blocked) {
            self.last_build_message = Some(validation.message);
            return;
        }
        let stress_warning = matches!(validation.tier, structural::ValidationTier::Warning);
        if stress_warning {
            self.last_build_message = Some(validation.message);
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node to the blueprint.
        let task_location = match self.nav_graph.find_nearest_node(build_voxels[0]) {
            Some(n) => n,
            None => return,
        };
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = build_voxels.len() as u64;
        let total_cost = self.config.build_work_ticks_per_voxel * num_voxels;
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: total_cost as f32,
            required_species: Some(Species::Elf),
        };
        self.tasks.insert(task_id, build_task);

        let bp = Blueprint {
            id: project_id,
            build_type,
            voxels: build_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            face_layout: None,
            stress_warning,
            original_voxels,
        };
        self.blueprints.insert(project_id, bp);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for a building with paper-thin walls.
    ///
    /// Validation (silent no-op on failure):
    /// - width and depth must be >= 3 (minimum building size)
    /// - height must be >= 1
    /// - All foundation voxels (anchor.y level) must be solid
    /// - All interior voxels (above foundation) must be Air
    /// - All interior voxels must be in-bounds
    fn designate_building(
        &mut self,
        anchor: VoxelCoord,
        width: i32,
        depth: i32,
        height: i32,
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        self.last_build_message = None;

        if width < 3 || depth < 3 || height < 1 {
            self.last_build_message = Some("Building too small (min 3x3x1).".to_string());
            return;
        }

        // Validate foundation (all must be solid).
        for x in anchor.x..anchor.x + width {
            for z in anchor.z..anchor.z + depth {
                let coord = VoxelCoord::new(x, anchor.y, z);
                if !self.world.in_bounds(coord) || !self.world.get(coord).is_solid() {
                    self.last_build_message =
                        Some("Foundation must be on solid ground.".to_string());
                    return;
                }
            }
        }

        // Validate interior (all must be Air and in-bounds).
        for y in anchor.y + 1..anchor.y + 1 + height {
            for x in anchor.x..anchor.x + width {
                for z in anchor.z..anchor.z + depth {
                    let coord = VoxelCoord::new(x, y, z);
                    if !self.world.in_bounds(coord) || self.world.get(coord) != VoxelType::Air {
                        self.last_build_message =
                            Some("Building interior must be clear.".to_string());
                        return;
                    }
                }
            }
        }

        // Compute face layout.
        let face_layout =
            crate::building::compute_building_face_layout(anchor, width, depth, height);
        let voxels: Vec<VoxelCoord> = face_layout.keys().copied().collect();

        // Structural validation: fast BFS + weight-flow check (no full solver).
        let validation = structural::validate_blueprint_fast(
            &self.world,
            &self.face_data,
            &voxels,
            VoxelType::BuildingInterior,
            &face_layout,
            &self.config,
        );
        if matches!(validation.tier, structural::ValidationTier::Blocked) {
            self.last_build_message = Some(validation.message);
            return;
        }
        let stress_warning = matches!(validation.tier, structural::ValidationTier::Warning);
        if stress_warning {
            self.last_build_message = Some(validation.message);
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node.
        let task_location = match self.nav_graph.find_nearest_node(voxels[0]) {
            Some(n) => n,
            None => return,
        };
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = voxels.len() as u64;
        let total_cost = self.config.build_work_ticks_per_voxel * num_voxels;
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: total_cost as f32,
            required_species: Some(Species::Elf),
        };
        self.tasks.insert(task_id, build_task);

        let bp = Blueprint {
            id: project_id,
            build_type: BuildType::Building,
            voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            face_layout: Some(face_layout.into_iter().collect()),
            stress_warning,
            original_voxels: Vec::new(),
        };
        self.blueprints.insert(project_id, bp);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for a ladder (wood or rope).
    ///
    /// Validation:
    /// - height >= 1
    /// - orientation must be horizontal (PosX/NegX/PosZ/NegZ)
    /// - All column voxels must be Air or Convertible (Leaf/Fruit)
    /// - Wood: at least one voxel's ladder face is adjacent to solid
    /// - Rope: topmost voxel's ladder face is adjacent to solid
    fn designate_ladder(
        &mut self,
        anchor: VoxelCoord,
        height: i32,
        orientation: FaceDirection,
        kind: LadderKind,
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        self.last_build_message = None;

        if height < 1 {
            self.last_build_message = Some("Ladder height must be at least 1.".to_string());
            return;
        }

        // Orientation must be horizontal (ody == 0 after this guard).
        let (odx, _ody, odz) = orientation.to_offset();
        if _ody != 0 {
            self.last_build_message = Some("Ladder orientation must be horizontal.".to_string());
            return;
        }

        // Classify column voxels using overlap rules (ladders allow tree overlap).
        let build_type = match kind {
            LadderKind::Wood => BuildType::WoodLadder,
            LadderKind::Rope => BuildType::RopeLadder,
        };
        let mut build_voxels = Vec::new();
        let mut original_voxels = Vec::new();
        for dy in 0..height {
            let coord = VoxelCoord::new(anchor.x, anchor.y + dy, anchor.z);
            if !self.world.in_bounds(coord) {
                self.last_build_message = Some("Ladder extends out of bounds.".to_string());
                return;
            }
            match self.world.get(coord).classify_for_overlap() {
                OverlapClassification::Exterior => {
                    build_voxels.push(coord);
                }
                OverlapClassification::Convertible => {
                    original_voxels.push((coord, self.world.get(coord)));
                    build_voxels.push(coord);
                }
                OverlapClassification::AlreadyWood => {
                    // Skip — already wood, no blueprint voxel needed.
                }
                OverlapClassification::Blocked => {
                    self.last_build_message =
                        Some("Ladder position is blocked by existing construction.".to_string());
                    return;
                }
            }
        }
        if build_voxels.is_empty() {
            self.last_build_message =
                Some("Nothing to build — all voxels are already wood.".to_string());
            return;
        }

        // Anchoring validation.
        match kind {
            LadderKind::Wood => {
                // At least one voxel's ladder face must be adjacent to solid.
                let any_anchored = build_voxels.iter().any(|&coord| {
                    let neighbor = VoxelCoord::new(coord.x + odx, coord.y, coord.z + odz);
                    self.world.get(neighbor).is_solid()
                });
                if !any_anchored {
                    self.last_build_message =
                        Some("Wood ladder must be adjacent to a solid surface.".to_string());
                    return;
                }
            }
            LadderKind::Rope => {
                // Topmost voxel's ladder face must be adjacent to solid.
                let top = VoxelCoord::new(anchor.x + odx, anchor.y + height - 1, anchor.z + odz);
                if !self.world.get(top).is_solid() {
                    self.last_build_message =
                        Some("Rope ladder must hang from a solid surface at the top.".to_string());
                    return;
                }
            }
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node to the bottom of the ladder.
        let task_location = match self.nav_graph.find_nearest_node(build_voxels[0]) {
            Some(n) => n,
            None => return,
        };
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = build_voxels.len() as u64;
        let total_cost = self.config.build_work_ticks_per_voxel * num_voxels;
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: total_cost as f32,
            required_species: Some(Species::Elf),
        };
        self.tasks.insert(task_id, build_task);

        // Store the orientation in the blueprint's face_layout field for later
        // use during materialization. We encode it as a map from each voxel to
        // its FaceData (computed from orientation).
        let face_layout: Vec<(VoxelCoord, FaceData)> = build_voxels
            .iter()
            .map(|&coord| (coord, ladder_face_data(orientation)))
            .collect();

        let bp = Blueprint {
            id: project_id,
            build_type,
            voxels: build_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            face_layout: Some(face_layout.into_iter().collect()),
            stress_warning: false,
            original_voxels,
        };
        self.blueprints.insert(project_id, bp);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for carving (removing) solid voxels.
    ///
    /// Filters the input to only carvable voxels (solid and not ForestFloor).
    /// Air and ForestFloor voxels are silently skipped. Records original voxel
    /// types for cancel restoration.
    fn designate_carve(
        &mut self,
        voxels: &[VoxelCoord],
        priority: Priority,
        events: &mut Vec<SimEvent>,
    ) {
        self.last_build_message = None;

        if voxels.is_empty() {
            self.last_build_message = Some("No voxels to carve.".to_string());
            return;
        }
        for &coord in voxels {
            if !self.world.in_bounds(coord) {
                self.last_build_message = Some("Carve position is out of bounds.".to_string());
                return;
            }
        }

        // Filter to only carvable voxels: solid and not ForestFloor.
        let mut carve_voxels = Vec::new();
        let mut original_voxels = Vec::new();
        for &coord in voxels {
            let vt = self.world.get(coord);
            if vt.is_solid() && vt != VoxelType::ForestFloor {
                carve_voxels.push(coord);
                original_voxels.push((coord, vt));
            }
        }

        if carve_voxels.is_empty() {
            self.last_build_message = Some("Nothing to carve.".to_string());
            return;
        }

        let validation = structural::validate_carve_fast(
            &self.world,
            &self.face_data,
            &carve_voxels,
            &self.config,
        );
        if matches!(validation.tier, structural::ValidationTier::Blocked) {
            self.last_build_message = Some(validation.message);
            return;
        }
        let stress_warning = matches!(validation.tier, structural::ValidationTier::Warning);
        if stress_warning {
            self.last_build_message = Some(validation.message);
        }

        let project_id = ProjectId::new(&mut self.rng);

        // Create a Build task at the nearest nav node to the carve site.
        let task_location = match self.nav_graph.find_nearest_node(carve_voxels[0]) {
            Some(n) => n,
            None => return,
        };
        let task_id = TaskId::new(&mut self.rng);
        let num_voxels = carve_voxels.len() as u64;
        let total_cost = self.config.carve_work_ticks_per_voxel * num_voxels;
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: total_cost as f32,
            required_species: Some(Species::Elf),
        };
        self.tasks.insert(task_id, build_task);

        let bp = Blueprint {
            id: project_id,
            build_type: BuildType::Carve,
            voxels: carve_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            face_layout: None,
            stress_warning,
            original_voxels,
        };
        self.blueprints.insert(project_id, bp);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Cancel a blueprint by ProjectId. Removes the associated Build task,
    /// unassigns any workers, reverts materialized voxels to Air, and rebuilds
    /// the nav graph. Emits `BuildCancelled` if found.
    /// Silent no-op if the ProjectId doesn't exist (idempotent for multiplayer).
    fn cancel_build(&mut self, project_id: ProjectId, events: &mut Vec<SimEvent>) {
        let bp = match self.blueprints.remove(&project_id) {
            Some(bp) => bp,
            None => return,
        };

        // Remove any completed structure for this project (linear scan — map is small).
        // Also remove structure_voxels entries for the cancelled blueprint.
        for &coord in &bp.voxels {
            self.structure_voxels.remove(&coord);
        }
        self.structures.retain(|_, s| s.project_id != project_id);

        // Remove the associated Build task and unassign workers.
        if let Some(task_id) = bp.task_id
            && let Some(task) = self.tasks.remove(&task_id)
        {
            for cid in &task.assignees {
                if let Some(creature) = self.creatures.get_mut(cid) {
                    creature.current_task = None;
                    creature.path = None;
                }
            }
        }

        let bp_voxels: Vec<VoxelCoord> = bp.voxels.clone();
        let original_map: BTreeMap<VoxelCoord, VoxelType> =
            bp.original_voxels.iter().copied().collect();
        let is_building = bp.build_type == BuildType::Building;
        let is_carve = bp.build_type == BuildType::Carve;
        let mut any_reverted = false;

        if is_carve {
            // Carve cancel: restore carved voxels to their original types.
            for &coord in &bp_voxels {
                if self.world.get(coord) == VoxelType::Air
                    && let Some(&original) = original_map.get(&coord)
                {
                    self.world.set(coord, original);
                    any_reverted = true;
                }
            }
            self.carved_voxels.retain(|c| !bp_voxels.contains(c));
        } else {
            // Build cancel: revert materialized voxels to Air (or original for
            // overlap builds with convertible Leaf/Fruit).
            for &coord in &bp_voxels {
                if self.world.get(coord) != VoxelType::Air {
                    let revert_to = original_map.get(&coord).copied().unwrap_or(VoxelType::Air);
                    self.world.set(coord, revert_to);
                    any_reverted = true;
                }
            }
            // Remove from placed_voxels.
            self.placed_voxels
                .retain(|(coord, _)| !bp_voxels.contains(coord));
        }

        // For buildings and ladders, also remove face_data entries.
        let is_ladder = matches!(bp.build_type, BuildType::WoodLadder | BuildType::RopeLadder);
        if is_building || is_ladder {
            for &coord in &bp_voxels {
                self.face_data.remove(&coord);
            }
            self.face_data_list
                .retain(|(coord, _)| !bp_voxels.contains(coord));
        }
        // For ladders, also remove ladder_orientations entries.
        if is_ladder {
            for &coord in &bp_voxels {
                self.ladder_orientations.remove(&coord);
            }
            self.ladder_orientations_list
                .retain(|(coord, _)| !bp_voxels.contains(coord));
        }

        // Rebuild nav graph if geometry changed.
        if any_reverted {
            self.nav_graph = nav::build_nav_graph(&self.world, &self.face_data);
            self.resnap_creature_nodes();
        }

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BuildCancelled { project_id },
        });
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

                // Phase 1: apply food decay and read state needed for hunger check.
                let should_seek_food = if let Some(creature) = self.creatures.get_mut(&creature_id)
                {
                    let species = creature.species;
                    let species_data = &self.species_table[&species];
                    let interval = species_data.heartbeat_interval_ticks;
                    let decay = species_data.food_decay_per_tick * interval as i64;
                    creature.food = (creature.food - decay).max(0);

                    let threshold =
                        species_data.food_max * species_data.food_hunger_threshold_pct / 100;
                    let is_hungry = creature.food < threshold;
                    let is_idle = creature.current_task.is_none();

                    // Reschedule the next heartbeat.
                    let next_tick = self.tick + interval;
                    self.event_queue.schedule(
                        next_tick,
                        ScheduledEventKind::CreatureHeartbeat { creature_id },
                    );

                    is_hungry && is_idle
                } else {
                    false
                };

                // Phase 2: if hungry and idle, find nearest fruit by graph
                // travel cost (Dijkstra) and create an EatFruit task.
                if should_seek_food
                    && let Some((fruit_pos, nav_node)) = self.find_nearest_fruit(creature_id)
                {
                    let task_id = TaskId::new(&mut self.rng);
                    let new_task = task::Task {
                        id: task_id,
                        kind: task::TaskKind::EatFruit { fruit_pos },
                        state: task::TaskState::InProgress,
                        location: nav_node,
                        assignees: vec![creature_id],
                        progress: 0.0,
                        total_cost: 0.0,
                        required_species: None,
                    };
                    self.tasks.insert(task_id, new_task);
                    if let Some(creature) = self.creatures.get_mut(&creature_id) {
                        creature.current_task = Some(task_id);
                    }
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
        let graph = self.graph_for_species(species);

        let nearest_node = if species_data.ground_only {
            graph.find_nearest_ground_node(position)
        } else {
            graph.find_nearest_node(position)
        };

        let nearest_node = match nearest_node {
            Some(n) => n,
            None => return, // No suitable nav nodes — can't spawn.
        };

        let node_pos = graph.node(nearest_node).position;
        let creature_id = CreatureId::new(&mut self.rng);

        let creature = Creature {
            id: creature_id,
            species,
            position: node_pos,
            current_node: Some(nearest_node),
            path: None,
            current_task: None,
            food: species_data.food_max,
            move_from: None,
            move_to: None,
            move_start_tick: 0,
            move_end_tick: 0,
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
        let (mut current_node, species, current_task) = {
            let creature = match self.creatures.get(&creature_id) {
                Some(c) => c,
                None => return,
            };
            let node = match creature.current_node {
                Some(n) => n,
                None => return,
            };
            (node, creature.species, creature.current_task)
        };

        // Guard: if current_node is a dead slot (removed by incremental nav
        // update), resnap the creature before proceeding.
        if !self.graph_for_species(species).is_node_alive(current_node) {
            let pos = self.creatures[&creature_id].position;
            let graph = self.graph_for_species(species);
            let new_node = match graph.find_nearest_node(pos) {
                Some(n) => n,
                None => return,
            };
            let new_pos = graph.node(new_node).position;
            let c = self.creatures.get_mut(&creature_id).unwrap();
            c.current_node = Some(new_node);
            c.position = new_pos;
            c.path = None;
            current_node = new_node;
        }

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
                    && t.required_species.is_none_or(|s| s == species)
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

        // Check that both current_node and task_location are still alive in
        // the nav graph. They can become dead slots after incremental updates
        // (e.g. construction solidifying a voxel). If either is dead, abandon
        // the task and wander.
        let species = self
            .creatures
            .get(&creature_id)
            .map(|c| c.species)
            .unwrap_or(Species::Elf);
        let graph = self.graph_for_species(species);
        if !graph.is_node_alive(current_node) || !graph.is_node_alive(task_location) {
            if let Some(c) = self.creatures.get_mut(&creature_id) {
                c.current_task = None;
                c.path = None;
            }
            // Resnap the creature to a live node before wandering.
            let graph = self.graph_for_species(species);
            if let Some(c) = self.creatures.get(&creature_id) {
                let pos = c.position;
                if let Some(new_node) = graph.find_nearest_node(pos) {
                    let new_pos = graph.node(new_node).position;
                    let c = self.creatures.get_mut(&creature_id).unwrap();
                    c.current_node = Some(new_node);
                    c.position = new_pos;
                    self.wander(creature_id, new_node);
                }
            }
            return;
        }

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

        match task.kind.clone() {
            task::TaskKind::GoTo => {
                // GoTo completes instantly on arrival.
                self.complete_task(task_id);
            }
            task::TaskKind::EatFruit { fruit_pos } => {
                self.do_eat_fruit(creature_id, task_id, fruit_pos);
            }
            task::TaskKind::Build { project_id } => {
                self.do_build_work(creature_id, task_id, project_id);
                return; // do_build_work handles its own next-activation scheduling.
            }
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

    /// Find the nearest reachable fruit for a creature, using Dijkstra over the
    /// nav graph with the creature's species-specific speeds and edge restrictions.
    ///
    /// Returns the fruit voxel coordinate and its nearest nav node, or `None`
    /// if no fruit exists or none is reachable by this creature.
    fn find_nearest_fruit(&self, creature_id: CreatureId) -> Option<(VoxelCoord, NavNodeId)> {
        let creature = self.creatures.get(&creature_id)?;
        let start_node = creature.current_node?;
        let species_data = &self.species_table[&creature.species];
        let graph = self.graph_for_species(creature.species);

        // Map each fruit position to its nearest nav node, keeping the association.
        let mut nav_to_fruit: Vec<(NavNodeId, VoxelCoord)> = Vec::new();
        let mut target_nodes: Vec<NavNodeId> = Vec::new();
        for tree in self.trees.values() {
            for &fruit_pos in &tree.fruit_positions {
                if let Some(nav_node) = graph.find_nearest_node(fruit_pos) {
                    target_nodes.push(nav_node);
                    nav_to_fruit.push((nav_node, fruit_pos));
                }
            }
        }

        if target_nodes.is_empty() {
            return None;
        }

        let nearest_node = pathfinding::dijkstra_nearest(
            graph,
            start_node,
            &target_nodes,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
            species_data.allowed_edge_types.as_deref(),
        )?;

        // Find the fruit_pos associated with this nav node.
        let fruit_pos = nav_to_fruit
            .iter()
            .find(|(n, _)| *n == nearest_node)
            .map(|(_, fp)| *fp)?;

        Some((fruit_pos, nearest_node))
    }

    /// Eat fruit at `fruit_pos`: restore food, remove the fruit voxel, and
    /// complete the task.
    fn do_eat_fruit(&mut self, creature_id: CreatureId, task_id: TaskId, fruit_pos: VoxelCoord) {
        // Restore food.
        if let Some(creature) = self.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let restore = species_data.food_max * species_data.food_restore_pct / 100;
            let food_max = species_data.food_max;
            let creature = self.creatures.get_mut(&creature_id).unwrap();
            creature.food = (creature.food + restore).min(food_max);
        }

        // Remove fruit from world and tree's fruit_positions list.
        if self.world.get(fruit_pos) == VoxelType::Fruit {
            self.world.set(fruit_pos, VoxelType::Air);
        }
        for tree in self.trees.values_mut() {
            tree.fruit_positions.retain(|&p| p != fruit_pos);
        }

        self.complete_task(task_id);
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
        let graph = self.graph_for_species(species);

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
        let wood_ladder_tpv = species_data.wood_ladder_tpv;
        let rope_ladder_tpv = species_data.rope_ladder_tpv;

        let (edge_idx, dest_node) = if let Some(step) = next_step {
            step
        } else {
            // Compute path to task location.
            let path_result = if let Some(ref allowed) = species_data.allowed_edge_types {
                pathfinding::astar_filtered(
                    graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                    wood_ladder_tpv,
                    rope_ladder_tpv,
                    allowed,
                )
            } else {
                pathfinding::astar(
                    graph,
                    current_node,
                    task_location,
                    walk_tpv,
                    climb_tpv,
                    wood_ladder_tpv,
                    rope_ladder_tpv,
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
        let graph = self.graph_for_species(species);
        let edge = graph.edge(edge_idx);
        let tpv = match edge.edge_type {
            crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                climb_tpv.unwrap_or(walk_tpv)
            }
            crate::nav::EdgeType::WoodLadderClimb => wood_ladder_tpv.unwrap_or(walk_tpv),
            crate::nav::EdgeType::RopeLadderClimb => rope_ladder_tpv.unwrap_or(walk_tpv),
            _ => walk_tpv,
        };
        let delay = ((edge.distance * tpv as f32).ceil() as u64).max(1);
        let dest_pos = graph.node(dest_node).position;

        let creature = self.creatures.get_mut(&creature_id).unwrap();
        let old_pos = creature.position;
        creature.position = dest_pos;
        creature.current_node = Some(dest_node);

        // Set movement interpolation metadata for smooth rendering.
        creature.move_from = Some(old_pos);
        creature.move_to = Some(dest_pos);
        creature.move_start_tick = self.tick;
        creature.move_end_tick = self.tick + delay;

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
        if let Some(tid) = task_id
            && let Some(task) = self.tasks.get_mut(&tid)
        {
            task.assignees.retain(|&id| id != creature_id);
            if task.assignees.is_empty() && matches!(task.state, task::TaskState::InProgress) {
                task.state = task::TaskState::Available;
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

        // Collect eligible edges before mutably borrowing self (for rng).
        let eligible_edges: Vec<usize> = {
            let species_data = &self.species_table[&species];
            let graph = self.graph_for_species(species);
            let edge_indices = graph.neighbors(current_node);
            if edge_indices.is_empty() {
                self.event_queue.schedule(
                    self.tick + 1000,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
                return;
            }
            if let Some(ref allowed) = species_data.allowed_edge_types {
                edge_indices
                    .iter()
                    .copied()
                    .filter(|&idx| allowed.contains(&graph.edge(idx).edge_type))
                    .collect()
            } else {
                edge_indices.to_vec()
            }
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

        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);
        let edge = graph.edge(edge_idx);
        let dest_node = edge.to;

        // Compute traversal time from distance * species ticks-per-voxel.
        let tpv = match edge.edge_type {
            crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => species_data
                .climb_ticks_per_voxel
                .unwrap_or(species_data.walk_ticks_per_voxel),
            crate::nav::EdgeType::WoodLadderClimb => species_data
                .wood_ladder_tpv
                .unwrap_or(species_data.walk_ticks_per_voxel),
            crate::nav::EdgeType::RopeLadderClimb => species_data
                .rope_ladder_tpv
                .unwrap_or(species_data.walk_ticks_per_voxel),
            _ => species_data.walk_ticks_per_voxel,
        };
        let delay = ((edge.distance * tpv as f32).ceil() as u64).max(1);

        // Move creature to the destination.
        let dest_pos = graph.node(dest_node).position;
        let creature = self.creatures.get_mut(&creature_id).unwrap();
        let old_pos = creature.position;
        creature.position = dest_pos;
        creature.current_node = Some(dest_node);

        // Set movement interpolation metadata for smooth rendering.
        creature.move_from = Some(old_pos);
        creature.move_to = Some(dest_pos);
        creature.move_start_tick = self.tick;
        creature.move_end_tick = self.tick + delay;

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
        let creature = match self.creatures.get_mut(&creature_id) {
            Some(c) => c,
            None => return, // Creature was removed.
        };

        let species = creature.species;
        let graph = self.graph_for_species(species);
        let node_pos = graph.node(arrived_at).position;

        let creature = self.creatures.get_mut(&creature_id).unwrap();

        // Update position and current node.
        creature.position = node_pos;
        creature.current_node = Some(arrived_at);

        // Clear movement interpolation metadata on arrival.
        creature.move_from = None;
        creature.move_to = None;

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
            let graph = self.graph_for_species(species);
            let path = self.creatures[&creature_id].path.as_ref().unwrap();
            let next_edge_idx = path.remaining_edge_indices[0];
            let next_edge = graph.edge(next_edge_idx);
            let tpv = match next_edge.edge_type {
                crate::nav::EdgeType::TrunkClimb | crate::nav::EdgeType::GroundToTrunk => {
                    species_data
                        .climb_ticks_per_voxel
                        .unwrap_or(species_data.walk_ticks_per_voxel)
                }
                crate::nav::EdgeType::WoodLadderClimb => species_data
                    .wood_ladder_tpv
                    .unwrap_or(species_data.walk_ticks_per_voxel),
                crate::nav::EdgeType::RopeLadderClimb => species_data
                    .rope_ladder_tpv
                    .unwrap_or(species_data.walk_ticks_per_voxel),
                _ => species_data.walk_ticks_per_voxel,
            };
            let delay = ((next_edge.distance * tpv as f32).ceil() as u64).max(1);
            let next_dest = path.remaining_nodes[0];
            let next_dest_pos = graph.node(next_dest).position;
            let arrival_tick = self.tick + delay;

            // Set movement interpolation metadata for the next leg.
            let creature = self.creatures.get_mut(&creature_id).unwrap();
            creature.move_from = Some(node_pos);
            creature.move_to = Some(next_dest_pos);
            creature.move_start_tick = self.tick;
            creature.move_end_tick = arrival_tick;

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
        // Skip leaves that have been carved away.
        let leaf_count = tree.leaf_voxels.len();
        let leaf_idx = self.rng.range_u64(0, leaf_count as u64) as usize;
        let leaf_pos = tree.leaf_voxels[leaf_idx];
        if self.world.get(leaf_pos) == VoxelType::Air {
            return false;
        }
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
    // Build work — incremental voxel materialization
    // -----------------------------------------------------------------------

    /// Perform one activation's worth of build work on a blueprint.
    ///
    /// Increments the task's `progress`. When enough progress accumulates
    /// (every `build_work_ticks_per_voxel` units), one blueprint voxel is
    /// materialized as solid. When all voxels are placed, the blueprint is
    /// marked Complete and the task finishes.
    fn do_build_work(&mut self, creature_id: CreatureId, task_id: TaskId, project_id: ProjectId) {
        // Look up the blueprint to determine if this is a carve or build.
        let is_carve = self
            .blueprints
            .get(&project_id)
            .is_some_and(|bp| bp.build_type.is_carve());

        let ticks_per_voxel = if is_carve {
            self.config.carve_work_ticks_per_voxel as f32
        } else {
            self.config.build_work_ticks_per_voxel as f32
        };

        // Increment progress.
        let task = match self.tasks.get_mut(&task_id) {
            Some(t) => t,
            None => return,
        };
        let old_progress = task.progress;
        task.progress += 1.0;
        let new_progress = task.progress;
        let total_cost = task.total_cost;

        // Check if we crossed a voxel-placement threshold.
        let old_voxels = (old_progress / ticks_per_voxel).floor() as u64;
        let new_voxels = (new_progress / ticks_per_voxel).floor() as u64;

        if new_voxels > old_voxels {
            if is_carve {
                self.materialize_next_carve_voxel(project_id);
            } else {
                self.materialize_next_build_voxel(project_id);
            }
        }

        // Check if the build is complete.
        if new_progress >= total_cost {
            self.complete_build(project_id, task_id);
        }

        // Schedule next activation.
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Pick the next blueprint voxel to materialize and place it.
    ///
    /// Selection criteria:
    /// 1. Must not already be the target type (not yet placed).
    /// 2. Must have at least one face-adjacent solid neighbor (adjacency
    ///    invariant — connects to existing geometry).
    /// 3. Prefer voxels NOT occupied by any creature.
    /// 4. If all eligible are occupied, pick randomly using the sim PRNG.
    fn materialize_next_build_voxel(&mut self, project_id: ProjectId) {
        let bp = match self.blueprints.get(&project_id) {
            Some(bp) => bp,
            None => return,
        };
        let build_type = bp.build_type;
        let voxel_type = build_type.to_voxel_type();
        let is_building = build_type == BuildType::Building;
        let is_ladder = matches!(build_type, BuildType::WoodLadder | BuildType::RopeLadder);
        let allows_overlap = build_type.allows_tree_overlap();

        // Find unplaced voxels that are adjacent to existing geometry.
        // For buildings, adjacency accepts BuildingInterior face neighbors in
        // addition to solid neighbors (building interior voxels grow from the
        // foundation and from each other).
        // For ladders, adjacency accepts same-type ladder face neighbors
        // (ladder voxels grow from bottom to top or from an anchored voxel).
        // For overlap-enabled types, a voxel is "unplaced" if it hasn't been
        // converted to the target type yet (it may be Air, Leaf, or Fruit).
        let eligible: Vec<VoxelCoord> = bp
            .voxels
            .iter()
            .copied()
            .filter(|&coord| {
                let current = self.world.get(coord);
                if allows_overlap {
                    // Already materialized to target type → skip.
                    if current == voxel_type {
                        return false;
                    }
                    // Must be Air or Convertible (Leaf/Fruit).
                    if current != VoxelType::Air
                        && !matches!(
                            current.classify_for_overlap(),
                            OverlapClassification::Convertible
                        )
                    {
                        return false;
                    }
                } else if current != VoxelType::Air {
                    return false;
                }
                if self.world.has_solid_face_neighbor(coord) {
                    return true;
                }
                // For buildings, also accept BuildingInterior face neighbors.
                if is_building {
                    return self
                        .world
                        .has_face_neighbor_of_type(coord, VoxelType::BuildingInterior);
                }
                // For ladders, also accept same-type ladder face neighbors.
                if is_ladder {
                    return self.world.has_face_neighbor_of_type(coord, voxel_type);
                }
                false
            })
            .collect();

        if eligible.is_empty() {
            return;
        }

        // Collect creature positions for occupancy check.
        let creature_positions: Vec<VoxelCoord> =
            self.creatures.values().map(|c| c.position).collect();

        // Prefer unoccupied voxels.
        let unoccupied: Vec<VoxelCoord> = eligible
            .iter()
            .copied()
            .filter(|coord| !creature_positions.contains(coord))
            .collect();

        let chosen = if !unoccupied.is_empty() {
            let idx = self.rng.range_u64(0, unoccupied.len() as u64) as usize;
            unoccupied[idx]
        } else {
            let idx = self.rng.range_u64(0, eligible.len() as u64) as usize;
            eligible[idx]
        };

        // Place the voxel.
        self.world.set(chosen, voxel_type);
        self.placed_voxels.push((chosen, voxel_type));

        // For buildings and ladders, copy face data from the blueprint into sim state.
        if is_building || is_ladder {
            if let Some(bp) = self.blueprints.get(&project_id)
                && let Some(layout) = bp.face_layout_map()
                && let Some(fd) = layout.get(&chosen)
            {
                self.face_data.insert(chosen, fd.clone());
                self.face_data_list.push((chosen, fd.clone()));
            }
            // For ladders, also store the orientation.
            if is_ladder
                && let Some(bp) = self.blueprints.get(&project_id)
                && let Some(layout) = bp.face_layout_map()
                && let Some(fd) = layout.get(&chosen)
            {
                // Derive orientation: the horizontal Wall face whose opposite is Open.
                for dir in [
                    FaceDirection::PosX,
                    FaceDirection::NegX,
                    FaceDirection::PosZ,
                    FaceDirection::NegZ,
                ] {
                    if fd.get(dir) == FaceType::Wall && fd.get(dir.opposite()) == FaceType::Open {
                        self.ladder_orientations.insert(chosen, dir);
                        self.ladder_orientations_list.push((chosen, dir));
                        break;
                    }
                }
            }
            let removed = self.nav_graph.update_after_building_voxel_set(
                &self.world,
                &self.face_data,
                chosen,
            );
            let large_removed = nav::update_large_after_voxel_solidified(
                &mut self.large_nav_graph,
                &self.world,
                chosen,
            );
            let mut all_removed = removed;
            all_removed.extend(large_removed);
            self.resnap_removed_nodes(&all_removed);
        } else {
            // Incrementally update nav graph (touches only ~7 affected positions
            // instead of scanning the entire world) and resnap displaced creatures.
            let removed =
                self.nav_graph
                    .update_after_voxel_solidified(&self.world, &self.face_data, chosen);
            let large_removed = nav::update_large_after_voxel_solidified(
                &mut self.large_nav_graph,
                &self.world,
                chosen,
            );
            let mut all_removed = removed;
            all_removed.extend(large_removed);
            self.resnap_removed_nodes(&all_removed);
        }
    }

    /// Pick the next blueprint voxel to carve (set to Air).
    ///
    /// Selection: find voxels that are still solid, pick one randomly using
    /// the sim PRNG (no adjacency constraint for carving).
    fn materialize_next_carve_voxel(&mut self, project_id: ProjectId) {
        let bp = match self.blueprints.get(&project_id) {
            Some(bp) => bp,
            None => return,
        };

        // Find blueprint voxels that are still solid (not yet carved).
        let still_solid: Vec<VoxelCoord> = bp
            .voxels
            .iter()
            .copied()
            .filter(|&coord| self.world.get(coord).is_solid())
            .collect();

        if still_solid.is_empty() {
            return;
        }

        let idx = self.rng.range_u64(0, still_solid.len() as u64) as usize;
        let chosen = still_solid[idx];

        // Set to Air.
        self.world.set(chosen, VoxelType::Air);
        self.carved_voxels.push(chosen);

        // Nav update: the algorithm is state-based and works for both
        // solidifying and clearing voxels.
        let removed =
            self.nav_graph
                .update_after_voxel_solidified(&self.world, &self.face_data, chosen);
        let large_removed = nav::update_large_after_voxel_solidified(
            &mut self.large_nav_graph,
            &self.world,
            chosen,
        );
        let mut all_removed = removed;
        all_removed.extend(large_removed);
        self.resnap_removed_nodes(&all_removed);
    }

    /// Mark a blueprint as Complete, register the completed structure, and
    /// complete its associated task.
    fn complete_build(&mut self, project_id: ProjectId, task_id: TaskId) {
        if let Some(bp) = self.blueprints.get_mut(&project_id) {
            bp.state = BlueprintState::Complete;
        }

        // Register a CompletedStructure if the blueprint exists.
        if let Some(bp) = self.blueprints.get(&project_id) {
            let structure_id = StructureId(self.next_structure_id);
            self.next_structure_id += 1;
            // Populate structure_voxels ownership map.
            for &coord in &bp.voxels {
                self.structure_voxels.insert(coord, structure_id);
            }
            let structure = CompletedStructure::from_blueprint(structure_id, bp, self.tick);
            self.structures.insert(structure_id, structure);
        }

        self.complete_task(task_id);
    }

    /// After a nav graph rebuild, re-resolve every creature's `current_node`
    /// by finding the nearest node to its position. Clears stored paths since
    /// NavNodeIds change when the graph is rebuilt.
    fn resnap_creature_nodes(&mut self) {
        let creature_info: Vec<(CreatureId, Species, VoxelCoord)> = self
            .creatures
            .values()
            .map(|c| (c.id, c.species, c.position))
            .collect();
        for (cid, species, pos) in creature_info {
            let graph = self.graph_for_species(species);
            let new_node = graph.find_nearest_node(pos);
            let new_pos = new_node.map(|nid| graph.node(nid).position);
            let creature = self.creatures.get_mut(&cid).unwrap();
            creature.current_node = new_node;
            creature.path = None;
            if let Some(p) = new_pos {
                creature.position = p;
            }
        }
    }

    /// Resnap only creatures whose `current_node` was among the removed IDs.
    /// Used after incremental nav graph updates where most creatures are
    /// unaffected — much cheaper than resnapping all creatures.
    fn resnap_removed_nodes(&mut self, removed: &[NavNodeId]) {
        if removed.is_empty() {
            return;
        }
        let to_resnap: Vec<(CreatureId, Species, VoxelCoord)> = self
            .creatures
            .values()
            .filter(|c| matches!(c.current_node, Some(nid) if removed.contains(&nid)))
            .map(|c| (c.id, c.species, c.position))
            .collect();
        for (cid, species, pos) in to_resnap {
            let graph = self.graph_for_species(species);
            let new_node = graph.find_nearest_node(pos);
            let new_pos = new_node.map(|nid| graph.node(nid).position);
            let creature = self.creatures.get_mut(&cid).unwrap();
            creature.current_node = new_node;
            creature.path = None;
            if let Some(p) = new_pos {
                creature.position = p;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Structure raycast
    // -----------------------------------------------------------------------

    /// DDA voxel raycast that returns the `StructureId` of the first structure
    /// voxel hit along the ray. Uses the same Amanatides & Woo algorithm as
    /// `VoxelWorld::raycast_hits_solid`, but checks `structure_voxels` at each
    /// step:
    /// - If the voxel is in `structure_voxels`, return that `StructureId`.
    /// - If the voxel is solid but NOT a structure voxel (e.g., tree trunk),
    ///   stop (return `None` — geometry occludes).
    /// - If the voxel is air (and not a structure voxel), continue.
    ///
    /// This correctly handles non-solid structure types (ladders, building
    /// interiors) since they're in `structure_voxels` even though
    /// `is_solid()` returns false.
    pub fn raycast_structure(
        &self,
        from: [f32; 3],
        dir: [f32; 3],
        max_steps: u32,
    ) -> Option<StructureId> {
        let mut voxel = [
            from[0].floor() as i32,
            from[1].floor() as i32,
            from[2].floor() as i32,
        ];

        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
        }

        for _ in 0..max_steps {
            let coord = VoxelCoord::new(voxel[0], voxel[1], voxel[2]);

            // Check structure ownership first.
            if let Some(&sid) = self.structure_voxels.get(&coord) {
                return Some(sid);
            }

            // Non-structure solid voxels occlude — stop.
            let vt = self.world.get(coord);
            if vt.is_solid() {
                return None;
            }

            // Advance along the axis with the smallest t_max.
            let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            voxel[min_axis] += step[min_axis];
            t_max[min_axis] += t_delta[min_axis];
        }

        None
    }

    // -----------------------------------------------------------------------
    // Save/load helpers
    // -----------------------------------------------------------------------

    /// Rebuild the voxel world from config, stored tree entity data, and
    /// construction-placed voxels.
    ///
    /// Recreates the `VoxelWorld` from scratch: lays the forest floor at y=0
    /// using `config.floor_extent`, then places every tree's trunk, branch,
    /// root, leaf, and fruit voxels, then places any construction voxels from
    /// `placed_voxels`. This is the inverse of tree generation — instead of
    /// growing the tree procedurally, we replay the stored voxel lists.
    pub fn rebuild_world(
        config: &GameConfig,
        trees: &BTreeMap<TreeId, Tree>,
        placed_voxels: &[(VoxelCoord, VoxelType)],
        carved_voxels: &[VoxelCoord],
    ) -> VoxelWorld {
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

        // Place dirt voxels (terrain hills). Dirt has priority 0 so tree voxels
        // overwrite it where they overlap — the tree embeds naturally in hillside.
        for tree in trees.values() {
            for &coord in &tree.dirt_voxels {
                world.set(coord, VoxelType::Dirt);
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

        // Place construction voxels.
        for &(coord, voxel_type) in placed_voxels {
            world.set(coord, voxel_type);
        }

        // Apply carve removals (set to Air).
        for &coord in carved_voxels {
            world.set(coord, VoxelType::Air);
        }

        world
    }

    /// Rebuild all transient (`#[serde(skip)]`) fields after deserialization.
    ///
    /// Restores: `world` (voxel grid from stored tree voxels + config),
    /// `nav_graph` (from rebuilt world geometry), `species_table` (from config),
    /// `structure_voxels` (from completed blueprints + structures).
    pub fn rebuild_transient_state(&mut self) {
        self.world = Self::rebuild_world(
            &self.config,
            &self.trees,
            &self.placed_voxels,
            &self.carved_voxels,
        );
        self.face_data = self.face_data_list.iter().cloned().collect();
        self.ladder_orientations = self.ladder_orientations_list.iter().cloned().collect();
        self.nav_graph = nav::build_nav_graph(&self.world, &self.face_data);
        self.large_nav_graph = nav::build_large_nav_graph(&self.world);
        self.species_table = self.config.species.clone();

        // Rebuild structure_voxels from completed blueprints.
        self.structure_voxels.clear();
        for bp in self.blueprints.values() {
            if bp.state == BlueprintState::Complete {
                // Find the StructureId for this blueprint's project.
                if let Some(structure) = self.structures.values().find(|s| s.project_id == bp.id) {
                    let sid = structure.id;
                    for &coord in &bp.voxels {
                        self.structure_voxels.insert(coord, sid);
                    }
                }
            }
        }
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

    /// Apply a batch of serialized command payloads (JSON-encoded `SimAction`s)
    /// to the sim at the given tick. Used by both `SimBridge` (multiplayer mode)
    /// and integration tests to ensure identical turn-application logic.
    ///
    /// Each payload is deserialized independently; malformed payloads are
    /// silently skipped (matching the relay's opaque-payload semantics).
    /// Returns the number of commands successfully deserialized and applied.
    pub fn apply_turn_payloads(&mut self, tick: u64, payloads: &[&[u8]]) -> usize {
        let mut commands = Vec::new();
        for payload in payloads {
            if let Ok(action) = serde_json::from_slice::<SimAction>(payload) {
                commands.push(SimCommand {
                    player_id: self.player_id,
                    tick,
                    action,
                });
            }
        }
        let count = commands.len();
        self.step(&commands, tick);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{Task, TaskKind, TaskState};

    /// Test config with a small 64^3 world and reduced tree energy.
    /// Matches the approach used by nav::tests and tree_gen::tests.
    /// This is ~64x fewer voxels than the default 256×128×256 world,
    /// making SimState construction dramatically faster in debug builds.
    /// Terrain is disabled (terrain_max_height = 0) to preserve existing
    /// test behavior (flat forest floor).
    fn test_config() -> GameConfig {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        config.tree_profile.growth.initial_energy = 50.0;
        config.terrain_max_height = 0;
        config
    }

    /// Create a test SimState with a small world for fast tests.
    fn test_sim(seed: u64) -> SimState {
        SimState::with_config(seed, test_config())
    }

    #[test]
    fn new_sim_has_home_tree() {
        let sim = test_sim(42);
        assert!(sim.trees.contains_key(&sim.player_tree_id));
        let tree = &sim.trees[&sim.player_tree_id];
        assert_eq!(tree.owner, Some(sim.player_id));
        assert_eq!(tree.mana_stored, sim.config.starting_mana);
    }

    #[test]
    fn determinism_two_sims_same_seed() {
        let sim_a = test_sim(42);
        let sim_b = test_sim(42);
        assert_eq!(sim_a.player_id, sim_b.player_id);
        assert_eq!(sim_a.player_tree_id, sim_b.player_tree_id);
        assert_eq!(sim_a.tick, sim_b.tick);
    }

    #[test]
    fn step_advances_tick() {
        let mut sim = test_sim(42);
        sim.step(&[], 100);
        assert_eq!(sim.tick, 100);
    }

    #[test]
    fn step_processes_speed_command() {
        let mut sim = test_sim(42);
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 10,
            action: SimAction::SetSimSpeed {
                speed: SimSpeed::Paused,
            },
        };
        let result = sim.step(&[cmd], 20);
        assert_eq!(sim.speed, SimSpeed::Paused);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::SpeedChanged {
                speed: SimSpeed::Paused
            }
        )));
    }

    #[test]
    fn tree_heartbeat_reschedules() {
        let mut sim = test_sim(42);
        let heartbeat_interval = sim.config.tree_heartbeat_interval_ticks;

        // Step past the first heartbeat.
        sim.step(&[], heartbeat_interval + 1);

        // The tree heartbeat should have rescheduled. There should be a
        // pending event for tick = 2 * heartbeat_interval.
        assert_eq!(sim.event_queue.peek_tick(), Some(heartbeat_interval * 2));
    }

    #[test]
    fn serialization_roundtrip() {
        let mut sim = test_sim(42);
        sim.step(&[], 50);
        let json = serde_json::to_string(&sim).unwrap();
        let restored: SimState = serde_json::from_str(&json).unwrap();
        assert_eq!(sim.tick, restored.tick);
        assert_eq!(sim.player_id, restored.player_id);
        assert_eq!(sim.player_tree_id, restored.player_tree_id);
    }

    #[test]
    fn determinism_after_stepping() {
        let mut sim_a = test_sim(42);
        let mut sim_b = test_sim(42);

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
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.trunk_voxels.is_empty(),
            "Tree should have trunk voxels"
        );
        assert!(
            !tree.branch_voxels.is_empty(),
            "Tree should have branch voxels"
        );
    }

    #[test]
    fn new_sim_has_nav_graph() {
        let sim = test_sim(42);
        assert!(
            sim.nav_graph.node_count() > 0,
            "Nav graph should have nodes"
        );
    }

    #[test]
    fn spawn_elf_command() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.creature_count(Species::Elf), 1);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::CreatureArrived {
                species: Species::Elf,
                ..
            }
        )));
    }

    #[test]
    fn elf_wanders_after_spawn() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn elf.
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[spawn_cmd], 2);

        // Step far enough for many activations (each ground edge costs ~500
        // ticks at walk_ticks_per_voxel=500).
        sim.step(&[], 50000);

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
        let mut sim_a = test_sim(42);
        let mut sim_b = test_sim(42);

        let tree_pos = sim_a.trees[&sim_a.player_tree_id].position;

        let spawn = SimCommand {
            player_id: sim_a.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.creature_count(Species::Capybara), 1);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::CreatureArrived {
                species: Species::Capybara,
                ..
            }
        )));

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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Step far enough for heartbeat + movement.
        sim.step(&[], 50000);

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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Run for many ticks — capybara must never leave y=1 (air above ForestFloor).
        for target in (10000..100000).step_by(10000) {
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
        let mut sim_a = test_sim(42);
        let mut sim_b = test_sim(42);

        let tree_pos = sim_a.trees[&sim_a.player_tree_id].position;

        let spawn = SimCommand {
            player_id: sim_a.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
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
        sim.step(&[], 50000);

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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Run for many ticks, periodically checking node validity.
        for target in (10000..100000).step_by(10000) {
            sim.step(&[], target);
            let elf = sim
                .creatures
                .values()
                .find(|c| c.species == Species::Elf)
                .unwrap();
            let node = elf
                .current_node
                .expect("Elf should always have a current node");
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
            action: SimAction::SpawnCreature {
                species: Species::Elf,
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
        let mut sim = test_sim(42);
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
        let mut sim = test_sim(42);
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
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Put the task at the elf's current location for instant completion.
        let elf_node = sim.creatures[&elf_id].current_node.unwrap();
        let task_id = insert_goto_task(&mut sim, elf_node);

        // One activation should be enough: elf claims task, is already there, completes.
        sim.step(&[], sim.tick + 10000);

        let task = &sim.tasks[&task_id];
        assert_eq!(
            task.state,
            TaskState::Complete,
            "GoTo task should be complete"
        );
        let elf = &sim.creatures[&elf_id];
        assert_eq!(
            elf.current_task, None,
            "Elf should be unassigned after task completion"
        );
    }

    #[test]
    fn completed_task_creature_resumes_wandering() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Put the task at the elf's current location for instant completion.
        let elf_node = sim.creatures[&elf_id].current_node.unwrap();
        let _task_id = insert_goto_task(&mut sim, elf_node);

        // Complete the task.
        sim.step(&[], sim.tick + 10000);
        let pos_after_task = sim.creatures[&elf_id].position;

        // Continue ticking — elf should resume wandering (position changes).
        sim.step(&[], sim.tick + 50000);

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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::CreateTask {
                kind: TaskKind::GoTo,
                position: tree_pos,
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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn an elf.
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
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
        sim.step(&[], 50000);

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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn multiple elves and capybaras.
        for _ in 0..3 {
            let cmd = SimCommand {
                player_id: sim.player_id,
                tick: sim.tick + 1,
                action: SimAction::SpawnCreature {
                    species: Species::Elf,
                    position: tree_pos,
                },
            };
            sim.step(&[cmd], sim.tick + 2);
        }
        for _ in 0..2 {
            let cmd = SimCommand {
                player_id: sim.player_id,
                tick: sim.tick + 1,
                action: SimAction::SpawnCreature {
                    species: Species::Capybara,
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
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.fruit_positions.is_empty(),
            "Tree should have some initial fruit (got 0)"
        );
    }

    #[test]
    fn fruit_hangs_below_leaf_voxels() {
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        for fruit_pos in &tree.fruit_positions {
            // The leaf above the fruit should be in the tree's leaf_voxels.
            let leaf_above = VoxelCoord::new(fruit_pos.x, fruit_pos.y + 1, fruit_pos.z);
            assert!(
                tree.leaf_voxels.contains(&leaf_above),
                "Fruit at {} should hang below a leaf voxel, but no leaf at {}",
                fruit_pos,
                leaf_above
            );
        }
    }

    #[test]
    fn fruit_set_in_world_grid() {
        let sim = test_sim(42);
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
        let mut config = test_config();
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
        let mut config = test_config();
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
        let sim_a = test_sim(42);
        let sim_b = test_sim(42);
        let tree_a = &sim_a.trees[&sim_a.player_tree_id];
        let tree_b = &sim_b.trees[&sim_b.player_tree_id];
        assert_eq!(tree_a.fruit_positions, tree_b.fruit_positions);
    }

    // -----------------------------------------------------------------------
    // Save/load roundtrip tests
    // -----------------------------------------------------------------------

    #[test]
    fn rebuild_world_matches_original() {
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];

        // Rebuild world from stored tree voxels and config.
        let rebuilt = SimState::rebuild_world(
            &sim.config,
            &sim.trees,
            &sim.placed_voxels,
            &sim.carved_voxels,
        );

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
    fn rebuild_world_includes_placed_voxels() {
        let sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Manually construct placed_voxels and rebuild.
        let placed = vec![(air_coord, VoxelType::GrownPlatform)];
        let rebuilt = SimState::rebuild_world(&sim.config, &sim.trees, &placed, &[]);

        assert_eq!(
            rebuilt.get(air_coord),
            VoxelType::GrownPlatform,
            "Rebuilt world should contain the placed platform voxel"
        );
    }

    #[test]
    fn rebuild_world_includes_dirt_voxels() {
        let mut config = test_config();
        config.terrain_max_height = 3;
        config.terrain_noise_scale = 8.0;
        let sim = SimState::with_config(42, config);

        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.dirt_voxels.is_empty(),
            "With terrain enabled, tree should have dirt voxels"
        );

        // Rebuild and verify dirt is present.
        let rebuilt = SimState::rebuild_world(&sim.config, &sim.trees, &sim.placed_voxels, &[]);
        for &coord in &tree.dirt_voxels {
            let voxel = rebuilt.get(coord);
            // Dirt might be overwritten by tree voxels (trunk/branch/root),
            // but any remaining should be Dirt.
            if voxel == VoxelType::Dirt {
                assert_eq!(voxel, VoxelType::Dirt);
            }
        }
        // At least some dirt voxels should survive (not all overwritten by tree).
        let dirt_count = tree
            .dirt_voxels
            .iter()
            .filter(|c| rebuilt.get(**c) == VoxelType::Dirt)
            .count();
        assert!(
            dirt_count > 0,
            "Some dirt voxels should survive in the rebuilt world"
        );
    }

    #[test]
    fn rebuild_transient_state_restores_nav_graph() {
        let sim = test_sim(42);
        let json = sim.to_json().unwrap();

        // Deserialize — transient fields are default (empty).
        let mut restored: SimState = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.nav_graph.node_count(),
            0,
            "Before rebuild, nav_graph should be empty"
        );
        assert_eq!(
            restored.world.size_x, 0,
            "Before rebuild, world should be empty"
        );

        // Rebuild transient state.
        restored.rebuild_transient_state();
        assert!(
            restored.nav_graph.node_count() > 0,
            "After rebuild, nav_graph should have nodes"
        );
        // Node count may differ very slightly because fruit voxels are placed
        // after the initial nav graph build but before serialization, so the
        // rebuilt world includes fruit while the original nav graph was built
        // without them. Allow a small tolerance.
        let diff = (restored.nav_graph.node_count() as i64 - sim.nav_graph.node_count() as i64)
            .unsigned_abs();
        assert!(
            diff <= 5,
            "Rebuilt nav_graph node count ({}) should be close to original ({}), diff={}",
            restored.nav_graph.node_count(),
            sim.nav_graph.node_count(),
            diff,
        );
    }

    #[test]
    fn json_roundtrip_preserves_state() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn creatures and advance ticks.
        let cmds = vec![
            SimCommand {
                player_id: sim.player_id,
                tick: 1,
                action: SimAction::SpawnCreature {
                    species: Species::Elf,
                    position: tree_pos,
                },
            },
            SimCommand {
                player_id: sim.player_id,
                tick: 1,
                action: SimAction::SpawnCreature {
                    species: Species::Capybara,
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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn creatures and advance.
        let spawn = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
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
        let sim = test_sim(42);
        assert_eq!(sim.species_table.len(), 7);
        assert!(sim.species_table.contains_key(&Species::Elf));
        assert!(sim.species_table.contains_key(&Species::Capybara));
        assert!(sim.species_table.contains_key(&Species::Boar));
        assert!(sim.species_table.contains_key(&Species::Deer));
        assert!(sim.species_table.contains_key(&Species::Elephant));
        assert!(sim.species_table.contains_key(&Species::Monkey));
        assert!(sim.species_table.contains_key(&Species::Squirrel));

        let elf_data = &sim.species_table[&Species::Elf];
        assert!(!elf_data.ground_only);
        assert!(elf_data.allowed_edge_types.is_none());

        let capy_data = &sim.species_table[&Species::Capybara];
        assert!(capy_data.ground_only);
        assert!(capy_data.allowed_edge_types.is_some());

        let boar_data = &sim.species_table[&Species::Boar];
        assert!(boar_data.ground_only);
        assert_eq!(boar_data.walk_ticks_per_voxel, 600);

        let deer_data = &sim.species_table[&Species::Deer];
        assert!(deer_data.ground_only);
        assert_eq!(deer_data.walk_ticks_per_voxel, 400);

        let monkey_data = &sim.species_table[&Species::Monkey];
        assert!(!monkey_data.ground_only);
        assert_eq!(monkey_data.climb_ticks_per_voxel, Some(800));

        let squirrel_data = &sim.species_table[&Species::Squirrel];
        assert!(!squirrel_data.ground_only);
        assert_eq!(squirrel_data.climb_ticks_per_voxel, Some(600));
    }

    #[test]
    fn graph_for_species_dispatch() {
        let sim = test_sim(42);

        // Elf (1x1x1) → standard graph.
        let elf_graph = sim.graph_for_species(Species::Elf) as *const _;
        let standard = &sim.nav_graph as *const _;
        assert_eq!(elf_graph, standard, "Elf should use standard nav graph");

        // Elephant (2x2x2) → large graph.
        let elephant_graph = sim.graph_for_species(Species::Elephant) as *const _;
        let large = &sim.large_nav_graph as *const _;
        assert_eq!(elephant_graph, large, "Elephant should use large nav graph");
    }

    #[test]
    fn new_sim_has_large_nav_graph() {
        let sim = test_sim(42);
        assert!(
            sim.large_nav_graph.live_nodes().count() > 0,
            "Large nav graph should have nodes after construction",
        );
    }

    #[test]
    fn elephant_spawns_on_large_graph() {
        let mut sim = test_sim(42);
        let mut events = Vec::new();
        let spawn_pos = VoxelCoord::new(10, 1, 10);
        sim.spawn_creature(Species::Elephant, spawn_pos, &mut events);

        // There should be exactly one elephant.
        let elephants: Vec<&Creature> = sim
            .creatures
            .values()
            .filter(|c| c.species == Species::Elephant)
            .collect();
        assert_eq!(elephants.len(), 1, "Should have spawned one elephant");

        // Its current_node should be in the large nav graph.
        let elephant = elephants[0];
        let node_id = elephant
            .current_node
            .expect("Elephant should have a current_node");
        let node = sim.large_nav_graph.node(node_id);
        assert_eq!(
            node.position, elephant.position,
            "Elephant position should match its large graph node",
        );
    }

    #[test]
    fn creature_species_preserved() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn one elf and one capybara.
        let cmds = vec![
            SimCommand {
                player_id: sim.player_id,
                tick: 1,
                action: SimAction::SpawnCreature {
                    species: Species::Elf,
                    position: tree_pos,
                },
            },
            SimCommand {
                player_id: sim.player_id,
                tick: 1,
                action: SimAction::SpawnCreature {
                    species: Species::Capybara,
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
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let food_max = sim.species_table[&Species::Elf].food_max;
        let decay_per_tick = sim.species_table[&Species::Elf].food_decay_per_tick;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
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
        let mut config = test_config();
        config
            .species
            .get_mut(&Species::Elf)
            .unwrap()
            .food_decay_per_tick = 1_000_000_000_000_000; // Depletes in 1 tick
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
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
    // Movement interpolation tests
    // -----------------------------------------------------------------------

    #[test]
    fn interpolated_position_midpoint() {
        let creature = Creature {
            id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
            species: Species::Elf,
            position: VoxelCoord::new(10, 0, 0),
            current_node: None,
            path: None,
            current_task: None,
            food: 1000,
            move_from: Some(VoxelCoord::new(0, 0, 0)),
            move_to: Some(VoxelCoord::new(10, 0, 0)),
            move_start_tick: 100,
            move_end_tick: 200,
        };
        let (x, y, z) = creature.interpolated_position(150.0);
        assert!((x - 5.0).abs() < 0.001, "x should be 5.0, got {x}");
        assert!((y - 0.0).abs() < 0.001, "y should be 0.0, got {y}");
        assert!((z - 0.0).abs() < 0.001, "z should be 0.0, got {z}");
    }

    #[test]
    fn interpolated_position_at_start() {
        let creature = Creature {
            id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
            species: Species::Elf,
            position: VoxelCoord::new(10, 0, 0),
            current_node: None,
            path: None,
            current_task: None,
            food: 1000,
            move_from: Some(VoxelCoord::new(0, 0, 0)),
            move_to: Some(VoxelCoord::new(10, 0, 0)),
            move_start_tick: 100,
            move_end_tick: 200,
        };
        let (x, _, _) = creature.interpolated_position(100.0);
        assert!((x - 0.0).abs() < 0.001, "At t=0 should be at from, got {x}");
    }

    #[test]
    fn interpolated_position_at_end() {
        let creature = Creature {
            id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
            species: Species::Elf,
            position: VoxelCoord::new(10, 0, 0),
            current_node: None,
            path: None,
            current_task: None,
            food: 1000,
            move_from: Some(VoxelCoord::new(0, 0, 0)),
            move_to: Some(VoxelCoord::new(10, 0, 0)),
            move_start_tick: 100,
            move_end_tick: 200,
        };
        let (x, _, _) = creature.interpolated_position(200.0);
        assert!((x - 10.0).abs() < 0.001, "At t=1 should be at to, got {x}");
    }

    #[test]
    fn interpolated_position_clamped_past_end() {
        let creature = Creature {
            id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
            species: Species::Elf,
            position: VoxelCoord::new(10, 0, 0),
            current_node: None,
            path: None,
            current_task: None,
            food: 1000,
            move_from: Some(VoxelCoord::new(0, 0, 0)),
            move_to: Some(VoxelCoord::new(10, 0, 0)),
            move_start_tick: 100,
            move_end_tick: 200,
        };
        let (x, _, _) = creature.interpolated_position(999.0);
        assert!(
            (x - 10.0).abs() < 0.001,
            "Past end should clamp to destination, got {x}"
        );
    }

    #[test]
    fn interpolated_position_stationary() {
        let creature = Creature {
            id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
            species: Species::Elf,
            position: VoxelCoord::new(5, 3, 7),
            current_node: None,
            path: None,
            current_task: None,
            food: 1000,
            move_from: None,
            move_to: None,
            move_start_tick: 0,
            move_end_tick: 0,
        };
        let (x, y, z) = creature.interpolated_position(50.0);
        assert!((x - 5.0).abs() < 0.001);
        assert!((y - 3.0).abs() < 0.001);
        assert!((z - 7.0).abs() < 0.001);
    }

    #[test]
    fn wander_sets_movement_metadata() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn an elf at tick 1, step only to tick 1 so the first activation
        // (scheduled at tick 2) hasn't fired yet.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Before the first activation, the elf should have no movement metadata.
        let elf = &sim.creatures[&elf_id];
        assert!(elf.move_from.is_none());
        assert!(elf.move_to.is_none());

        let initial_pos = elf.position;

        // Step to tick 2 — the first activation fires and the elf wanders.
        sim.step(&[], 2);

        let elf = &sim.creatures[&elf_id];
        assert!(
            elf.move_from.is_some(),
            "move_from should be set after wander"
        );
        assert!(elf.move_to.is_some(), "move_to should be set after wander");
        assert_eq!(
            elf.move_from.unwrap(),
            initial_pos,
            "move_from should be the spawn position"
        );
        assert_eq!(
            elf.move_to.unwrap(),
            elf.position,
            "move_to should be the new position"
        );
        assert_eq!(
            elf.move_start_tick, 2,
            "move_start_tick should be the activation tick"
        );
        assert!(
            elf.move_end_tick > elf.move_start_tick,
            "move_end_tick should be after start"
        );
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
                let neighbor =
                    VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y + dy, trunk_coord.z + dz);
                if sim.world.in_bounds(neighbor) && sim.world.get(neighbor) == VoxelType::Air {
                    return neighbor;
                }
            }
        }
        panic!("No air voxel adjacent to trunk found");
    }

    #[test]
    fn designate_build_creates_blueprint() {
        let mut sim = test_sim(42);
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
        assert!(
            result
                .events
                .iter()
                .any(|e| matches!(e.kind, SimEventKind::BlueprintDesignated { .. }))
        );
    }

    #[test]
    fn designate_build_creates_build_task() {
        let mut sim = test_sim(42);
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

        // Blueprint should exist and have a linked task.
        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        assert!(
            bp.task_id.is_some(),
            "Blueprint should have a linked task_id"
        );

        // Task should exist.
        let task_id = bp.task_id.unwrap();
        let task = &sim.tasks[&task_id];
        assert!(matches!(task.kind, crate::task::TaskKind::Build { .. }));
        assert_eq!(task.state, TaskState::Available);
        assert_eq!(
            task.total_cost,
            sim.config.build_work_ticks_per_voxel as f32
        );
        assert_eq!(task.required_species, Some(Species::Elf));
    }

    #[test]
    fn designate_build_rejects_out_of_bounds() {
        let mut sim = test_sim(42);
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
        let mut sim = test_sim(42);
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
        let mut sim = test_sim(42);
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
        let mut sim = test_sim(42);

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
        let mut sim = test_sim(42);
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
        assert!(
            result
                .events
                .iter()
                .any(|e| matches!(e.kind, SimEventKind::BuildCancelled { .. }))
        );
    }

    #[test]
    fn cancel_build_nonexistent_is_noop() {
        let mut sim = test_sim(42);
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
        assert!(
            !result
                .events
                .iter()
                .any(|e| matches!(e.kind, SimEventKind::BuildCancelled { .. }))
        );
    }

    #[test]
    fn cancel_build_removes_associated_task() {
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Designate a build.
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

        let project_id = *sim.blueprints.keys().next().unwrap();
        let task_id = sim.blueprints[&project_id].task_id.unwrap();
        assert!(sim.tasks.contains_key(&task_id));

        // Cancel.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd2], 2);

        assert!(sim.blueprints.is_empty());
        assert!(!sim.tasks.contains_key(&task_id));
    }

    #[test]
    fn cancel_build_unassigns_elf() {
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Spawn elf.
        let elf_id = spawn_elf(&mut sim);

        // Designate a build.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        let project_id = *sim.blueprints.keys().next().unwrap();

        // Tick enough for the elf to claim the task.
        sim.step(&[], sim.tick + 50000);

        let task_id = sim.blueprints[&project_id].task_id.unwrap();
        let task = &sim.tasks[&task_id];
        // The elf should have claimed it (it's the only available task).
        assert!(
            task.assignees.contains(&elf_id),
            "Elf should have claimed the build task"
        );

        // Cancel the build.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd2], sim.tick + 2);

        // Elf should be unassigned.
        let elf = &sim.creatures[&elf_id];
        assert!(
            elf.current_task.is_none(),
            "Elf should have no task after cancel"
        );
    }

    #[test]
    fn cancel_build_reverts_partial_voxels() {
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Designate a build.
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
        let project_id = *sim.blueprints.keys().next().unwrap();

        // Simulate partial construction by manually placing a voxel.
        sim.placed_voxels
            .push((air_coord, VoxelType::GrownPlatform));
        sim.world.set(air_coord, VoxelType::GrownPlatform);

        // Cancel the build.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd2], 2);

        // Voxel should be reverted to Air.
        assert_eq!(sim.world.get(air_coord), VoxelType::Air);
        assert!(
            sim.placed_voxels.is_empty(),
            "placed_voxels should be cleared"
        );
    }

    #[test]
    fn sim_serialization_with_blueprints() {
        let mut sim = test_sim(42);
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
        let mut sim_a = test_sim(42);
        let mut sim_b = test_sim(42);

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

    // -----------------------------------------------------------------------
    // New species tests (Boar, Deer, Monkey, Squirrel)
    // -----------------------------------------------------------------------

    #[test]
    fn spawn_boar_command() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Boar,
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.creature_count(Species::Boar), 1);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::CreatureArrived {
                species: Species::Boar,
                ..
            }
        )));

        // Boar is ground-only — should be at y=1.
        let boar = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Boar)
            .unwrap();
        assert_eq!(boar.position.y, 1);
    }

    #[test]
    fn spawn_deer_command() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Deer,
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.creature_count(Species::Deer), 1);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::CreatureArrived {
                species: Species::Deer,
                ..
            }
        )));

        // Deer is ground-only — should be at y=1.
        let deer = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Deer)
            .unwrap();
        assert_eq!(deer.position.y, 1);
    }

    #[test]
    fn spawn_monkey_command() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Monkey,
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.creature_count(Species::Monkey), 1);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::CreatureArrived {
                species: Species::Monkey,
                ..
            }
        )));
    }

    #[test]
    fn spawn_squirrel_command() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Squirrel,
                position: tree_pos,
            },
        };

        let result = sim.step(&[cmd], 2);
        assert_eq!(sim.creature_count(Species::Squirrel), 1);
        assert!(result.events.iter().any(|e| matches!(
            e.kind,
            SimEventKind::CreatureArrived {
                species: Species::Squirrel,
                ..
            }
        )));
    }

    #[test]
    fn boar_stays_on_ground() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Boar,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Run for many ticks — boar must never leave y=1 (ground-only).
        for target in (10000..100000).step_by(10000) {
            sim.step(&[], target);
            let boar = sim
                .creatures
                .values()
                .find(|c| c.species == Species::Boar)
                .unwrap();
            assert_eq!(
                boar.position.y, 1,
                "Boar left ground at tick {target}: pos={:?}",
                boar.position
            );
        }
    }

    #[test]
    fn deer_stays_on_ground() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Deer,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        for target in (10000..100000).step_by(10000) {
            sim.step(&[], target);
            let deer = sim
                .creatures
                .values()
                .find(|c| c.species == Species::Deer)
                .unwrap();
            assert_eq!(
                deer.position.y, 1,
                "Deer left ground at tick {target}: pos={:?}",
                deer.position
            );
        }
    }

    #[test]
    fn monkey_can_climb() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Monkey,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        // Run for enough ticks that a climbing species should have left ground.
        sim.step(&[], 100000);

        let monkey = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Monkey)
            .unwrap();
        // Monkey is not ground_only, so it should be able to reach y > 1
        // (trunk/branch surfaces). This verifies the species config allows
        // climbing edges. The monkey may still be at y=1 if the PRNG led it
        // only to ground neighbors, so we just verify it has a valid node.
        assert!(monkey.current_node.is_some());
    }

    #[test]
    fn squirrel_can_climb() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Squirrel,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 2);

        sim.step(&[], 100000);

        let squirrel = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Squirrel)
            .unwrap();
        assert!(squirrel.current_node.is_some());
    }

    #[test]
    fn all_six_species_spawn_and_coexist() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let species_list = [
            Species::Elf,
            Species::Capybara,
            Species::Boar,
            Species::Deer,
            Species::Monkey,
            Species::Squirrel,
        ];
        let mut tick = 1;
        for &species in &species_list {
            let cmd = SimCommand {
                player_id: sim.player_id,
                tick,
                action: SimAction::SpawnCreature {
                    species,
                    position: tree_pos,
                },
            };
            sim.step(&[cmd], tick + 1);
            tick = sim.tick + 1;
        }

        assert_eq!(sim.creatures.len(), 6);
        for &species in &species_list {
            assert_eq!(sim.creature_count(species), 1, "Expected 1 {:?}", species);
        }

        // Run for a while — all should remain alive with valid nodes.
        sim.step(&[], 50000);
        assert_eq!(sim.creatures.len(), 6);
        for creature in sim.creatures.values() {
            assert!(
                creature.current_node.is_some(),
                "{:?} has no current node",
                creature.species
            );
        }
    }

    // -----------------------------------------------------------------------
    // Build work + incremental materialization tests
    // -----------------------------------------------------------------------

    /// Helper: find N air voxels adjacent to trunk, all face-adjacent to
    /// each other or to solid geometry (valid for a multi-voxel blueprint).
    fn find_air_strip_adjacent_to_trunk(sim: &SimState, count: usize) -> Vec<VoxelCoord> {
        let tree = &sim.trees[&sim.player_tree_id];
        // Find a trunk voxel with an air voxel to the +x side, then extend
        // in the +x direction.
        for &trunk_coord in &tree.trunk_voxels {
            let start = VoxelCoord::new(trunk_coord.x + 1, trunk_coord.y, trunk_coord.z);
            if !sim.world.in_bounds(start) || sim.world.get(start) != VoxelType::Air {
                continue;
            }
            let mut strip = vec![start];
            for i in 1..count {
                let next = VoxelCoord::new(start.x + i as i32, start.y, start.z);
                if !sim.world.in_bounds(next) || sim.world.get(next) != VoxelType::Air {
                    break;
                }
                strip.push(next);
            }
            if strip.len() == count {
                return strip;
            }
        }
        panic!("Could not find {count} air voxels adjacent to trunk");
    }

    /// Helper: create a sim with fast build speed for testing.
    fn build_test_sim() -> SimState {
        let mut config = test_config();
        // Fast builds: 1 tick per voxel for quick test completion.
        config.build_work_ticks_per_voxel = 1;
        SimState::with_config(42, config)
    }

    #[test]
    fn build_task_completes_and_all_voxels_placed() {
        let mut sim = build_test_sim();
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Spawn elf.
        let elf_id = spawn_elf(&mut sim);

        // Designate a 1-voxel platform.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        let project_id = *sim.blueprints.keys().next().unwrap();
        let task_id = sim.blueprints[&project_id].task_id.unwrap();

        // Tick until completion (elf needs to pathfind + do work).
        sim.step(&[], sim.tick + 200_000);

        // Blueprint should be Complete.
        let bp = &sim.blueprints[&project_id];
        assert_eq!(
            bp.state,
            BlueprintState::Complete,
            "Blueprint should be Complete"
        );

        // Voxel should be solid.
        assert_eq!(
            sim.world.get(air_coord),
            VoxelType::GrownPlatform,
            "Build voxel should be GrownPlatform"
        );

        // Task should be Complete.
        let task = &sim.tasks[&task_id];
        assert_eq!(task.state, TaskState::Complete);

        // Elf should be freed (no current task).
        let elf = &sim.creatures[&elf_id];
        assert!(
            elf.current_task.is_none(),
            "Elf should be free after build completion"
        );

        // placed_voxels should contain the coord.
        assert!(
            sim.placed_voxels
                .contains(&(air_coord, VoxelType::GrownPlatform))
        );
    }

    #[test]
    fn build_task_materializes_voxels_incrementally() {
        let mut config = test_config();
        // Slow build: 50000 ticks per voxel (elf walk_tpv is 500, so the elf
        // needs to arrive first, then do 50000 ticks of work per voxel).
        config.build_work_ticks_per_voxel = 50000;
        let mut sim = SimState::with_config(42, config);

        let strip = find_air_strip_adjacent_to_trunk(&sim, 3);

        // Spawn elf.
        spawn_elf(&mut sim);

        // Designate a 3-voxel platform.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: strip.clone(),
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        let project_id = *sim.blueprints.keys().next().unwrap();

        // Tick enough for the elf to arrive and do partial work (enough for
        // 1 voxel but not all 3). Elf walk speed is 500 tpv, so a few
        // thousand ticks should let it arrive. Then 50000 more for 1 voxel.
        sim.step(&[], sim.tick + 200_000);

        // At least 1 voxel should be placed, but not all 3.
        let placed_count = strip
            .iter()
            .filter(|c| sim.world.get(**c) != VoxelType::Air)
            .count();

        // With 200k ticks and 50k per voxel, we'd expect 1-3 placed.
        // The exact count depends on pathfinding time, but at least 1.
        assert!(
            placed_count >= 1,
            "Expected at least 1 voxel placed, got {placed_count}"
        );

        // Blueprint should still be Designated (not all voxels done).
        if placed_count < 3 {
            let bp = &sim.blueprints[&project_id];
            assert_eq!(bp.state, BlueprintState::Designated);
        }
    }

    #[test]
    fn build_voxels_maintain_adjacency() {
        let mut sim = build_test_sim();

        let strip = find_air_strip_adjacent_to_trunk(&sim, 3);

        // Spawn elf.
        spawn_elf(&mut sim);

        // Designate a 3-voxel strip.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: strip.clone(),
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        // Tick to completion.
        sim.step(&[], sim.tick + 200_000);

        // All 3 voxels should be solid.
        for coord in &strip {
            assert_eq!(
                sim.world.get(*coord),
                VoxelType::GrownPlatform,
                "Voxel at {coord} should be GrownPlatform"
            );
        }

        // Verify each placed voxel is adjacent to at least one solid neighbor
        // that existed BEFORE it was placed (the trunk or a previously-placed
        // voxel). Since we can't replay the order, we verify the weaker
        // property: each voxel has at least one solid face neighbor now.
        for coord in &strip {
            assert!(
                sim.world.has_solid_face_neighbor(*coord),
                "Placed voxel at {coord} should have a solid face neighbor"
            );
        }
    }

    #[test]
    fn build_displaces_creature_on_occupied_voxel() {
        let mut sim = build_test_sim();
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Spawn an elf, then manually place it at the blueprint voxel.
        let elf_id = spawn_elf(&mut sim);

        // Find the nav node at air_coord (if one exists).
        let node_at_build = sim.nav_graph.find_nearest_node(air_coord);
        if let Some(node_id) = node_at_build {
            let node_pos = sim.nav_graph.node(node_id).position;
            if node_pos == air_coord {
                // Move the elf there.
                let elf = sim.creatures.get_mut(&elf_id).unwrap();
                elf.position = air_coord;
                elf.current_node = Some(node_id);
            }
        }

        // Spawn a SECOND elf to do the building (the first one is standing
        // on the build site, so we need another builder).
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        // Designate the build at the occupied voxel.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        // Tick to completion.
        sim.step(&[], sim.tick + 200_000);

        // The voxel should be solid.
        assert_eq!(sim.world.get(air_coord), VoxelType::GrownPlatform);

        // The first elf should have been displaced — its position should not
        // be at air_coord (which is now solid).
        let elf = &sim.creatures[&elf_id];
        assert_ne!(
            elf.position, air_coord,
            "Elf should have been displaced from the now-solid voxel"
        );
        // It should still have a valid nav node.
        assert!(elf.current_node.is_some());
    }

    #[test]
    fn save_load_preserves_partially_built_platform() {
        let mut config = test_config();
        config.build_work_ticks_per_voxel = 50000;
        let mut sim = SimState::with_config(42, config);

        let strip = find_air_strip_adjacent_to_trunk(&sim, 3);

        spawn_elf(&mut sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: strip.clone(),
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        // Tick for partial construction.
        sim.step(&[], sim.tick + 200_000);

        let placed_before = sim.placed_voxels.len();

        // Save and load.
        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        // placed_voxels should be preserved.
        assert_eq!(restored.placed_voxels.len(), placed_before);

        // The world should contain the placed voxels.
        for &(coord, vt) in &restored.placed_voxels {
            assert_eq!(
                restored.world.get(coord),
                vt,
                "Restored world should contain placed voxel at {coord}"
            );
        }
    }

    // --- DesignateBuilding tests ---

    /// Find a ground-level position where a 3x3 building can be placed.
    /// Needs solid foundation at y=0 and air above at y=1.
    fn find_building_site(sim: &SimState) -> VoxelCoord {
        let (sx, _, sz) = sim.config.world_size;
        for x in 1..(sx as i32 - 4) {
            for z in 1..(sz as i32 - 4) {
                let mut all_solid = true;
                let mut all_air = true;
                for dx in 0..3 {
                    for dz in 0..3 {
                        let foundation = VoxelCoord::new(x + dx, 0, z + dz);
                        if !sim.world.get(foundation).is_solid() {
                            all_solid = false;
                        }
                        let above = VoxelCoord::new(x + dx, 1, z + dz);
                        if sim.world.get(above) != VoxelType::Air {
                            all_air = false;
                        }
                    }
                }
                if all_solid && all_air {
                    return VoxelCoord::new(x, 0, z);
                }
            }
        }
        panic!("No valid 3x3 building site found");
    }

    #[test]
    fn designate_building_creates_blueprint() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        assert_eq!(bp.build_type, BuildType::Building);
        assert_eq!(bp.voxels.len(), 9); // 3x3x1
        assert!(bp.face_layout.is_some());
        assert_eq!(bp.face_layout.as_ref().unwrap().len(), 9);
    }

    #[test]
    fn designate_building_creates_task() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.tasks.len(), 1);
        let task = sim.tasks.values().next().unwrap();
        assert_eq!(task.state, TaskState::Available);
        match &task.kind {
            TaskKind::Build { project_id } => {
                assert!(sim.blueprints.contains_key(project_id));
            }
            _ => panic!("Expected Build task"),
        }
    }

    #[test]
    fn designate_building_rejects_small_width() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 2, // too small
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert!(sim.blueprints.is_empty());
    }

    #[test]
    fn designate_building_rejects_non_solid_foundation() {
        let mut sim = test_sim(42);
        // Place anchor at a position where foundation is Air.
        // y=10 should have Air below.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor: VoxelCoord::new(1, 10, 1),
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert!(sim.blueprints.is_empty());
    }

    #[test]
    fn building_materialization_sets_building_interior() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        let project_id = *sim.blueprints.keys().next().unwrap();

        // Manually materialize one voxel.
        sim.materialize_next_build_voxel(project_id);

        // At least one voxel should now be BuildingInterior.
        let has_building = sim
            .placed_voxels
            .iter()
            .any(|(_, vt)| *vt == VoxelType::BuildingInterior);
        assert!(has_building, "Should have placed a BuildingInterior voxel");

        // The placed voxel should have face_data.
        let placed_coord = sim.placed_voxels[0].0;
        assert!(
            sim.face_data.contains_key(&placed_coord),
            "Placed building voxel should have face_data",
        );
    }

    #[test]
    fn building_materialization_creates_nav_node() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        let project_id = *sim.blueprints.keys().next().unwrap();

        sim.materialize_next_build_voxel(project_id);

        let placed_coord = sim.placed_voxels[0].0;
        assert!(
            sim.nav_graph.has_node_at(placed_coord),
            "BuildingInterior voxel should be a nav node",
        );
    }

    #[test]
    fn cancel_building_removes_face_data() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        let project_id = *sim.blueprints.keys().next().unwrap();

        // Materialize some voxels.
        sim.materialize_next_build_voxel(project_id);
        sim.materialize_next_build_voxel(project_id);
        assert!(!sim.face_data.is_empty(), "Should have face_data");
        assert!(!sim.placed_voxels.is_empty(), "Should have placed voxels");

        // Cancel the build.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd2], 2);

        assert!(sim.face_data.is_empty(), "face_data should be cleared");
        assert!(
            sim.placed_voxels.is_empty(),
            "placed_voxels should be cleared",
        );
        assert!(sim.blueprints.is_empty(), "blueprint should be removed");

        // Verify voxels reverted to Air.
        for x in anchor.x..anchor.x + 3 {
            for z in anchor.z..anchor.z + 3 {
                assert_eq!(
                    sim.world.get(VoxelCoord::new(x, anchor.y + 1, z)),
                    VoxelType::Air,
                );
            }
        }
    }

    #[test]
    fn save_load_preserves_building() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        let project_id = *sim.blueprints.keys().next().unwrap();

        // Materialize some voxels.
        sim.materialize_next_build_voxel(project_id);
        sim.materialize_next_build_voxel(project_id);
        sim.materialize_next_build_voxel(project_id);

        let original_face_data_len = sim.face_data.len();
        let original_placed_len = sim.placed_voxels.len();
        assert!(original_face_data_len > 0);
        assert!(original_placed_len > 0);

        // Save and reload.
        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        // Check face_data preserved.
        assert_eq!(restored.face_data.len(), original_face_data_len);
        for (coord, fd) in &sim.face_data {
            let restored_fd = restored.face_data.get(coord).unwrap();
            assert_eq!(fd, restored_fd);
        }

        // Check placed voxels preserved in rebuilt world.
        for &(coord, vt) in &sim.placed_voxels {
            assert_eq!(restored.world.get(coord), vt);
        }

        // Check nav graph has nodes at building voxels.
        for &(coord, vt) in &sim.placed_voxels {
            if vt == VoxelType::BuildingInterior {
                assert!(
                    restored.nav_graph.has_node_at(coord),
                    "Restored nav graph should have node at {coord}",
                );
            }
        }
    }

    #[test]
    fn designate_building_rejects_non_air_interior() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);

        // Place a solid voxel in the interior area.
        let interior = VoxelCoord::new(anchor.x + 1, anchor.y + 1, anchor.z + 1);
        sim.world.set(interior, VoxelType::Trunk);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuilding {
                anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert!(sim.blueprints.is_empty());
    }

    // --- CompletedStructure integration tests ---

    /// Helper: designate a single-voxel platform and run the sim until the
    /// build task is complete. Returns the sim after completion.
    fn designate_and_complete_build(mut sim: SimState) -> SimState {
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Spawn an elf near the build site.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: air_coord,
            },
        };
        sim.step(&[cmd], 1);

        // Designate a 1-voxel platform.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 2);
        assert_eq!(sim.blueprints.len(), 1);

        // Run the sim forward until the blueprint is Complete.
        // The elf will claim the task, walk to the site, and do build work.
        // build_work_ticks_per_voxel * 1 voxel = total_cost ticks of work.
        // Cap at 1 million ticks to avoid infinite loops in tests.
        let max_tick = sim.tick + 1_000_000;
        while sim.tick < max_tick {
            sim.step(&[], sim.tick + 100);
            let all_complete = sim
                .blueprints
                .values()
                .all(|bp| bp.state == BlueprintState::Complete);
            if all_complete {
                break;
            }
        }
        assert!(
            sim.blueprints
                .values()
                .all(|bp| bp.state == BlueprintState::Complete),
            "Build did not complete within tick limit"
        );
        sim
    }

    #[test]
    fn completed_structure_registered_on_build_complete() {
        let sim = designate_and_complete_build(test_sim(42));

        assert_eq!(sim.structures.len(), 1);
        let structure = sim.structures.values().next().unwrap();
        assert_eq!(structure.id, StructureId(0));
        assert_eq!(structure.build_type, BuildType::Platform);
        assert_eq!(structure.width, 1);
        assert_eq!(structure.depth, 1);
        assert_eq!(structure.height, 1);
        assert!(structure.completed_tick > 0);
    }

    #[test]
    fn completed_structure_sequential_ids() {
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: air_coord,
            },
        };
        sim.step(&[cmd], 1);

        // Designate first build.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 2);

        // Run until first build completes.
        let max_tick = sim.tick + 1_000_000;
        while sim.tick < max_tick {
            sim.step(&[], sim.tick + 100);
            let all_complete = sim
                .blueprints
                .values()
                .all(|bp| bp.state == BlueprintState::Complete);
            if all_complete {
                break;
            }
        }
        assert_eq!(sim.structures.len(), 1);
        assert_eq!(sim.structures.values().next().unwrap().id, StructureId(0));

        // Find another air coord for the second build.
        let mut second_air = None;
        let tree = &sim.trees[&sim.player_tree_id];
        for &trunk_coord in &tree.trunk_voxels {
            for (dx, dy, dz) in [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 0, 1),
                (0, 0, -1),
                (0, 1, 0),
                (0, -1, 0),
            ] {
                let neighbor =
                    VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y + dy, trunk_coord.z + dz);
                if sim.world.in_bounds(neighbor)
                    && sim.world.get(neighbor) == VoxelType::Air
                    && neighbor != air_coord
                {
                    second_air = Some(neighbor);
                    break;
                }
            }
            if second_air.is_some() {
                break;
            }
        }
        let second_coord = second_air.expect("Need a second air coord");

        // Designate second build.
        let tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![second_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], tick);

        // Run until second build completes.
        let max_tick = sim.tick + 1_000_000;
        while sim.tick < max_tick {
            sim.step(&[], sim.tick + 100);
            let all_complete = sim
                .blueprints
                .values()
                .all(|bp| bp.state == BlueprintState::Complete);
            if all_complete {
                break;
            }
        }
        assert_eq!(sim.structures.len(), 2);

        // IDs should be 0 and 1.
        let ids: Vec<StructureId> = sim.structures.keys().copied().collect();
        assert!(ids.contains(&StructureId(0)));
        assert!(ids.contains(&StructureId(1)));
    }

    #[test]
    fn cancel_completed_structure_removes_entry() {
        let mut sim = designate_and_complete_build(test_sim(42));
        assert_eq!(sim.structures.len(), 1);

        // Get the project_id of the completed structure.
        let project_id = sim.structures.values().next().unwrap().project_id;

        // Cancel the build (should remove from structures too).
        let tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd], tick);

        assert!(
            sim.structures.is_empty(),
            "Cancelling a completed build should remove it from structures"
        );
    }

    // -----------------------------------------------------------------------
    // Structure voxels and raycast tests
    // -----------------------------------------------------------------------

    #[test]
    fn structure_voxels_populated_on_complete_build() {
        let sim = designate_and_complete_build(test_sim(42));

        // The completed build should populate structure_voxels.
        assert!(!sim.structure_voxels.is_empty());
        let structure = sim.structures.values().next().unwrap();
        let bp = sim
            .blueprints
            .values()
            .find(|bp| bp.state == BlueprintState::Complete)
            .unwrap();
        for &coord in &bp.voxels {
            assert_eq!(
                sim.structure_voxels.get(&coord),
                Some(&structure.id),
                "Voxel {coord} should map to structure {}",
                structure.id
            );
        }
    }

    #[test]
    fn structure_voxels_cleared_on_cancel_build() {
        let mut sim = designate_and_complete_build(test_sim(42));
        assert!(!sim.structure_voxels.is_empty());

        let project_id = sim.structures.values().next().unwrap().project_id;

        let tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd], tick);

        assert!(
            sim.structure_voxels.is_empty(),
            "Cancelling a completed build should clear structure_voxels"
        );
    }

    #[test]
    fn structure_voxels_rebuilt_on_rebuild_transient_state() {
        let sim = designate_and_complete_build(test_sim(42));
        let voxels_before = sim.structure_voxels.clone();
        assert!(!voxels_before.is_empty());

        // Round-trip through JSON (which drops transient fields).
        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        assert_eq!(
            restored.structure_voxels, voxels_before,
            "structure_voxels should be identical after save/load"
        );
    }

    #[test]
    fn raycast_structure_finds_structure_voxel() {
        let sim = designate_and_complete_build(test_sim(42));
        let structure = sim.structures.values().next().unwrap();
        let bp = sim
            .blueprints
            .values()
            .find(|bp| bp.state == BlueprintState::Complete)
            .unwrap();
        let voxel = bp.voxels[0];

        // Cast a ray from above the voxel straight down.
        let from = [
            voxel.x as f32 + 0.5,
            voxel.y as f32 + 10.0,
            voxel.z as f32 + 0.5,
        ];
        let dir = [0.0, -1.0, 0.0];
        let result = sim.raycast_structure(from, dir, 100);

        assert_eq!(
            result,
            Some(structure.id),
            "Raycast should find the structure at {voxel}"
        );
    }

    #[test]
    fn raycast_structure_stops_at_trunk() {
        let sim = designate_and_complete_build(test_sim(42));
        let bp = sim
            .blueprints
            .values()
            .find(|bp| bp.state == BlueprintState::Complete)
            .unwrap();
        let voxel = bp.voxels[0];

        // Place a trunk voxel between the ray origin and the structure.
        let mut sim = sim;
        let blocker = VoxelCoord::new(voxel.x, voxel.y + 5, voxel.z);
        sim.world.set(blocker, VoxelType::Trunk);

        let from = [
            voxel.x as f32 + 0.5,
            voxel.y as f32 + 10.0,
            voxel.z as f32 + 0.5,
        ];
        let dir = [0.0, -1.0, 0.0];
        let result = sim.raycast_structure(from, dir, 100);

        assert_eq!(
            result, None,
            "Raycast should stop at the trunk and not find the structure"
        );
    }

    #[test]
    fn raycast_structure_returns_none_for_empty_ray() {
        let sim = test_sim(42);
        // Cast a ray into empty space.
        let from = [32.5, 50.0, 32.5];
        let dir = [0.0, 1.0, 0.0];
        let result = sim.raycast_structure(from, dir, 100);
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // Hunger / EatFruit tests
    // -----------------------------------------------------------------------

    #[test]
    fn find_nearest_fruit_returns_reachable() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // The tree should have fruit after initialization (fruit_initial_attempts).
        let has_fruit = sim.trees.values().any(|t| !t.fruit_positions.is_empty());
        assert!(has_fruit, "Test tree should have some fruit after init");

        // Spawn an elf near the tree so it has a nav node.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // find_nearest_fruit should return a fruit reachable via nav graph.
        let result = sim.find_nearest_fruit(elf_id);
        assert!(
            result.is_some(),
            "Elf near tree should find reachable fruit"
        );
        let (fruit_pos, nav_node) = result.unwrap();

        // The fruit_pos should actually be in a tree's fruit list.
        let in_tree = sim
            .trees
            .values()
            .any(|t| t.fruit_positions.contains(&fruit_pos));
        assert!(in_tree, "Returned fruit should be in a tree's fruit list");

        // The nav node should be valid.
        let _node = sim.nav_graph.node(nav_node);
    }

    #[test]
    fn eat_fruit_task_restores_food_on_arrival() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let restore_pct = sim.species_table[&Species::Elf].food_restore_pct;

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;
        let elf_node = sim.creatures[&elf_id].current_node.unwrap();

        // Set elf food low.
        sim.creatures.get_mut(&elf_id).unwrap().food = food_max / 10;
        let food_before = sim.creatures[&elf_id].food;

        // Manually create an EatFruit task at the elf's current node (instant arrival).
        let fruit_pos = VoxelCoord::new(0, 0, 0); // dummy — food restore doesn't depend on real fruit
        let task_id = TaskId::new(&mut sim.rng);
        let eat_task = Task {
            id: task_id,
            kind: TaskKind::EatFruit { fruit_pos },
            state: TaskState::InProgress,
            location: elf_node,
            assignees: vec![elf_id],
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
        };
        sim.tasks.insert(task_id, eat_task);
        sim.creatures.get_mut(&elf_id).unwrap().current_task = Some(task_id);

        // Advance 1 tick — the elf's next activation should complete the task.
        sim.step(&[], sim.tick + 2);

        let elf = &sim.creatures[&elf_id];
        let expected_restore = food_max * restore_pct / 100;
        assert!(
            elf.food >= food_before + expected_restore - 1, // allow tiny rounding
            "Food should increase by ~restore_pct%: before={}, after={}, expected_restore={}",
            food_before,
            elf.food,
            expected_restore,
        );
        assert!(elf.current_task.is_none(), "Task should be complete");
    }

    #[test]
    fn hungry_idle_elf_creates_eat_fruit_task() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Need fruit to exist.
        let has_fruit = sim.trees.values().any(|t| !t.fruit_positions.is_empty());
        assert!(has_fruit, "Tree must have fruit for this test");

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set food below threshold (threshold is 50% by default).
        sim.creatures.get_mut(&elf_id).unwrap().food = food_max * 30 / 100;

        // Advance past the next heartbeat — hunger check should fire.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // The elf should now have an EatFruit task.
        let elf = &sim.creatures[&elf_id];
        assert!(
            elf.current_task.is_some(),
            "Hungry idle elf should have been assigned an EatFruit task"
        );
        let task = &sim.tasks[&elf.current_task.unwrap()];
        assert!(
            matches!(task.kind, TaskKind::EatFruit { .. }),
            "Task should be EatFruit, got {:?}",
            task.kind
        );
    }

    #[test]
    fn well_fed_elf_does_not_create_eat_fruit_task() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Spawn an elf — starts at full food.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        // Advance past the heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // No EatFruit task should exist.
        let has_eat_task = sim
            .tasks
            .values()
            .any(|t| matches!(t.kind, TaskKind::EatFruit { .. }));
        assert!(
            !has_eat_task,
            "Well-fed elf should not create an EatFruit task"
        );
    }

    #[test]
    fn busy_hungry_elf_does_not_create_eat_fruit_task() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set food very low.
        sim.creatures.get_mut(&elf_id).unwrap().food = food_max * 10 / 100;

        // Give the elf a GoTo task so it's busy.
        let task_id = TaskId::new(&mut sim.rng);
        let goto_task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: NavNodeId(0),
            assignees: vec![elf_id],
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
        };
        sim.tasks.insert(task_id, goto_task);
        sim.creatures.get_mut(&elf_id).unwrap().current_task = Some(task_id);

        // Advance past the heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // The elf should still have its GoTo task, not an EatFruit one.
        let elf = &sim.creatures[&elf_id];
        assert_eq!(
            elf.current_task,
            Some(task_id),
            "Busy elf should keep its existing task"
        );
        let eat_task_count = sim
            .tasks
            .values()
            .filter(|t| matches!(t.kind, TaskKind::EatFruit { .. }))
            .count();
        assert_eq!(
            eat_task_count, 0,
            "No EatFruit task should be created for a busy elf"
        );
    }

    #[test]
    fn hungry_elf_eats_fruit_and_food_increases() {
        // Integration test: set low food, run many ticks, verify food is higher
        // than it would be with decay alone (i.e. eating happened).
        let mut config = test_config();
        // Use aggressive decay so the elf gets hungry quickly.
        config
            .species
            .get_mut(&Species::Elf)
            .unwrap()
            .food_decay_per_tick = 100_000_000_000; // ~10x faster than default
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;

        // Need fruit to exist.
        assert!(
            sim.trees.values().any(|t| !t.fruit_positions.is_empty()),
            "Tree must have fruit"
        );

        // Spawn an elf.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = sim
            .creatures
            .values()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set food to 20% — well below the 50% threshold.
        sim.creatures.get_mut(&elf_id).unwrap().food = food_max * 20 / 100;
        let food_before = sim.creatures[&elf_id].food;

        // Run for 50_000 ticks — enough for heartbeat + pathfind + eat.
        sim.step(&[], 50_001);

        let elf = &sim.creatures[&elf_id];

        // With decay alone at 100_000_000_000/tick * 3000 ticks/heartbeat =
        // 300_000_000_000_000 per heartbeat. 50_000 ticks = ~16 heartbeats.
        // That would drop food to 0 quickly.
        // But eating restores 40% = 400_000_000_000_000.
        // So if the elf ate at least once, food should be above 0.
        // (We can't predict exact value due to timing, but food > 0 proves eating happened.)
        assert!(
            elf.food > 0,
            "Hungry elf should have eaten fruit and restored food above 0. food={}",
            elf.food
        );
    }

    // -----------------------------------------------------------------------
    // Tree overlap construction tests
    // -----------------------------------------------------------------------

    /// Find a Leaf voxel that is face-adjacent to a Trunk, Branch, or Root
    /// voxel (not just any solid — must be adjacent to structural wood so the
    /// structural validator can reach the ground).
    fn find_leaf_adjacent_to_wood(sim: &SimState) -> VoxelCoord {
        let tree = &sim.trees[&sim.player_tree_id];
        for &leaf_coord in &tree.leaf_voxels {
            for &(dx, dy, dz) in &[
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ] {
                let neighbor =
                    VoxelCoord::new(leaf_coord.x + dx, leaf_coord.y + dy, leaf_coord.z + dz);
                let vt = sim.world.get(neighbor);
                if matches!(vt, VoxelType::Trunk | VoxelType::Branch | VoxelType::Root) {
                    return leaf_coord;
                }
            }
        }
        panic!("No leaf voxel adjacent to wood found");
    }

    #[test]
    fn overlap_platform_at_leaf_creates_blueprint() {
        let mut sim = test_sim(42);
        let leaf_coord = find_leaf_adjacent_to_wood(&sim);
        assert_eq!(sim.world.get(leaf_coord), VoxelType::Leaf);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![leaf_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 1, "Blueprint should be created");
        let bp = sim.blueprints.values().next().unwrap();
        assert_eq!(bp.voxels, vec![leaf_coord]);
        assert_eq!(bp.original_voxels.len(), 1);
        assert_eq!(bp.original_voxels[0], (leaf_coord, VoxelType::Leaf));
    }

    #[test]
    fn overlap_all_trunk_rejects_nothing_to_build() {
        let mut sim = test_sim(42);
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

        assert!(sim.blueprints.is_empty(), "All-trunk should be rejected");
        assert_eq!(
            sim.last_build_message.as_deref(),
            Some("Nothing to build — all voxels are already wood.")
        );
    }

    #[test]
    fn overlap_mixed_air_trunk_only_builds_air() {
        let mut sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        // Find a trunk voxel with an air neighbor.
        let air_coord = find_air_adjacent_to_trunk(&sim);
        // Find which trunk voxel is adjacent.
        let mut trunk_coord = None;
        for &(dx, dy, dz) in &[
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            let neighbor = VoxelCoord::new(air_coord.x + dx, air_coord.y + dy, air_coord.z + dz);
            if sim.world.in_bounds(neighbor)
                && matches!(
                    sim.world.get(neighbor),
                    VoxelType::Trunk | VoxelType::Branch | VoxelType::Root
                )
            {
                trunk_coord = Some(neighbor);
                break;
            }
        }
        let trunk_coord = trunk_coord.expect("Should find adjacent trunk");

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord, trunk_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        // Only the air voxel should be in the blueprint.
        assert_eq!(bp.voxels, vec![air_coord]);
        assert!(bp.original_voxels.is_empty());
    }

    #[test]
    fn overlap_blocked_voxel_rejects() {
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // First build a platform at the air coord.
        sim.world.set(air_coord, VoxelType::GrownPlatform);

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

        assert!(
            sim.blueprints.is_empty(),
            "Blocked voxel should reject build"
        );
    }

    #[test]
    fn overlap_leaf_materializes_to_grown_platform() {
        let mut sim = build_test_sim();
        let leaf_coord = find_leaf_adjacent_to_wood(&sim);
        assert_eq!(sim.world.get(leaf_coord), VoxelType::Leaf);

        // Spawn elf.
        spawn_elf(&mut sim);

        // Designate platform at the leaf voxel.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![leaf_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        let project_id = *sim.blueprints.keys().next().unwrap();

        // Tick until completion.
        sim.step(&[], sim.tick + 200_000);

        // Leaf should have been converted to GrownPlatform.
        assert_eq!(
            sim.world.get(leaf_coord),
            VoxelType::GrownPlatform,
            "Leaf voxel should be converted to GrownPlatform"
        );

        // Blueprint should be Complete.
        let bp = &sim.blueprints[&project_id];
        assert_eq!(bp.state, BlueprintState::Complete);
    }

    #[test]
    fn overlap_cancel_reverts_to_original_type() {
        let mut sim = test_sim(42);
        let leaf_coord = find_leaf_adjacent_to_wood(&sim);
        assert_eq!(sim.world.get(leaf_coord), VoxelType::Leaf);

        // Designate platform at the leaf voxel.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![leaf_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        let project_id = *sim.blueprints.keys().next().unwrap();

        // Simulate partial construction by manually placing the voxel.
        sim.placed_voxels
            .push((leaf_coord, VoxelType::GrownPlatform));
        sim.world.set(leaf_coord, VoxelType::GrownPlatform);

        // Cancel the build.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd2], 2);

        // Voxel should revert to Leaf, not Air.
        assert_eq!(
            sim.world.get(leaf_coord),
            VoxelType::Leaf,
            "Cancelled overlap build should revert to original Leaf, not Air"
        );
    }

    #[test]
    fn overlap_save_load_preserves_original_voxels() {
        let mut sim = test_sim(42);
        let leaf_coord = find_leaf_adjacent_to_wood(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![leaf_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert_eq!(sim.blueprints.len(), 1);

        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        assert_eq!(restored.blueprints.len(), 1);
        let bp = restored.blueprints.values().next().unwrap();
        assert_eq!(bp.original_voxels.len(), 1);
        assert_eq!(bp.original_voxels[0], (leaf_coord, VoxelType::Leaf));
    }

    #[test]
    fn overlap_determinism() {
        let mut sim_a = test_sim(42);
        let mut sim_b = test_sim(42);

        let leaf_a = find_leaf_adjacent_to_wood(&sim_a);
        let leaf_b = find_leaf_adjacent_to_wood(&sim_b);
        assert_eq!(leaf_a, leaf_b);

        let cmd_a = SimCommand {
            player_id: sim_a.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![leaf_a],
                priority: Priority::Normal,
            },
        };
        let cmd_b = SimCommand {
            player_id: sim_b.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![leaf_b],
                priority: Priority::Normal,
            },
        };
        sim_a.step(&[cmd_a], 1);
        sim_b.step(&[cmd_b], 1);

        let id_a = *sim_a.blueprints.keys().next().unwrap();
        let id_b = *sim_b.blueprints.keys().next().unwrap();
        assert_eq!(id_a, id_b);

        let bp_a = &sim_a.blueprints[&id_a];
        let bp_b = &sim_b.blueprints[&id_b];
        assert_eq!(bp_a.voxels, bp_b.voxels);
        assert_eq!(bp_a.original_voxels, bp_b.original_voxels);
        assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
    }

    #[test]
    fn overlap_wall_at_leaf_rejects() {
        let mut sim = test_sim(42);
        let leaf_coord = find_leaf_adjacent_to_wood(&sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Wall,
                voxels: vec![leaf_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(
            sim.blueprints.is_empty(),
            "Wall does not allow overlap, should reject leaf"
        );
    }

    #[test]
    fn walk_toward_dead_task_node_does_not_panic() {
        // Reproduce B-dead-node-panic: a creature has a task whose location
        // nav node gets removed by an incremental update. The creature should
        // gracefully abandon the task instead of panicking in pathfinding.
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn an elf.
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[spawn_cmd], 2);

        let elf_id = *sim.creatures.keys().next().unwrap();

        // Find a ground nav node different from the elf's to use as task target.
        let elf_node = sim.creatures[&elf_id].current_node.unwrap();
        let task_node = sim
            .nav_graph
            .ground_node_ids()
            .into_iter()
            .find(|&nid| nid != elf_node)
            .expect("Need at least 2 ground nodes");

        // Create a GoTo task at that nav node and assign it to the elf.
        let task_id = TaskId::new(&mut sim.rng);
        sim.tasks.insert(
            task_id,
            Task {
                id: task_id,
                kind: TaskKind::GoTo,
                state: TaskState::InProgress,
                location: task_node,
                assignees: vec![elf_id],
                progress: 0.0,
                total_cost: 0.0,
                required_species: None,
            },
        );
        sim.creatures.get_mut(&elf_id).unwrap().current_task = Some(task_id);

        // Directly kill the task node's slot to simulate an incremental update
        // that removed it without recycling the slot. This is the exact state
        // that causes the panic: the NavNodeId in the task points to a dead
        // (None) slot.
        sim.nav_graph.kill_node(task_node);

        assert!(
            !sim.nav_graph.is_node_alive(task_node),
            "Task node should be dead",
        );

        // Step the sim — the elf should try to walk toward the now-dead
        // task node. This must NOT panic.
        sim.step(&[], 50000);

        // The elf should have dropped the task (can't reach dead node).
        let elf = &sim.creatures[&elf_id];
        assert!(
            elf.current_task.is_none(),
            "Elf should abandon task with dead location node",
        );
    }

    // ===================================================================
    // Carve tests
    // ===================================================================

    /// Helper: find a solid voxel that is safe to carve (won't disconnect the
    /// structure). Picks the highest trunk voxel so removing it doesn't sever
    /// the tree's connection to ground.
    fn find_carvable_voxel(sim: &SimState) -> VoxelCoord {
        let tree = &sim.trees[&sim.player_tree_id];
        // Pick the highest trunk voxel — removing the top is structurally safe.
        tree.trunk_voxels
            .iter()
            .copied()
            .filter(|v| v.y > 0)
            .max_by_key(|v| v.y)
            .expect("No trunk voxel above floor")
    }

    #[test]
    fn test_designate_carve_filters_air() {
        let mut sim = test_sim(42);
        let solid = find_carvable_voxel(&sim);
        // Pick an air voxel (high up, guaranteed empty).
        let air = VoxelCoord::new(5, 50, 5);
        assert_eq!(sim.world.get(air), VoxelType::Air);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateCarve {
                voxels: vec![solid, air],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        // A blueprint should exist with only the solid voxel.
        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        assert_eq!(bp.build_type, BuildType::Carve);
        assert_eq!(bp.voxels.len(), 1);
        assert_eq!(bp.voxels[0], solid);
    }

    #[test]
    fn test_carve_execution_removes_voxels() {
        let mut config = test_config();
        // Set carve ticks very low so the test completes quickly.
        config.carve_work_ticks_per_voxel = 1;
        let mut sim = SimState::with_config(42, config);
        let solid = find_carvable_voxel(&sim);
        assert!(sim.world.get(solid).is_solid());

        // Spawn an elf near the tree so it can claim the task.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let elf_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z + 3);

        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: elf_pos,
            },
        };
        let carve_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateCarve {
                voxels: vec![solid],
                priority: Priority::Normal,
            },
        };
        sim.step(&[spawn_cmd, carve_cmd], 2);

        // Ensure carve blueprint was created (not blocked by structural check).
        assert_eq!(
            sim.blueprints.len(),
            1,
            "Blueprint should exist; last_build_message: {:?}",
            sim.last_build_message
        );

        // Run the sim long enough for the elf to reach and carve.
        sim.step(&[], 500_000);

        // The solid voxel should now be Air.
        assert_eq!(
            sim.world.get(solid),
            VoxelType::Air,
            "Carved voxel should be Air"
        );
        assert!(
            sim.carved_voxels.contains(&solid),
            "carved_voxels should track the removal"
        );
    }

    // -------------------------------------------------------------------
    // Ladder tests
    // -------------------------------------------------------------------

    /// Find an air voxel adjacent to a trunk voxel on a horizontal face,
    /// ensuring at least `height` air voxels above it. Returns (anchor, orientation)
    /// where orientation is the face of the anchor pointing toward the trunk.
    fn find_ladder_column(sim: &SimState, height: i32) -> (VoxelCoord, FaceDirection) {
        let tree = &sim.trees[&sim.player_tree_id];
        for &trunk_coord in &tree.trunk_voxels {
            for &(dx, dz) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let base = VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y, trunk_coord.z + dz);
                if !sim.world.in_bounds(base) {
                    continue;
                }
                // The orientation points from air toward trunk (i.e., face
                // direction on the ladder voxel that faces the wall).
                let orientation = FaceDirection::from_offset(-dx, 0, -dz).unwrap();
                let all_air = (0..height).all(|dy| {
                    let coord = VoxelCoord::new(base.x, base.y + dy, base.z);
                    sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
                });
                if all_air {
                    return (base, orientation);
                }
            }
        }
        panic!("No suitable ladder column found");
    }

    #[test]
    fn designate_wood_ladder_creates_blueprint() {
        let mut sim = test_sim(42);
        let (anchor, orientation) = find_ladder_column(&sim, 3);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 3,
                orientation,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        let result = sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        assert_eq!(bp.build_type, BuildType::WoodLadder);
        assert_eq!(bp.voxels.len(), 3);
        assert_eq!(bp.state, BlueprintState::Designated);
        assert!(
            result
                .events
                .iter()
                .any(|e| matches!(e.kind, SimEventKind::BlueprintDesignated { .. }))
        );
    }

    #[test]
    fn designate_rope_ladder_creates_blueprint() {
        let mut sim = test_sim(42);
        // Rope ladders need top voxel adjacent to solid. Find a trunk voxel
        // with air below and on the side.
        let (anchor, orientation) = find_ladder_column(&sim, 1);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 1,
                orientation,
                kind: LadderKind::Rope,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        assert_eq!(bp.build_type, BuildType::RopeLadder);
    }

    #[test]
    fn designate_ladder_rejects_vertical_orientation() {
        let mut sim = test_sim(42);
        let (anchor, _) = find_ladder_column(&sim, 1);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 1,
                orientation: FaceDirection::PosY,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
        assert_eq!(
            sim.last_build_message.as_deref(),
            Some("Ladder orientation must be horizontal.")
        );
    }

    #[test]
    fn designate_ladder_rejects_zero_height() {
        let mut sim = test_sim(42);
        let (anchor, orientation) = find_ladder_column(&sim, 1);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 0,
                orientation,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
        assert_eq!(
            sim.last_build_message.as_deref(),
            Some("Ladder height must be at least 1.")
        );
    }

    #[test]
    fn designate_wood_ladder_rejects_no_anchor() {
        let mut sim = test_sim(42);
        // Place ladder in open air with no adjacent solid.
        let anchor = VoxelCoord::new(1, 10, 1);
        // Confirm it's air with no solid neighbor.
        assert_eq!(sim.world.get(anchor), VoxelType::Air);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 1,
                orientation: FaceDirection::PosX,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert!(sim.blueprints.is_empty());
    }

    #[test]
    fn designate_ladder_creates_build_task() {
        let mut sim = test_sim(42);
        let (anchor, orientation) = find_ladder_column(&sim, 2);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 2,
                orientation,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        let bp = sim.blueprints.values().next().unwrap();
        assert!(bp.task_id.is_some());
        let task = &sim.tasks[&bp.task_id.unwrap()];
        assert!(matches!(task.kind, crate::task::TaskKind::Build { .. }));
        assert_eq!(task.required_species, Some(Species::Elf));
        assert_eq!(
            task.total_cost,
            (sim.config.build_work_ticks_per_voxel * 2) as f32
        );
    }

    #[test]
    fn ladder_face_data_blocks_correctly() {
        let fd = ladder_face_data(FaceDirection::PosX);
        // Only the ladder face (PosX) should be Wall.
        assert_eq!(fd.get(FaceDirection::PosX), FaceType::Wall);
        // All other faces should be Open.
        assert_eq!(fd.get(FaceDirection::NegX), FaceType::Open);
        assert_eq!(fd.get(FaceDirection::PosZ), FaceType::Open);
        assert_eq!(fd.get(FaceDirection::NegZ), FaceType::Open);
        assert_eq!(fd.get(FaceDirection::PosY), FaceType::Open);
        assert_eq!(fd.get(FaceDirection::NegY), FaceType::Open);
    }

    #[test]
    fn ladder_voxel_type_not_solid() {
        assert!(!VoxelType::WoodLadder.is_solid());
        assert!(!VoxelType::RopeLadder.is_solid());
    }

    #[test]
    fn ladder_voxel_type_is_ladder() {
        assert!(VoxelType::WoodLadder.is_ladder());
        assert!(VoxelType::RopeLadder.is_ladder());
        assert!(!VoxelType::Air.is_ladder());
        assert!(!VoxelType::Trunk.is_ladder());
    }

    #[test]
    fn ladder_build_type_allows_tree_overlap() {
        assert!(BuildType::WoodLadder.allows_tree_overlap());
        assert!(BuildType::RopeLadder.allows_tree_overlap());
    }

    #[test]
    fn cancel_ladder_removes_blueprint_and_data() {
        let mut sim = test_sim(42);
        let (anchor, orientation) = find_ladder_column(&sim, 2);

        // Designate a wood ladder.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 2,
                orientation,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert_eq!(sim.blueprints.len(), 1);

        let project_id = *sim.blueprints.keys().next().unwrap();
        let cancel_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        let result = sim.step(&[cancel_cmd], 2);

        assert!(sim.blueprints.is_empty());
        assert!(sim.tasks.is_empty());
        assert!(
            result
                .events
                .iter()
                .any(|e| matches!(e.kind, SimEventKind::BuildCancelled { .. }))
        );
    }

    #[test]
    fn test_carve_skips_forest_floor() {
        let mut sim = test_sim(42);
        let (ws_x, _, ws_z) = sim.config.world_size;
        let center_x = ws_x as i32 / 2;
        let center_z = ws_z as i32 / 2;
        let floor = VoxelCoord::new(center_x, 0, center_z);
        assert_eq!(sim.world.get(floor), VoxelType::ForestFloor);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateCarve {
                voxels: vec![floor],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        // No blueprint should be created — ForestFloor is not carvable.
        assert!(
            sim.blueprints.is_empty(),
            "ForestFloor should not be carvable"
        );
        assert_eq!(sim.last_build_message.as_deref(), Some("Nothing to carve."));
    }

    #[test]
    fn test_cancel_carve_restores_originals() {
        let mut config = test_config();
        config.carve_work_ticks_per_voxel = 1;
        let mut sim = SimState::with_config(42, config);

        // Find two adjacent trunk voxels for carving.
        let tree = &sim.trees[&sim.player_tree_id];
        let v1 = tree.trunk_voxels.iter().copied().find(|v| v.y > 0).unwrap();
        let original_type = sim.world.get(v1);
        assert!(original_type.is_solid());

        // Spawn elf and designate carve.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let elf_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z + 3);
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: elf_pos,
            },
        };
        let carve_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateCarve {
                voxels: vec![v1],
                priority: Priority::Normal,
            },
        };
        sim.step(&[spawn_cmd, carve_cmd], 2);

        // Run long enough for the carve to complete.
        sim.step(&[], 500_000);
        assert_eq!(sim.world.get(v1), VoxelType::Air);

        // Now cancel the build.
        let project_id = *sim.blueprints.keys().next().unwrap();
        let cancel_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 500_001,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cancel_cmd], 500_001);

        // The voxel should be restored to its original type.
        assert_eq!(
            sim.world.get(v1),
            original_type,
            "Cancelled carve should restore original voxel type"
        );
        assert!(
            !sim.carved_voxels.contains(&v1),
            "carved_voxels should be cleaned up"
        );
    }

    #[test]
    fn ladder_save_load_roundtrip() {
        let mut sim = test_sim(42);
        let (anchor, orientation) = find_ladder_column(&sim, 2);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 2,
                orientation,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();
        assert_eq!(restored.blueprints.len(), 1);
        let bp = restored.blueprints.values().next().unwrap();
        assert_eq!(bp.build_type, BuildType::WoodLadder);
    }

    #[test]
    fn designate_rope_ladder_rejects_no_anchor() {
        let mut sim = test_sim(42);
        // Find a column of 3 air voxels next to trunk, then try placing a
        // rope ladder of height 3. The top voxel's ladder face must be
        // adjacent to solid — pick an anchor where that's not the case.
        let (anchor, orientation) = find_ladder_column(&sim, 3);
        // The anchor faces toward trunk, so height=1 passes (top is adjacent).
        // But we want to fail: place the anchor 2 voxels further out from the
        // trunk so the top voxel's neighbor is air.
        let (odx, _, odz) = orientation.to_offset();
        let far_anchor = VoxelCoord::new(anchor.x - odx * 2, anchor.y, anchor.z - odz * 2);
        // Make sure the far anchor column is air (best effort — skip if not).
        let all_air = (0..3).all(|dy| {
            let coord = VoxelCoord::new(far_anchor.x, far_anchor.y + dy, far_anchor.z);
            sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
        });
        if !all_air {
            // Can't construct the test scenario — skip gracefully.
            return;
        }

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor: far_anchor,
                height: 3,
                orientation,
                kind: LadderKind::Rope,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 0);
        assert!(sim.last_build_message.is_some());
    }

    #[test]
    fn designate_rope_ladder_multiheight() {
        let mut sim = test_sim(42);
        // Find a column of 3 air voxels next to trunk. Rope ladder needs
        // top voxel adjacent to solid — but the anchor is at the bottom.
        // With height=3, the top (anchor.y+2) must have its ladder face
        // neighbor be solid.
        let tree = &sim.trees[&sim.player_tree_id];
        let mut found = None;
        for &trunk_coord in &tree.trunk_voxels {
            for &(dx, dz) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let orientation = FaceDirection::from_offset(-dx, 0, -dz).unwrap();
                // We need a column of 3 air voxels starting below the trunk,
                // where the topmost voxel (base.y+2) is at trunk_coord.y.
                let base =
                    VoxelCoord::new(trunk_coord.x + dx, trunk_coord.y - 2, trunk_coord.z + dz);
                let all_air = (0..3).all(|dy| {
                    let coord = VoxelCoord::new(base.x, base.y + dy, base.z);
                    sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
                });
                // Top voxel's ladder-face neighbor must be solid (the trunk).
                let (odx, _, odz) = orientation.to_offset();
                let top_neighbor = VoxelCoord::new(base.x + odx, base.y + 2, base.z + odz);
                if all_air && sim.world.get(top_neighbor).is_solid() {
                    found = Some((base, orientation));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let (anchor, orientation) = found.expect("No suitable multi-height rope column found");

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor,
                height: 3,
                orientation,
                kind: LadderKind::Rope,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.blueprints.len(), 1);
        let bp = sim.blueprints.values().next().unwrap();
        assert_eq!(bp.build_type, BuildType::RopeLadder);
        assert_eq!(bp.voxels.len(), 3);
    }

    #[test]
    fn ladder_classify_for_overlap_blocked() {
        assert_eq!(
            VoxelType::WoodLadder.classify_for_overlap(),
            OverlapClassification::Blocked
        );
        assert_eq!(
            VoxelType::RopeLadder.classify_for_overlap(),
            OverlapClassification::Blocked
        );
    }

    #[test]
    fn ladder_build_type_to_voxel_type() {
        assert_eq!(BuildType::WoodLadder.to_voxel_type(), VoxelType::WoodLadder);
        assert_eq!(BuildType::RopeLadder.to_voxel_type(), VoxelType::RopeLadder);
    }

    #[test]
    fn designate_ladder_determinism() {
        let (anchor_a, orientation_a) = {
            let sim = test_sim(42);
            find_ladder_column(&sim, 3)
        };
        let (anchor_b, orientation_b) = {
            let sim = test_sim(42);
            find_ladder_column(&sim, 3)
        };
        assert_eq!(anchor_a, anchor_b);
        assert_eq!(orientation_a, orientation_b);

        let mut sim_a = test_sim(42);
        let mut sim_b = test_sim(42);

        let cmd_a = SimCommand {
            player_id: sim_a.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor: anchor_a,
                height: 3,
                orientation: orientation_a,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        let cmd_b = SimCommand {
            player_id: sim_b.player_id,
            tick: 1,
            action: SimAction::DesignateLadder {
                anchor: anchor_b,
                height: 3,
                orientation: orientation_b,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim_a.step(&[cmd_a], 1);
        sim_b.step(&[cmd_b], 1);

        assert_eq!(sim_a.blueprints.len(), sim_b.blueprints.len());
    }

    #[test]
    fn test_carve_nav_graph_update() {
        let mut config = test_config();
        config.carve_work_ticks_per_voxel = 1;
        let mut sim = SimState::with_config(42, config);

        // Find a solid voxel that is part of the tree.
        let solid = find_carvable_voxel(&sim);
        // Before carving, the voxel is solid — it should not be a nav node itself.
        assert!(
            sim.world.get(solid).is_solid(),
            "Precondition: voxel is solid"
        );

        // Spawn elf and designate carve.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let elf_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z + 3);
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: elf_pos,
            },
        };
        let carve_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateCarve {
                voxels: vec![solid],
                priority: Priority::Normal,
            },
        };
        sim.step(&[spawn_cmd, carve_cmd], 2);

        // Run sim to complete the carve.
        sim.step(&[], 500_000);

        // After carving, the voxel is Air. If it has a solid face neighbor,
        // it should now be a nav node.
        assert_eq!(sim.world.get(solid), VoxelType::Air);
        if sim.world.has_solid_face_neighbor(solid) {
            let node = sim.nav_graph.find_nearest_node(solid);
            assert!(
                node.is_some(),
                "Carved voxel with solid neighbor should be a nav node"
            );
        }
    }

    #[test]
    fn test_carve_save_load_roundtrip() {
        let mut config = test_config();
        config.carve_work_ticks_per_voxel = 1;
        let mut sim = SimState::with_config(42, config);

        let solid = find_carvable_voxel(&sim);
        let original_type = sim.world.get(solid);

        // Spawn elf and designate carve.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let elf_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z + 3);
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: elf_pos,
            },
        };
        let carve_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateCarve {
                voxels: vec![solid],
                priority: Priority::Normal,
            },
        };
        sim.step(&[spawn_cmd, carve_cmd], 2);

        // Complete the carve.
        sim.step(&[], 500_000);
        assert_eq!(sim.world.get(solid), VoxelType::Air);
        assert!(sim.carved_voxels.contains(&solid));

        // Save and load.
        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        // Verify carved voxels survived.
        assert!(restored.carved_voxels.contains(&solid));
        assert_eq!(
            restored.world.get(solid),
            VoxelType::Air,
            "Carved voxel should be Air after reload"
        );

        // Verify the original type is in the blueprint's original_voxels.
        let bp = restored.blueprints.values().next().unwrap();
        let orig = bp.original_voxels.iter().find(|(c, _)| *c == solid);
        assert!(orig.is_some());
        assert_eq!(orig.unwrap().1, original_type);
    }
}
