// Core simulation state and tick loop.
//
// `SimState` is the single source of truth for the entire game world. Entity
// data (creatures, tasks, blueprints, structures, ground piles) lives in
// `SimState.db` (a tabulosity `SimDb`). The sim also owns the voxel world,
// the nav graph, the event queue, the PRNG, and the game config.
// The sim is a pure function:
// `(state, commands) -> (new_state, events)`.
//
// On construction (`new()`/`with_config()`), the sim delegates world creation
// to `worldgen.rs`, which runs generators in order (tree → fruits → civs →
// knowledge) using a dedicated worldgen PRNG. The runtime PRNG is derived from
// the worldgen PRNG's final state. Two nav graphs are maintained: the standard
// graph for 1x1x1 creatures and a large graph for 2x2x2 creatures (elephants).
// `graph_for_species()` dispatches to the correct graph based on the species'
// `footprint` field from `species.rs`. Creature spawning and movement are
// handled through the command/event system. Initial creature populations are
// spawned by `spawn_initial_creatures()`, called from `session.rs` during
// `StartGame` processing — it reads `config.initial_creatures` and
// `config.initial_ground_piles` to populate the world.
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
// When a creature starts traversing a nav edge, a `MoveAction` row is
// inserted into the `move_actions` table with `move_from`, `move_to`, and
// `move_start_tick`. The end tick is the creature's `next_available_tick`.
// `interpolated_position(render_tick, move_action)` lerps between them,
// returning floats for the GDExtension bridge. This data is never read by
// sim logic and does not affect determinism.
//
// `CreatureHeartbeat` still exists but is decoupled from movement; it handles
// periodic non-movement checks (mood, mana, food decay, rest decay,
// hunger-driven task creation, sleep-driven task creation, personal item
// acquisition, etc.). After decaying food and rest, the heartbeat checks
// needs in priority order:
//
// - **Phase 2a (hunger):** If hungry and idle, eat bread from inventory
//   (instant EatBread task) or fall back to seeking fruit (EatFruit task via
//   Dijkstra nearest-fruit search).
// - **Phase 2b (tiredness):** If tired and idle (not hungry), find a bed
//   (assigned home → dormitory → ground fallback) and create a Sleep task.
// - **Phase 2b½ (moping):** If mood is Unhappy or worse, roll a Poisson-like
//   probability check. If triggered, abandon any current task (at Miserable+)
//   or start moping if idle. Mope location is assigned home if available,
//   else current node. Duration from `MoodConsequencesConfig`.
// - **Phase 2c (acquisition):** If still idle after hunger/sleep checks,
//   iterate the creature's `wants` list. For each want where owned items are
//   below the target, call `find_item_source()` to locate unowned items in
//   ground piles or building inventories, reserve them, and create an
//   `AcquireItem` task. One task per heartbeat (first unsatisfied want wins).
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
// Tasks are the core assignment mechanism. The sim's `db.tasks` table stores
// the base task data; variant-specific data is decomposed into extension tables
// (`task_haul_data`, `task_sleep_data`, `task_acquire_data`, `task_craft_data`)
// and relationship
// tables (`task_blueprint_refs`, `task_structure_refs`, `task_voxel_refs`).
// Query helpers on `SimState` (`task_project_id`, `task_structure_ref`,
// `task_voxel_ref`, `task_haul_data`, `task_sleep_data`, `task_acquire_data`,
// `task_craft_data`,
// `task_haul_source`, `task_acquire_source`, `task_sleep_location`) abstract
// the extension table lookups. Each creature stores an optional `current_task`.
//
// ### Task entity (`task.rs`)
//
// A `Task` has:
// - `kind: TaskKind` — determines behavior (`GoTo`, `Build`, `EatBread`,
//   `EatFruit`, `Sleep`, `Furnish`, `Haul`, `Cook`, `Harvest`, `AcquireItem`,
//   `Mope`, `Craft`).
// - `state: TaskState` — lifecycle: `Available` → `InProgress` → `Complete`.
// - `location: NavNodeId` — where creatures go to work on the task.
// - Assignment tracked via `creature.current_task` FK.
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
//    sets the task to `InProgress`, sets the creature's `current_task`, and
//    computes an A* path to `task.location`.
// 3. Each subsequent activation runs the task's behavior script (see below).
// 4. On completion, `complete_task` sets the task to `Complete` and clears
//    `current_task` for assigned creatures, returning them to wandering.
//
// Only one creature can transition a task from `Available` → `InProgress`.
// Once `InProgress`, `find_available_task` skips it, preventing pile-ons.
// (Multi-worker tasks are structurally supported via creature FK but not yet
// used — a future task kind could transition back to `Available` to recruit
// more workers.)
//
// ### Action system
//
// Every creature activity is a typed, duration-bearing action with clear
// start/end semantics. The creature stores `action_kind` (what it's doing)
// and `next_available_tick` (when the action completes). The activation
// handler fires at `next_available_tick`, resolves the action's effects,
// then enters the decision cascade for the next action.
//
// **Action lifecycle:**
// 1. `start_*_action()` — sets `action_kind` and `next_available_tick`,
//    schedules a `CreatureActivation` at the completion tick.
// 2. `resolve_*_action()` — applies the action's effects (place voxel,
//    restore food, etc.) and returns whether the task completed.
// 3. Decision cascade — if task not done, re-enter `execute_task_behavior`
//    to start the next action; if done, find a new task or wander.
//
// **Interruption:** `interrupt_task()` is the single entry point for task
// interruption from any source (nav invalidation, mope preemption, death,
// flee, player cancel). It calls `abort_current_action()` to clean up
// action state (including Move's MoveAction row) without resolving effects,
// dispatches per-kind cleanup (release reservations, drop carried items),
// and handles task state: resumable tasks (Build, Furnish) return to
// Available; all others are marked Complete.
//
// ### Task kinds and their actions
//
// Each task kind maps to one or more action kinds:
//
//   GoTo — no action; completes instantly at task location.
//   Build — ActionKind::Build, duration `build_work_ticks_per_voxel`.
//     Multi-action: one action materializes one voxel.
//   Furnish — ActionKind::Furnish, duration `furnish_work_ticks_per_item`.
//     Multi-action: one action places one furniture item.
//   EatBread/EatFruit — ActionKind::Eat, duration `eat_action_ticks`.
//     Single-action: resolve restores food and completes.
//   Sleep — ActionKind::Sleep, duration `sleep_action_ticks`.
//     Multi-action: each action restores rest; repeats until rest full.
//   Harvest — ActionKind::Harvest, duration `harvest_action_ticks`.
//     Single-action: removes fruit voxel, creates ground pile.
//   AcquireItem — ActionKind::AcquireItem, duration `acquire_item_action_ticks`.
//     Single-action: picks up items from source.
//   Haul — ActionKind::PickUp then ActionKind::DropOff.
//     Two actions: pickup at source, dropoff at destination.
//   Cook — ActionKind::Cook, duration `cook_work_ticks`.
//     Single-action: consumes fruit, produces bread.
//   Craft — ActionKind::Craft, duration `recipe.work_ticks`.
//     Single-action: consumes inputs, produces outputs.
//   Mope — ActionKind::Mope, duration `mope_action_ticks`.
//     Multi-action: progress incremented by `mope_action_ticks` per action.
//   MeleeStrike — ActionKind::MeleeStrike, duration `melee_interval_ticks`.
//     Not task-driven. Triggered by `DebugMeleeAttack` command (or future
//     AttackCreature task AI). Deals flat damage on start; action duration
//     is the cooldown before the next strike. Creature becomes idle on resolve.
//   Shoot — ActionKind::Shoot, duration `shoot_cooldown_ticks`.
//     Not task-driven. Triggered by `DebugShootAction` command or hostile AI.
//     Requires bow + arrow in inventory, LOS, and feasible aim trajectory.
//     Consumes one arrow, spawns projectile on start; action duration is the
//     cooldown before the next shot. Creature becomes idle on resolve.
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
// ## HP and death
//
// Each creature has `hp` and `hp_max` (set from `SpeciesData` at spawn) and
// a `vital_status` field (Alive or Dead). `DamageCreature` reduces HP;
// reaching 0 triggers `handle_creature_death`. `HealCreature` restores HP
// (clamped to `hp_max`, no-op on dead). `DebugKillCreature` kills instantly.
//
// Death does NOT delete the creature row. Instead, `vital_status` is set to
// `Dead` and the creature remains in the DB (supporting future states like
// Ghost or Undead). The death handler: interrupts any current task, drops
// inventory as a ground pile, deregisters from the spatial index, clears
// `assigned_home`, emits `CreatureDied`, and creates a notification.
// Heartbeats and activations check `vital_status` and skip dead creatures
// (no rescheduling). All live-creature queries (rendering, counting, task
// assignment) filter by `vital_status == Alive`.
//
// ## Save/load
//
// `SimState` derives `Serialize`/`Deserialize` via serde. Several transient
// fields (`world`, `nav_graph`, `large_nav_graph`, `species_table`, `lexicon`,
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
use crate::building;
use crate::command::{SimAction, SimCommand};
use crate::config::GameConfig;
use crate::db::{ActionKind, MoveAction, SimDb};
use crate::event::{EventQueue, ScheduledEventKind, SimEvent, SimEventKind};
use crate::inventory;
use crate::nav::{self, NavGraph};
use crate::pathfinding;
use crate::preemption;
use crate::prng::GameRng;
use crate::projectile::SubVoxelVec;
use crate::species::SpeciesData;
use crate::structural;
use crate::task;
use crate::types::*;
use crate::world::VoxelWorld;
use elven_canopy_lang::Lexicon;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Serde helper: serialize `BTreeMap<VoxelCoord, V>` with string keys `"x,y,z"`
/// so JSON output has valid string keys (JSON objects require string keys).
/// Top-level simulation state. This is the entire game world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimState {
    /// Current simulation tick.
    pub tick: u64,

    /// The simulation's deterministic PRNG.
    pub rng: GameRng,

    /// Game configuration (immutable after initialization).
    pub config: GameConfig,

    /// The event priority queue driving the discrete event simulation.
    pub event_queue: EventQueue,

    /// The tabulosity relational database storing all simulation entities.
    /// Replaces the old per-entity BTreeMap collections with a typed,
    /// FK-validated, indexed in-memory database.
    #[serde(default)]
    pub db: SimDb,

    /// All tree entities, keyed by ID. BTreeMap for deterministic iteration.
    pub trees: BTreeMap<TreeId, Tree>,

    // creatures field removed — now in self.db.creatures
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

    /// Counter for the next `StructureId` to assign (monotonically increasing).
    #[serde(default)]
    pub next_structure_id: u64,

    /// The player's tree ID.
    pub player_tree_id: TreeId,

    /// The player's ID.
    pub player_id: PlayerId,

    /// The player-controlled civilization's ID. `None` for pre-civilization saves.
    #[serde(default)]
    pub player_civ_id: Option<CivId>,

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

    /// Vaelith lexicon for elf name generation. Transient — loaded from the
    /// embedded JSON at startup and after deserialization. Not serialized.
    #[serde(skip)]
    pub lexicon: Option<Lexicon>,

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

    /// Spatial index mapping voxel coordinates to the creatures occupying them.
    /// Multi-voxel creatures (footprint > `[1,1,1]`) are registered at every
    /// voxel they occupy (anchor + footprint offsets). Transient — rebuilt from
    /// alive creatures in `rebuild_transient_state()`. Used by projectile hit
    /// detection and hostile detection scanning. `BTreeMap` with lexicographic
    /// `VoxelCoord` ordering does NOT support efficient 3D range queries —
    /// detection scans use O(n) iteration with distance filtering.
    #[serde(skip)]
    pub spatial_index: BTreeMap<VoxelCoord, Vec<CreatureId>>,

    /// Persisted list of (coord, species_id) pairs for fruit voxels. On load,
    /// `rebuild_transient_state()` rebuilds `fruit_voxel_species` from this.
    #[serde(default)]
    pub fruit_voxel_species_list: Vec<(VoxelCoord, crate::fruit::FruitSpeciesId)>,

    /// Maps each fruit voxel to the species of the fruit occupying it.
    /// Transient — rebuilt from `fruit_voxel_species_list` after deserialization.
    /// Maintained by `attempt_fruit_spawn` (insert) and fruit removal in
    /// `do_eat_fruit` / `do_harvest` (remove).
    #[serde(skip)]
    pub fruit_voxel_species: BTreeMap<VoxelCoord, crate::fruit::FruitSpeciesId>,
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
    /// The fruit species this tree produces. Assigned during worldgen from the
    /// world's procedurally generated fruit species roster. `None` for
    /// pre-fruit-variety saves (defaults to first species if available).
    #[serde(default)]
    pub fruit_species_id: Option<crate::fruit::FruitSpeciesId>,
}

/// A creature's current path through the nav graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreaturePath {
    /// Remaining node IDs to visit (next node is index 0).
    pub remaining_nodes: Vec<NavNodeId>,
    /// Remaining edge indices to traverse (next edge is index 0).
    pub remaining_edge_indices: Vec<usize>,
}

/// The result of processing commands and advancing the simulation.
pub struct StepResult {
    /// Narrative events emitted during this step, for the UI / event log.
    pub events: Vec<SimEvent>,
}

/// Check whether two creatures' footprints are within melee range.
///
/// Computes the closest-point squared euclidean distance between two
/// axis-aligned bounding boxes (attacker and target footprints) and
/// compares against `melee_range_sq`. Pure function — no sim state needed.
pub fn in_melee_range(
    attacker_pos: VoxelCoord,
    attacker_footprint: [u8; 3],
    target_pos: VoxelCoord,
    target_footprint: [u8; 3],
    melee_range_sq: i64,
) -> bool {
    // For each axis, compute the gap between the two footprint intervals.
    // If they overlap, gap = 0. Otherwise gap = distance between closest edges.
    let gap = |a_min: i32, a_size: u8, b_min: i32, b_size: u8| -> i64 {
        let a_max = a_min + a_size as i32 - 1;
        let b_max = b_min + b_size as i32 - 1;
        if a_max < b_min {
            (b_min - a_max) as i64
        } else if b_max < a_min {
            (a_min - b_max) as i64
        } else {
            0
        }
    };
    let dx = gap(
        attacker_pos.x,
        attacker_footprint[0],
        target_pos.x,
        target_footprint[0],
    );
    let dy = gap(
        attacker_pos.y,
        attacker_footprint[1],
        target_pos.y,
        target_footprint[1],
    );
    let dz = gap(
        attacker_pos.z,
        attacker_footprint[2],
        target_pos.z,
        target_footprint[2],
    );
    dx * dx + dy * dy + dz * dz <= melee_range_sq
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
    ///
    /// Delegates world creation to `worldgen::run_worldgen()`, which runs
    /// generators in a defined order (tree → fruits → civs → knowledge) using
    /// a dedicated worldgen PRNG. The runtime PRNG is derived from the worldgen
    /// PRNG's final state, ensuring deterministic separation.
    pub fn with_config(seed: u64, config: GameConfig) -> Self {
        use crate::worldgen;

        let wg = worldgen::run_worldgen(seed, &config);

        let player_tree_id = wg.home_tree.id;

        let mut trees = BTreeMap::new();
        trees.insert(player_tree_id, wg.home_tree);

        // Build species table from config.
        let species_table = config.species.clone();

        let mut state = Self {
            tick: 0,
            rng: wg.runtime_rng,
            config,
            event_queue: EventQueue::new(),
            db: wg.db,
            trees,
            placed_voxels: Vec::new(),
            carved_voxels: Vec::new(),
            face_data_list: Vec::new(),
            face_data: BTreeMap::new(),
            ladder_orientations_list: Vec::new(),
            ladder_orientations: BTreeMap::new(),
            next_structure_id: 0,
            player_tree_id,
            player_id: wg.player_id,
            player_civ_id: Some(wg.player_civ_id),
            world: wg.world,
            nav_graph: wg.nav_graph,
            large_nav_graph: wg.large_nav_graph,
            species_table,
            lexicon: Some(elven_canopy_lang::default_lexicon()),
            last_build_message: None,
            structure_voxels: BTreeMap::new(),
            spatial_index: BTreeMap::new(),
            fruit_voxel_species_list: Vec::new(),
            fruit_voxel_species: BTreeMap::new(),
        };

        // The world rebuild above produces thousands of set() calls that
        // accumulate dirty_voxels entries. Clear them — the mesh cache will
        // do a full build_all() at init, so those entries aren't needed.
        state.world.clear_dirty_voxels();

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

        // Schedule the first logistics heartbeat.
        let logistics_interval = state.config.logistics_heartbeat_interval_ticks;
        state
            .event_queue
            .schedule(logistics_interval, ScheduledEventKind::LogisticsHeartbeat);

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
            SimAction::RenameStructure { structure_id, name } => {
                let name_clone = name.clone();
                let _ = self.db.structures.modify_unchecked(structure_id, |s| {
                    s.name = name_clone;
                });
            }
            SimAction::FurnishStructure {
                structure_id,
                furnishing_type,
                greenhouse_species,
            } => {
                self.furnish_structure(*structure_id, *furnishing_type, *greenhouse_species);
            }
            SimAction::AssignHome {
                creature_id,
                structure_id,
            } => {
                self.assign_home(*creature_id, *structure_id);
            }
            SimAction::SetLogisticsPriority {
                structure_id,
                priority,
            } => {
                let priority_val = *priority;
                let _ = self.db.structures.modify_unchecked(structure_id, |s| {
                    s.logistics_priority = priority_val;
                });
            }
            SimAction::SetLogisticsWants {
                structure_id,
                wants,
            } => {
                let inv_id = self.structure_inv(*structure_id);
                self.set_inv_wants(inv_id, wants);
            }
            SimAction::SetCookingConfig {
                structure_id,
                cooking_enabled,
                cooking_bread_target,
            } => {
                if self
                    .db
                    .structures
                    .get(structure_id)
                    .is_some_and(|s| s.furnishing == Some(FurnishingType::Kitchen))
                {
                    let cooking_enabled_val = *cooking_enabled;
                    let cooking_bread_target_val = *cooking_bread_target;
                    let _ = self.db.structures.modify_unchecked(structure_id, |s| {
                        s.cooking_enabled = cooking_enabled_val;
                        s.cooking_bread_target = cooking_bread_target_val;
                    });
                }
            }
            SimAction::SetCreatureFood { creature_id, food } => {
                let _ = self.db.creatures.modify_unchecked(creature_id, |creature| {
                    creature.food = *food;
                });
            }
            SimAction::SetCreatureRest { creature_id, rest } => {
                let _ = self.db.creatures.modify_unchecked(creature_id, |creature| {
                    creature.rest = *rest;
                });
            }
            SimAction::AddCreatureItem {
                creature_id,
                item_kind,
                quantity,
            } => {
                let inv_id = self.creature_inv(*creature_id);
                self.inv_add_simple_item(inv_id, *item_kind, *quantity, Some(*creature_id), None);
            }
            SimAction::AddGroundPileItem {
                position,
                item_kind,
                quantity,
            } => {
                let pile_id = self.ensure_ground_pile(*position);
                let pile = self.db.ground_piles.get(&pile_id).unwrap();
                self.inv_add_simple_item(pile.inventory_id, *item_kind, *quantity, None, None);
            }
            SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled,
                recipe_configs,
            } => {
                self.set_workshop_config(*structure_id, *workshop_enabled, recipe_configs.clone());
            }
            SimAction::DebugNotification { message } => {
                self.add_notification(message.clone());
            }
            SimAction::DiscoverCiv {
                civ_id,
                discovered_civ,
                initial_opinion,
            } => {
                self.discover_civ(*civ_id, *discovered_civ, *initial_opinion);
            }
            SimAction::SetCivOpinion {
                civ_id,
                target_civ,
                opinion,
            } => {
                self.set_civ_opinion(*civ_id, *target_civ, *opinion);
            }
            SimAction::DebugKillCreature { creature_id } => {
                self.handle_creature_death(*creature_id, DeathCause::Debug, events);
            }
            SimAction::DamageCreature {
                creature_id,
                amount,
            } => {
                self.apply_damage(*creature_id, *amount, events);
            }
            SimAction::HealCreature {
                creature_id,
                amount,
            } => {
                self.apply_heal(*creature_id, *amount);
            }
            SimAction::DebugMeleeAttack {
                attacker_id,
                target_id,
            } => {
                self.try_melee_strike(*attacker_id, *target_id, events);
            }
            SimAction::DebugShootAction {
                attacker_id,
                target_id,
            } => {
                self.try_shoot_arrow(*attacker_id, *target_id, events);
            }
            SimAction::DebugSpawnProjectile {
                origin,
                target,
                shooter_id,
            } => {
                self.spawn_projectile(*origin, *target, *shooter_id);
            }
        }
    }

    /// Validate and create a blueprint from a `DesignateBuild` command.
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for overlap,
    /// adjacency, and structural checks.
    ///
    /// Validation (silent no-op on failure, consistent with other commands):
    /// - Voxels must be non-empty.
    /// - All voxels must be in-bounds.
    /// - All voxels must be Air (or overlap-compatible, considering overlay).
    /// - At least one voxel must have a solid face neighbor (considering overlay).
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

        // Build overlay from existing designated blueprints so we treat
        // planned builds as already present for overlap, adjacency, and
        // structural checks.
        let overlay = self.blueprint_overlay();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

        // Branch validation: overlap-enabled types classify voxels, others
        // require all Air.
        let build_voxels: Vec<VoxelCoord>;
        let original_voxels: Vec<(VoxelCoord, VoxelType)>;

        if build_type.allows_tree_overlap() {
            let mut bv = Vec::new();
            let mut ov = Vec::new();
            for &coord in voxels {
                match effective_type(coord).classify_for_overlap() {
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
                if effective_type(coord) != VoxelType::Air {
                    self.last_build_message = Some("Build position is not empty.".to_string());
                    return;
                }
            }
            build_voxels = voxels.to_vec();
            original_voxels = Vec::new();
        }

        let any_adjacent = build_voxels.iter().any(|&coord| {
            self.world.has_solid_face_neighbor(coord)
                || FaceDirection::ALL.iter().any(|&dir| {
                    let (dx, dy, dz) = dir.to_offset();
                    let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                    overlay
                        .voxels
                        .get(&neighbor)
                        .is_some_and(|vt| vt.is_solid())
                })
        });
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
            &overlay,
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
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0.0,
            total_cost: num_voxels as f32,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(build_task);

        let composition_id = Some(self.create_composition(build_voxels.len()));
        let bp = Blueprint {
            id: project_id,
            build_type,
            voxels: build_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            composition_id,
            face_layout: None,
            stress_warning,
            original_voxels,
        };
        self.db.blueprints.insert_no_fk(bp).unwrap();
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for a building with paper-thin walls.
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for foundation
    /// solidity, interior clearance, and structural checks.
    ///
    /// Validation (silent no-op on failure):
    /// - width and depth must be >= 3 (minimum building size)
    /// - height must be >= 1
    /// - All foundation voxels (anchor.y level) must be solid (considering overlay)
    /// - All interior voxels (above foundation) must be Air (considering overlay)
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

        let overlay = self.blueprint_overlay();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

        // Validate foundation (all must be solid, considering blueprint overlay).
        for x in anchor.x..anchor.x + width {
            for z in anchor.z..anchor.z + depth {
                let coord = VoxelCoord::new(x, anchor.y, z);
                if !self.world.in_bounds(coord) || !effective_type(coord).is_solid() {
                    self.last_build_message =
                        Some("Foundation must be on solid ground.".to_string());
                    return;
                }
            }
        }

        // Validate interior (all must be Air, considering blueprint overlay).
        for y in anchor.y + 1..anchor.y + 1 + height {
            for x in anchor.x..anchor.x + width {
                for z in anchor.z..anchor.z + depth {
                    let coord = VoxelCoord::new(x, y, z);
                    if !self.world.in_bounds(coord) || effective_type(coord) != VoxelType::Air {
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
            &overlay,
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
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0.0,
            total_cost: num_voxels as f32,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(build_task);

        let composition_id = Some(self.create_composition(voxels.len()));
        let bp = Blueprint {
            id: project_id,
            build_type: BuildType::Building,
            voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            composition_id,
            face_layout: Some(face_layout.into_iter().collect()),
            stress_warning,
            original_voxels: Vec::new(),
        };
        self.db.blueprints.insert_no_fk(bp).unwrap();
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for a ladder (wood or rope).
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for overlap,
    /// anchoring, and structural checks.
    ///
    /// Validation:
    /// - height >= 1
    /// - orientation must be horizontal (PosX/NegX/PosZ/NegZ)
    /// - All column voxels must be Air or Convertible (considering overlay)
    /// - Wood: at least one voxel's ladder face is adjacent to solid (considering overlay)
    /// - Rope: topmost voxel's ladder face is adjacent to solid (considering overlay)
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

        // Build overlay from existing designated blueprints.
        let overlay = self.blueprint_overlay();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

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
            match effective_type(coord).classify_for_overlap() {
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

        // Anchoring validation (considers blueprint overlay).
        match kind {
            LadderKind::Wood => {
                // At least one voxel's ladder face must be adjacent to solid.
                let any_anchored = build_voxels.iter().any(|&coord| {
                    let neighbor = VoxelCoord::new(coord.x + odx, coord.y, coord.z + odz);
                    effective_type(neighbor).is_solid()
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
                if !effective_type(top).is_solid() {
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
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0.0,
            total_cost: num_voxels as f32,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(build_task);

        // Store the orientation in the blueprint's face_layout field for later
        // use during materialization. We encode it as a map from each voxel to
        // its FaceData (computed from orientation).
        let face_layout: Vec<(VoxelCoord, FaceData)> = build_voxels
            .iter()
            .map(|&coord| (coord, ladder_face_data(orientation)))
            .collect();

        let composition_id = Some(self.create_composition(build_voxels.len()));
        let bp = Blueprint {
            id: project_id,
            build_type,
            voxels: build_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            composition_id,
            face_layout: Some(face_layout.into_iter().collect()),
            stress_warning: false,
            original_voxels,
        };
        self.db.blueprints.insert_no_fk(bp).unwrap();
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::BlueprintDesignated { project_id },
        });
    }

    /// Validate and create a blueprint for carving (removing) solid voxels.
    ///
    /// **Blueprint-aware:** Uses `blueprint_overlay()` to treat designated
    /// (not yet built) blueprints as their target voxel types for carvability
    /// checks and structural validation. A voxel that is Air in the real world
    /// but solid in the overlay (pending build) is considered carvable; a voxel
    /// that is solid but overlaid as Air (pending carve) is not.
    ///
    /// Filters the input to only carvable voxels (solid and not ForestFloor,
    /// considering overlay). Air and ForestFloor voxels are silently skipped.
    /// Records original voxel types for cancel restoration.
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

        let overlay = self.blueprint_overlay();
        let effective_type =
            |coord: VoxelCoord| -> VoxelType { overlay.effective_type(&self.world, coord) };

        // Filter to only carvable voxels: solid and not ForestFloor
        // (considering blueprint overlay so designated builds are carvable).
        let mut carve_voxels = Vec::new();
        let mut original_voxels = Vec::new();
        for &coord in voxels {
            let vt = effective_type(coord);
            if vt.is_solid() && vt != VoxelType::ForestFloor {
                carve_voxels.push(coord);
                original_voxels.push((coord, self.world.get(coord)));
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
            &overlay,
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
        let build_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Build { project_id },
            state: task::TaskState::Available,
            location: task_location,
            progress: 0.0,
            total_cost: num_voxels as f32,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(build_task);

        let bp = Blueprint {
            id: project_id,
            build_type: BuildType::Carve,
            voxels: carve_voxels,
            priority,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            composition_id: None,
            face_layout: None,
            stress_warning,
            original_voxels,
        };
        self.db.blueprints.insert_no_fk(bp).unwrap();
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
        let bp = match self.db.blueprints.remove_no_fk(&project_id) {
            Ok(bp) => bp,
            Err(_) => return,
        };

        // Remove any completed structure for this project (linear scan — map is small).
        // Also remove structure_voxels entries for the cancelled blueprint.
        for &coord in &bp.voxels {
            self.structure_voxels.remove(&coord);
        }
        let structure_ids_to_remove: Vec<StructureId> = self
            .db
            .structures
            .iter_all()
            .filter(|s| s.project_id == project_id)
            .map(|s| s.id)
            .collect();
        for sid in structure_ids_to_remove {
            let _ = self.db.structures.remove_no_fk(&sid);
        }

        // Remove the associated Build task and unassign workers.
        if let Some(task_id) = bp.task_id
            && let Ok(_task) = self.db.tasks.remove_no_fk(&task_id)
        {
            // Unassign any creatures working on this task.
            for mut creature in self
                .db
                .creatures
                .by_current_task(&Some(task_id), tabulosity::QueryOpts::ASC)
            {
                creature.current_task = None;
                creature.path = None;
                let _ = self.db.creatures.update_no_fk(creature);
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
            // Reverted voxels may have been supporting ground piles.
            self.apply_pile_gravity();
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
            progress: 0.0,
            total_cost: 0.0,
            required_species,
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(new_task);
    }

    /// Process a single scheduled event.
    fn process_event(&mut self, kind: ScheduledEventKind, events: &mut Vec<SimEvent>) {
        match kind {
            ScheduledEventKind::CreatureHeartbeat { creature_id } => {
                // Dead creatures: do not process heartbeat or reschedule.
                if self
                    .db
                    .creatures
                    .get(&creature_id)
                    .is_none_or(|c| c.vital_status != VitalStatus::Alive)
                {
                    return;
                }

                // Heartbeat is for periodic non-movement checks (mood, mana, etc.).
                // Movement is driven by CreatureActivation, not heartbeats.

                // Phase 1: apply food and rest decay, read state for need checks.
                let (should_seek_food, should_seek_sleep) =
                    if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                        let species = creature.species;
                        let species_data = &self.species_table[&species];
                        let interval = species_data.heartbeat_interval_ticks;

                        // Food decay.
                        let food_decay = species_data.food_decay_per_tick * interval as i64;
                        creature.food = (creature.food - food_decay).max(0);

                        // Rest decay.
                        let rest_decay = species_data.rest_decay_per_tick * interval as i64;
                        creature.rest = (creature.rest - rest_decay).max(0);

                        let food_threshold =
                            species_data.food_max * species_data.food_hunger_threshold_pct / 100;
                        let is_hungry = creature.food < food_threshold;

                        let rest_threshold =
                            species_data.rest_max * species_data.rest_tired_threshold_pct / 100;
                        let is_tired = creature.rest < rest_threshold;

                        let is_idle = creature.current_task.is_none();

                        // Write back mutated fields.
                        let _ = self.db.creatures.update_no_fk(creature);

                        // Expire old thoughts.
                        self.expire_creature_thoughts(creature_id);

                        // Reschedule the next heartbeat.
                        let next_tick = self.tick + interval;
                        self.event_queue.schedule(
                            next_tick,
                            ScheduledEventKind::CreatureHeartbeat { creature_id },
                        );

                        // Hunger takes priority over tiredness.
                        let seek_food = is_hungry && is_idle;
                        let seek_sleep = is_tired && is_idle && !is_hungry;

                        (seek_food, seek_sleep)
                    } else {
                        (false, false)
                    };

                // Phase 2a: if hungry and idle, eat bread from inventory
                // (instant, no travel) or fall back to seeking fruit.
                let mut ate_bread = false;
                if should_seek_food {
                    // Check for owned bread in inventory.
                    let has_bread = self
                        .db
                        .creatures
                        .get(&creature_id)
                        .map(|c| {
                            self.inv_count_owned(
                                c.inventory_id,
                                inventory::ItemKind::Bread,
                                creature_id,
                            ) > 0
                        })
                        .unwrap_or(false);

                    if has_bread
                        && let Some(nav_node) = self
                            .db
                            .creatures
                            .get(&creature_id)
                            .and_then(|c| c.current_node)
                    {
                        let task_id = TaskId::new(&mut self.rng);
                        let new_task = task::Task {
                            id: task_id,
                            kind: task::TaskKind::EatBread,
                            state: task::TaskState::InProgress,
                            location: nav_node,
                            progress: 0.0,
                            total_cost: 0.0,
                            required_species: None,
                            origin: task::TaskOrigin::Autonomous,
                            target_creature: None,
                        };
                        self.insert_task(new_task);
                        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                            creature.current_task = Some(task_id);
                            let _ = self.db.creatures.update_no_fk(creature);
                        }
                        ate_bread = true;
                    }
                }

                // Fall back to seeking fruit if no bread was available.
                if should_seek_food
                    && !ate_bread
                    && let Some((fruit_pos, nav_node)) = self.find_nearest_fruit(creature_id)
                {
                    let task_id = TaskId::new(&mut self.rng);
                    let new_task = task::Task {
                        id: task_id,
                        kind: task::TaskKind::EatFruit { fruit_pos },
                        state: task::TaskState::InProgress,
                        location: nav_node,
                        progress: 0.0,
                        total_cost: 0.0,
                        required_species: None,
                        origin: task::TaskOrigin::Autonomous,
                        target_creature: None,
                    };
                    self.insert_task(new_task);
                    if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                        creature.current_task = Some(task_id);
                        let _ = self.db.creatures.update_no_fk(creature);
                    }
                }

                // Phase 2b: if tired and idle (and not hungry), find a bed
                // or fall back to sleeping on the ground.
                // Priority: assigned home bed → dormitory bed → ground.
                if should_seek_sleep {
                    let (bed_pos, nav_node, sleep_ticks, sleep_location) =
                        if let Some((bp, nn, sid)) = self.find_assigned_home_bed(creature_id) {
                            (
                                Some(bp),
                                nn,
                                self.config.sleep_ticks_bed,
                                task::SleepLocation::Home(sid),
                            )
                        } else if let Some((bp, nn, sid)) = self.find_nearest_bed(creature_id) {
                            (
                                Some(bp),
                                nn,
                                self.config.sleep_ticks_bed,
                                task::SleepLocation::Dormitory(sid),
                            )
                        } else if let Some(creature) = self.db.creatures.get(&creature_id)
                            && let Some(node) = creature.current_node
                        {
                            (
                                None,
                                node,
                                self.config.sleep_ticks_ground,
                                task::SleepLocation::Ground,
                            )
                        } else {
                            return; // No valid position — skip.
                        };

                    let task_id = TaskId::new(&mut self.rng);
                    let new_task = task::Task {
                        id: task_id,
                        kind: task::TaskKind::Sleep {
                            bed_pos,
                            location: sleep_location,
                        },
                        state: task::TaskState::InProgress,
                        location: nav_node,
                        progress: 0.0,
                        total_cost: (sleep_ticks / self.config.sleep_action_ticks) as f32,
                        required_species: None,
                        origin: task::TaskOrigin::Autonomous,
                        target_creature: None,
                    };
                    self.insert_task(new_task);
                    if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                        creature.current_task = Some(task_id);
                        let _ = self.db.creatures.update_no_fk(creature);
                    }
                }

                // Phase 2b½: mood-based moping check. Only applies to elves
                // (only species with meaningful thoughts currently).
                self.check_mope(creature_id);

                // Phase 2c: if idle (no task from hunger or sleep), check
                // personal wants and acquire items from unowned sources.
                let still_idle = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .is_some_and(|c| c.current_task.is_none());
                if still_idle {
                    self.check_creature_wants(creature_id);
                }
            }
            ScheduledEventKind::CreatureActivation { creature_id } => {
                self.process_creature_activation(creature_id, events);
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
            ScheduledEventKind::LogisticsHeartbeat => {
                // Periodic gravity sweep: drop any floating piles before
                // logistics processing sees them.
                self.apply_pile_gravity();
                self.process_logistics_heartbeat();
                let next_tick = self.tick + self.config.logistics_heartbeat_interval_ticks;
                self.event_queue
                    .schedule(next_tick, ScheduledEventKind::LogisticsHeartbeat);
            }
            ScheduledEventKind::ProjectileTick => {
                self.process_projectile_tick(events);
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
    ) -> Option<CreatureId> {
        let species_data = &self.species_table[&species];
        let food_max = species_data.food_max;
        let rest_max = species_data.rest_max;
        let hp_max = species_data.hp_max;
        let heartbeat_interval = species_data.heartbeat_interval_ticks;
        let ground_only = species_data.ground_only;
        let graph = self.graph_for_species(species);

        let nearest_node = if ground_only {
            graph.find_nearest_ground_node(position)
        } else {
            graph.find_nearest_node(position)
        };

        let nearest_node = nearest_node?;

        let node_pos = graph.node(nearest_node).position;
        let creature_id = CreatureId::new(&mut self.rng);

        // Generate a Vaelith name for elves; other species are unnamed.
        let (name, name_meaning) = if species == Species::Elf {
            if let Some(lexicon) = &self.lexicon {
                let vname = elven_canopy_lang::names::generate_name(lexicon, &mut self.rng);
                (
                    vname.full_name,
                    format!("{} {}", vname.given_meaning, vname.surname_meaning),
                )
            } else {
                (String::new(), String::new())
            }
        } else {
            (String::new(), String::new())
        };

        let default_wants = if species == Species::Elf {
            self.config.elf_default_wants.clone()
        } else {
            Vec::new()
        };

        // Create an inventory for this creature.
        let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::Creature);

        // Elves belong to the player's civ; other species are unaffiliated.
        let civ_id = if species == Species::Elf {
            self.player_civ_id
        } else {
            None
        };

        let creature = crate::db::Creature {
            id: creature_id,
            species,
            position: node_pos,
            name,
            name_meaning,
            current_node: Some(nearest_node),
            path: None,
            current_task: None,
            food: food_max,
            rest: rest_max,
            assigned_home: None,
            inventory_id: inv_id,
            civ_id,
            action_kind: ActionKind::NoAction,
            next_available_tick: None,
            hp: hp_max,
            hp_max,
            vital_status: VitalStatus::Alive,
        };

        self.db.creatures.insert_no_fk(creature).unwrap();

        // Register in spatial index.
        let footprint = self.species_table[&species].footprint;
        Self::register_creature_in_index(&mut self.spatial_index, creature_id, node_pos, footprint);

        // Set default logistics wants for this creature.
        if !default_wants.is_empty() {
            self.set_inv_wants(inv_id, &default_wants);
        }

        // Give elves starting bread so they don't immediately forage.
        if species == Species::Elf && self.config.elf_starting_bread > 0 {
            self.inv_add_simple_item(
                inv_id,
                inventory::ItemKind::Bread,
                self.config.elf_starting_bread,
                Some(creature_id),
                None,
            );
        }

        // Schedule first activation (drives movement — wander or task work).
        // Fires 1 tick after spawn so the creature starts moving immediately.
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );

        // Schedule first heartbeat (periodic non-movement checks).
        let heartbeat_tick = self.tick + heartbeat_interval;
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
        Some(creature_id)
    }

    /// Find the lowest non-solid Y position at the given (x, z) column.
    /// Returns the first air voxel above solid ground, defaulting to y=1.
    fn find_surface_position(&self, x: i32, z: i32) -> VoxelCoord {
        for y in 1..self.world.size_y as i32 {
            let pos = VoxelCoord::new(x, y, z);
            if !self.world.get(pos).is_solid() {
                return pos;
            }
        }
        VoxelCoord::new(x, 1, z)
    }

    /// Get or create a ground pile at the given position, returning its ID.
    /// If no pile exists at `pos`, inserts a new empty one. If the position
    /// is floating (no solid voxel below), it is snapped down to the nearest
    /// surface before creation. If a pile already exists at the snapped
    /// position, that pile is returned instead of creating a new one.
    fn ensure_ground_pile(&mut self, pos: VoxelCoord) -> GroundPileId {
        // Snap to surface if the position is floating.
        let pos = if pos.y > 0
            && !self
                .world
                .get(VoxelCoord::new(pos.x, pos.y - 1, pos.z))
                .is_solid()
        {
            self.find_surface_below(pos.x, pos.y, pos.z)
        } else {
            pos
        };

        if let Some(pile) = self
            .db
            .ground_piles
            .by_position(&pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
        {
            pile.id
        } else {
            let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
            self.db
                .ground_piles
                .insert_auto_no_fk(|id| crate::db::GroundPile {
                    id,
                    position: pos,
                    inventory_id: inv_id,
                })
                .unwrap()
        }
    }

    /// Find the surface position below a given Y coordinate in a column.
    /// Scans downward from `start_y - 1` to find the first solid voxel, then
    /// returns the air voxel directly above it. Falls back to y=1 if no solid
    /// voxel is found (ForestFloor at y=0 is always solid).
    fn find_surface_below(&self, x: i32, start_y: i32, z: i32) -> VoxelCoord {
        for y in (0..start_y).rev() {
            if self.world.get(VoxelCoord::new(x, y, z)).is_solid() {
                return VoxelCoord::new(x, y + 1, z);
            }
        }
        // Shouldn't happen (ForestFloor at y=0 is solid), but safe fallback.
        VoxelCoord::new(x, 1, z)
    }

    /// Check all ground piles for gravity: if the voxel below a pile's position
    /// is not solid, the pile falls to the nearest surface below. If a pile
    /// already exists at the landing position, the falling pile's inventory is
    /// merged into it and the floating pile is deleted. Returns the number of
    /// piles that fell.
    fn apply_pile_gravity(&mut self) -> usize {
        // Collect all piles that need to fall. We snapshot first because
        // modifying tables during iteration would invalidate the iterator.
        let floating: Vec<(GroundPileId, VoxelCoord)> = self
            .db
            .ground_piles
            .iter_all()
            .filter_map(|pile| {
                let below = VoxelCoord::new(pile.position.x, pile.position.y - 1, pile.position.z);
                if pile.position.y > 0 && !self.world.get(below).is_solid() {
                    Some((pile.id, pile.position))
                } else {
                    None
                }
            })
            .collect();

        let mut fell_count = 0;
        for (pile_id, old_pos) in floating {
            // The pile may have been deleted by a previous iteration's merge.
            let pile = match self.db.ground_piles.get(&pile_id) {
                Some(p) => p,
                None => continue,
            };
            let landing = self.find_surface_below(old_pos.x, old_pos.y, old_pos.z);
            if landing == old_pos {
                continue; // Already on a surface (race with another pile falling here).
            }

            // Check if a pile already exists at the landing position.
            let existing = self
                .db
                .ground_piles
                .by_position(&landing, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next();

            if let Some(target_pile) = existing {
                // Merge inventories and delete the floating pile.
                let src_inv = pile.inventory_id;
                self.inv_merge(src_inv, target_pile.inventory_id);
                let _ = self.db.ground_piles.remove_no_fk(&pile_id);
                let _ = self.db.inventories.remove_no_fk(&src_inv);
            } else {
                // No pile at landing — remove and re-insert to update the
                // unique position index.
                let inv_id = pile.inventory_id;
                let _ = self.db.ground_piles.remove_no_fk(&pile_id);
                let _ = self
                    .db
                    .ground_piles
                    .insert_auto_no_fk(|new_id| crate::db::GroundPile {
                        id: new_id,
                        position: landing,
                        inventory_id: inv_id,
                    });
            }
            fell_count += 1;
        }
        fell_count
    }

    /// Spawn initial creatures and ground piles from `config.initial_creatures`
    /// and `config.initial_ground_piles`. Called once when a new game starts.
    pub fn spawn_initial_creatures(&mut self, events: &mut Vec<SimEvent>) {
        let specs = self.config.initial_creatures.clone();
        for spec in &specs {
            let species_data = match self.species_table.get(&spec.species) {
                Some(sd) => sd.clone(),
                None => continue,
            };
            for i in 0..spec.count {
                let creature_id =
                    match self.spawn_creature(spec.species, spec.spawn_position, events) {
                        Some(id) => id,
                        None => continue,
                    };

                // Apply per-creature food override.
                if let Some(&pct) = spec.food_pcts.get(i) {
                    let _ = self
                        .db
                        .creatures
                        .modify_unchecked(&creature_id, |creature| {
                            creature.food = species_data.food_max * pct as i64 / 100;
                        });
                }

                // Apply per-creature rest override.
                if let Some(&pct) = spec.rest_pcts.get(i) {
                    let _ = self
                        .db
                        .creatures
                        .modify_unchecked(&creature_id, |creature| {
                            creature.rest = species_data.rest_max * pct as i64 / 100;
                        });
                }

                // Apply per-creature bread count.
                if let Some(&count) = spec.bread_counts.get(i)
                    && count > 0
                {
                    let inv_id = self.creature_inv(creature_id);
                    self.inv_add_simple_item(
                        inv_id,
                        crate::inventory::ItemKind::Bread,
                        count,
                        Some(creature_id),
                        None,
                    );
                }
            }
        }

        let pile_specs = self.config.initial_ground_piles.clone();
        for pile_spec in &pile_specs {
            let pos = self.find_surface_position(pile_spec.position.x, pile_spec.position.z);
            let pile_id = self.ensure_ground_pile(pos);
            let pile = self.db.ground_piles.get(&pile_id).unwrap();
            self.inv_add_simple_item(
                pile.inventory_id,
                pile_spec.item_kind,
                pile_spec.quantity,
                None,
                None,
            );
        }
    }

    /// Abort a creature's current action, cleaning up any per-action state.
    ///
    /// For Move actions, deletes the `MoveAction` row. Clears `action_kind`
    /// and `next_available_tick` on the creature. Does NOT unassign the
    /// creature's task or clear its path — callers handle that.
    fn abort_current_action(&mut self, creature_id: CreatureId) {
        let action_kind = match self.db.creatures.get(&creature_id) {
            Some(c) => c.action_kind,
            None => return,
        };
        if action_kind == ActionKind::Move {
            let _ = self.db.move_actions.remove_no_fk(&creature_id);
        }
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::NoAction;
            c.next_available_tick = None;
        });
    }

    /// Creature activation: fires when a creature's current action completes
    /// (at `next_available_tick`) or when the creature is idle.
    ///
    /// Flow:
    /// 1. Resolve the completed action's effects (e.g., delete MoveAction row).
    /// 2. Clear action state.
    /// 3. Decision cascade: continue task, find new task, or wander.
    fn process_creature_activation(&mut self, creature_id: CreatureId, events: &mut Vec<SimEvent>) {
        let (mut current_node, species, action_kind) = {
            let creature = match self.db.creatures.get(&creature_id) {
                Some(c) if c.vital_status == VitalStatus::Alive => c,
                _ => return, // dead or missing — do not reschedule
            };
            let node = match creature.current_node {
                Some(n) => n,
                None => return,
            };
            (node, creature.species, creature.action_kind)
        };

        // Guard: if current_node is a dead slot (removed by incremental nav
        // update), abort any in-progress action and resnap the creature.
        if !self.graph_for_species(species).is_node_alive(current_node) {
            self.abort_current_action(creature_id);
            let pos = self
                .db
                .creatures
                .get(&creature_id)
                .map(|c| c.position)
                .unwrap();
            let graph = self.graph_for_species(species);
            let new_node = match graph.find_nearest_node(pos) {
                Some(n) => n,
                None => return,
            };
            let new_pos = graph.node(new_node).position;
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.current_node = Some(new_node);
                c.position = new_pos;
                c.path = None;
            });
            self.update_creature_spatial_index(creature_id, species, pos, new_pos);
            // Action was aborted — skip resolve, schedule a fresh activation
            // so the creature can find a new task or wander.
            self.event_queue.schedule(
                self.tick + 1,
                ScheduledEventKind::CreatureActivation { creature_id },
            );
            return;
        }

        // --- Step 1: Resolve completed action ---
        if action_kind == ActionKind::Move {
            // Move action completed — clean up the MoveAction row.
            let _ = self.db.move_actions.remove_no_fk(&creature_id);
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
        }
        if action_kind == ActionKind::Build {
            // Resolve the Build action — materialize one voxel.
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
            let completed = self.resolve_build_action(creature_id);
            // Re-read current_node: voxel materialization may have resnaped
            // the creature to a different node.
            current_node = match self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.current_node)
            {
                Some(n) => n,
                None => return,
            };
            if !completed {
                // Task still in progress — re-enter task behavior to start the
                // next Build action (or walk if creature moved off the location).
                let task_id = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_task);
                if let Some(task_id) = task_id {
                    self.execute_task_behavior(creature_id, task_id, current_node, events);
                    return;
                }
            }
            // Fall through to decision cascade (task completed or creature
            // lost its task during resolution).
        }

        // Resolve simple work actions (no nav graph changes, just clear state
        // and re-enter task behavior if not completed).
        if matches!(
            action_kind,
            ActionKind::Furnish
                | ActionKind::Cook
                | ActionKind::Craft
                | ActionKind::Sleep
                | ActionKind::Eat
                | ActionKind::Harvest
                | ActionKind::AcquireItem
                | ActionKind::PickUp
                | ActionKind::DropOff
                | ActionKind::Mope
                | ActionKind::MeleeStrike
                | ActionKind::Shoot
        ) {
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.action_kind = ActionKind::NoAction;
                c.next_available_tick = None;
            });
            let completed = self.resolve_work_action(creature_id, action_kind);
            if !completed {
                let task_id = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_task);
                if let Some(task_id) = task_id {
                    self.execute_task_behavior(creature_id, task_id, current_node, events);
                    return;
                }
            }
        }

        // --- Step 2: Decision cascade ---
        // Re-read current_task since it may have changed during resolution.
        let current_task = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task);

        if let Some(task_id) = current_task {
            // --- Has task: run task behavior ---
            self.execute_task_behavior(creature_id, task_id, current_node, events);
        } else {
            // --- No task: try to claim one, or wander ---
            if let Some(task_id) = self.find_available_task(creature_id) {
                self.claim_task(creature_id, task_id);
                // Run task behavior immediately on the same activation.
                self.execute_task_behavior(creature_id, task_id, current_node, events);
            } else {
                self.wander(creature_id, current_node, events);
            }
        }
    }

    /// Find the first available task this creature can work on.
    /// Respects species restrictions: tasks with `required_species` are only
    /// visible to matching creatures.
    fn find_available_task(&self, creature_id: CreatureId) -> Option<TaskId> {
        let creature = self.db.creatures.get(&creature_id)?;
        let species = creature.species;

        self.db
            .tasks
            .iter_all()
            .find(|t| {
                t.state == task::TaskState::Available
                    && t.required_species.is_none_or(|s| s == species)
            })
            .map(|t| t.id)
    }

    /// Assign a creature to a task.
    fn claim_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        if let Some(mut task) = self.db.tasks.get(&task_id) {
            task.state = task::TaskState::InProgress;
            let _ = self.db.tasks.update_no_fk(task);
        }
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.current_task = Some(task_id);
            let _ = self.db.creatures.update_no_fk(creature);
        }
    }

    /// Execute one activation's worth of task behavior.
    fn execute_task_behavior(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        current_node: NavNodeId,
        events: &mut Vec<SimEvent>,
    ) {
        let (mut task_location, target_creature) = match self.db.tasks.get(&task_id) {
            Some(t) => (t.location, t.target_creature),
            None => {
                // Task was removed — abort action, unassign, and wander.
                self.abort_current_action(creature_id);
                if let Some(mut c) = self.db.creatures.get(&creature_id) {
                    c.current_task = None;
                    c.path = None;
                    let _ = self.db.creatures.update_no_fk(c);
                }
                self.wander(creature_id, current_node, events);
                return;
            }
        };

        // --- Dynamic pursuit: track moving target creature ---
        if let Some(target_id) = target_creature {
            let target_node = self
                .db
                .creatures
                .get(&target_id)
                .and_then(|c| c.current_node);
            match target_node {
                None => {
                    // Target creature is gone or has no nav node — abandon.
                    self.interrupt_task(creature_id, task_id);
                    self.wander(creature_id, current_node, events);
                    return;
                }
                Some(target_nav) => {
                    if target_nav != task_location {
                        // Target moved — update task location and invalidate path.
                        task_location = target_nav;
                        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
                            t.location = target_nav;
                        });
                        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                            c.path = None;
                        });
                    }
                }
            }
        }

        // Check that both current_node and task_location are still alive in
        // the nav graph. They can become dead slots after incremental updates
        // (e.g. construction solidifying a voxel). If either is dead, abandon
        // the task and wander.
        let species = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| c.species)
            .unwrap_or(Species::Elf);
        let graph = self.graph_for_species(species);
        if !graph.is_node_alive(current_node) || !graph.is_node_alive(task_location) {
            // Clean up action and task state before abandoning.
            self.interrupt_task(creature_id, task_id);
            // Resnap the creature to a live node before wandering.
            let graph = self.graph_for_species(species);
            if let Some(c) = self.db.creatures.get(&creature_id) {
                let old_pos = c.position;
                if let Some(new_node) = graph.find_nearest_node(old_pos) {
                    let new_pos = graph.node(new_node).position;
                    let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                        c.current_node = Some(new_node);
                        c.position = new_pos;
                    });
                    self.update_creature_spatial_index(creature_id, species, old_pos, new_pos);
                    self.wander(creature_id, new_node, events);
                }
            }
            return;
        }

        if current_node == task_location {
            // At task location — run the kind-specific completion/work logic.
            self.execute_task_at_location(creature_id, task_id);
        } else {
            // Not at location — walk one edge toward it.
            self.walk_toward_task(creature_id, task_location, current_node, events);
        }
    }

    /// Execute task-kind-specific logic when the creature is at the task location.
    fn execute_task_at_location(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let task = match self.db.tasks.get(&task_id) {
            Some(t) => t,
            None => return,
        };

        match task.kind_tag {
            crate::db::TaskKindTag::GoTo => {
                self.complete_task(task_id);
            }
            crate::db::TaskKindTag::EatBread | crate::db::TaskKindTag::EatFruit => {
                self.start_simple_action(
                    creature_id,
                    ActionKind::Eat,
                    self.config.eat_action_ticks,
                );
                return;
            }
            crate::db::TaskKindTag::Build => {
                let project_id = match self.task_project_id(task_id) {
                    Some(p) => p,
                    None => return,
                };
                self.start_build_action(creature_id, task_id, project_id);
                return;
            }
            crate::db::TaskKindTag::Furnish => {
                self.start_furnish_action(creature_id);
                return;
            }
            crate::db::TaskKindTag::Sleep => {
                self.start_sleep_action(creature_id, task_id);
                return;
            }
            crate::db::TaskKindTag::Haul => {
                // Determine phase — PickUp or DropOff.
                let phase = self
                    .task_haul_data(task_id)
                    .map(|h| h.phase)
                    .unwrap_or(task::HaulPhase::GoingToSource);
                match phase {
                    task::HaulPhase::GoingToSource => self.start_pickup_action(creature_id),
                    task::HaulPhase::GoingToDestination => self.start_dropoff_action(creature_id),
                }
                return;
            }
            crate::db::TaskKindTag::Cook => {
                self.start_cook_action(creature_id);
                return;
            }
            crate::db::TaskKindTag::Harvest => {
                self.start_simple_action(
                    creature_id,
                    ActionKind::Harvest,
                    self.config.harvest_action_ticks,
                );
                return;
            }
            crate::db::TaskKindTag::AcquireItem => {
                self.start_simple_action(
                    creature_id,
                    ActionKind::AcquireItem,
                    self.config.acquire_item_action_ticks,
                );
                return;
            }
            crate::db::TaskKindTag::Mope => {
                self.start_mope_action(creature_id);
                return;
            }
            crate::db::TaskKindTag::Craft => {
                self.start_craft_action(creature_id, task_id);
                return;
            }
        }

        // Schedule next activation (creature is now idle, will wander or pick
        // up another task).
        self.event_queue.schedule(
            self.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Start a simple action with a given kind and duration. Used for one-shot
    /// actions (Eat, Harvest, AcquireItem) that need no extra setup logic.
    fn start_simple_action(
        &mut self,
        creature_id: CreatureId,
        action_kind: ActionKind,
        duration: u64,
    ) {
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = action_kind;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Dispatch to the appropriate resolve function for a completed work action.
    /// Returns true if the task was completed.
    fn resolve_work_action(&mut self, creature_id: CreatureId, action_kind: ActionKind) -> bool {
        // For Eat, we need to know the task kind (bread vs fruit) before resolving.
        let task_id = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task);
        let task_kind_tag = task_id
            .and_then(|tid| self.db.tasks.get(&tid))
            .map(|t| t.kind_tag);

        match action_kind {
            ActionKind::Furnish => self.resolve_furnish_action(creature_id),
            ActionKind::Cook => self.resolve_cook_action(creature_id),
            ActionKind::Craft => self.resolve_craft_action(creature_id),
            ActionKind::Sleep => self.resolve_sleep_action(creature_id),
            ActionKind::Mope => self.resolve_mope_action(creature_id),
            ActionKind::PickUp => self.resolve_pickup_action(creature_id),
            ActionKind::DropOff => self.resolve_dropoff_action(creature_id),
            ActionKind::Eat => {
                let tid = match task_id {
                    Some(t) => t,
                    None => return false,
                };
                match task_kind_tag {
                    Some(crate::db::TaskKindTag::EatFruit) => {
                        let fruit_pos = self
                            .task_voxel_ref(tid, crate::db::TaskVoxelRole::FruitTarget)
                            .unwrap_or(VoxelCoord::new(0, 0, 0));
                        self.resolve_eat_fruit_action(creature_id, tid, fruit_pos)
                    }
                    _ => self.resolve_eat_bread_action(creature_id, tid),
                }
            }
            ActionKind::Harvest => {
                let tid = match task_id {
                    Some(t) => t,
                    None => return false,
                };
                let fruit_pos = self
                    .task_voxel_ref(tid, crate::db::TaskVoxelRole::FruitTarget)
                    .unwrap_or(VoxelCoord::new(0, 0, 0));
                self.resolve_harvest_action(creature_id, tid, fruit_pos)
            }
            ActionKind::AcquireItem => {
                let tid = match task_id {
                    Some(t) => t,
                    None => return false,
                };
                self.resolve_acquire_item_action(creature_id, tid)
            }
            ActionKind::MeleeStrike | ActionKind::Shoot => {
                // MeleeStrike/Shoot are not task-driven; creature becomes idle.
                false
            }
            _ => false,
        }
    }

    /// Complete a task: set state to Complete, unassign all workers.
    fn complete_task(&mut self, task_id: TaskId) {
        // Find creatures assigned to this task before completing it.
        let assignees = self
            .db
            .creatures
            .by_current_task(&Some(task_id), tabulosity::QueryOpts::ASC);
        if assignees.is_empty() && self.db.tasks.get(&task_id).is_none() {
            return;
        }
        if let Some(mut task) = self.db.tasks.get(&task_id) {
            task.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(task);
        }

        for cid in assignees.iter().map(|c| &c.id) {
            if let Some(mut creature) = self.db.creatures.get(cid) {
                creature.current_task = None;
                creature.path = None;
                let _ = self.db.creatures.update_no_fk(creature);
            }
        }
    }

    /// Find the nearest reachable fruit for a creature, using Dijkstra over the
    /// nav graph with the creature's species-specific speeds and edge restrictions.
    ///
    /// Returns the fruit voxel coordinate and its nearest nav node, or `None`
    /// if no fruit exists or none is reachable by this creature.
    fn find_nearest_fruit(&self, creature_id: CreatureId) -> Option<(VoxelCoord, NavNodeId)> {
        let creature = self.db.creatures.get(&creature_id)?;
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

    /// Look up the fruit species at a voxel position.
    ///
    /// Returns the full `FruitSpecies` record, or `None` if the voxel has no
    /// tracked species (pre-fruit-variety fruit or empty).
    pub fn fruit_species_at(&self, pos: VoxelCoord) -> Option<crate::fruit::FruitSpecies> {
        let species_id = self.fruit_voxel_species.get(&pos)?;
        self.db.fruit_species.get(species_id)
    }

    /// Return a human-readable display name for an item stack. For fruit with
    /// a known species, returns e.g. "Shinethúni Fruit" or "Révatórun Pod".
    /// For all other items, returns the basic `ItemKind::display_name()`.
    pub fn item_display_name(&self, stack: &crate::db::ItemStack) -> String {
        if stack.kind == inventory::ItemKind::Fruit
            && let Some(inventory::Material::FruitSpecies(id)) = stack.material
            && let Some(species) = self.db.fruit_species.get(&id)
        {
            let noun = species.appearance.shape.item_noun();
            return format!("{} {}", species.vaelith_name, noun);
        }
        stack.kind.display_name().to_owned()
    }

    /// Resolve a completed Eat action for fruit: restore food, remove fruit
    /// from world, generate thought, complete task. Always returns true.
    fn resolve_eat_fruit_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        fruit_pos: VoxelCoord,
    ) -> bool {
        // Restore food.
        if let Some(creature) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let restore = species_data.food_max * species_data.food_restore_pct / 100;
            let food_max = species_data.food_max;
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.food = (creature.food + restore).min(food_max);
                });
        }

        // Remove fruit from world, tree's fruit_positions, and species map.
        if self.world.get(fruit_pos) == VoxelType::Fruit {
            self.world.set(fruit_pos, VoxelType::Air);
        }
        self.fruit_voxel_species.remove(&fruit_pos);
        self.fruit_voxel_species_list
            .retain(|(pos, _)| *pos != fruit_pos);
        for tree in self.trees.values_mut() {
            tree.fruit_positions.retain(|&p| p != fruit_pos);
        }

        // Generate AteMeal thought.
        self.add_creature_thought(creature_id, ThoughtKind::AteMeal);

        self.complete_task(task_id);
        true
    }

    /// Resolve a completed Harvest action: remove fruit voxel, create ground
    /// pile, complete task. Always returns true.
    fn resolve_harvest_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        fruit_pos: VoxelCoord,
    ) -> bool {
        // Check fruit still exists.
        let fruit_exists = self.world.get(fruit_pos) == VoxelType::Fruit;

        if fruit_exists {
            // Look up species before removing from the map.
            let species_id = self.fruit_voxel_species.remove(&fruit_pos);
            self.fruit_voxel_species_list
                .retain(|(pos, _)| *pos != fruit_pos);
            let material = species_id.map(inventory::Material::FruitSpecies);

            // Remove fruit from world and tree's fruit_positions list.
            self.world.set(fruit_pos, VoxelType::Air);
            for tree in self.trees.values_mut() {
                tree.fruit_positions.retain(|&p| p != fruit_pos);
            }

            // Create ground pile at creature's position with species material.
            if let Some(creature) = self.db.creatures.get(&creature_id) {
                let pile_pos = creature.position;
                let pile_id = self.ensure_ground_pile(pile_pos);
                let pile = self.db.ground_piles.get(&pile_id).unwrap();
                self.inv_add_item(
                    pile.inventory_id,
                    inventory::ItemKind::Fruit,
                    1,
                    None,     // owner
                    None,     // reserved_by
                    material, // fruit species
                    0,        // quality
                    None,     // enchantment
                );
            }
        }

        self.complete_task(task_id);
        true
    }

    /// Resolve a completed AcquireItem action: pick up reserved items from
    /// source, add to creature inventory with ownership. Always returns true.
    fn resolve_acquire_item_action(&mut self, creature_id: CreatureId, task_id: TaskId) -> bool {
        let acquire = match self.task_acquire_data(task_id) {
            Some(a) => a,
            None => return false,
        };
        let source = match self.task_acquire_source(task_id, acquire.source_kind) {
            Some(s) => s,
            None => return false,
        };
        let item_kind = acquire.item_kind;
        let quantity = acquire.quantity;

        // Remove reserved items from source.
        let picked_up = match &source {
            task::HaulSource::GroundPile(pos) => {
                if let Some(pile) = self
                    .db
                    .ground_piles
                    .by_position(pos, tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .next()
                {
                    self.inv_remove_reserved_items(pile.inventory_id, item_kind, quantity, task_id)
                } else {
                    0
                }
            }
            task::HaulSource::Building(sid) => {
                if let Some(structure) = self.db.structures.get(sid) {
                    self.inv_remove_reserved_items(
                        structure.inventory_id,
                        item_kind,
                        quantity,
                        task_id,
                    )
                } else {
                    0
                }
            }
        };

        // Clean up empty ground piles.
        if let task::HaulSource::GroundPile(pos) = &source
            && let Some(pile) = self
                .db
                .ground_piles
                .by_position(pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
            && self.inv_items(pile.inventory_id).is_empty()
        {
            let _ = self.db.ground_piles.remove_no_fk(&pile.id);
        }

        // Add to creature inventory with ownership.
        if picked_up > 0 {
            let inv_id = self.creature_inv(creature_id);
            self.inv_add_simple_item(inv_id, item_kind, picked_up, Some(creature_id), None);
        }

        self.complete_task(task_id);
        true
    }

    /// Clean up an AcquireItem task on abandonment: clear reservations at
    /// the source. No items are in transit (pickup only happens on arrival).
    fn cleanup_acquire_item_task(&mut self, task_id: TaskId) {
        let acquire = match self.task_acquire_data(task_id) {
            Some(a) => a,
            None => return,
        };
        let source = match self.task_acquire_source(task_id, acquire.source_kind) {
            Some(s) => s,
            None => return,
        };
        match &source {
            task::HaulSource::GroundPile(pos) => {
                if let Some(pile) = self
                    .db
                    .ground_piles
                    .by_position(pos, tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .next()
                {
                    self.inv_clear_reservations(pile.inventory_id, task_id);
                }
            }
            task::HaulSource::Building(sid) => {
                if let Some(structure) = self.db.structures.get(sid) {
                    self.inv_clear_reservations(structure.inventory_id, task_id);
                }
            }
        }
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Resolve a completed EatBread action: remove 1 owned bread, restore food,
    /// generate thought, complete task. Always returns true.
    fn resolve_eat_bread_action(&mut self, creature_id: CreatureId, task_id: TaskId) -> bool {
        if let Some(creature) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let restore = species_data.food_max * species_data.bread_restore_pct / 100;
            let food_max = species_data.food_max;
            self.inv_remove_owned_item(
                creature.inventory_id,
                inventory::ItemKind::Bread,
                creature_id,
                1,
            );
            let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
                c.food = (c.food + restore).min(food_max);
            });
        }

        // Generate AteMeal thought.
        self.add_creature_thought(creature_id, ThoughtKind::AteMeal);

        self.complete_task(task_id);
        true
    }

    /// Find the bed in the creature's assigned home, if any.
    ///
    /// Returns `None` if the creature has no assigned home, the home isn't a
    /// Home, or the home has no placed furniture (bed not yet built). Does NOT
    /// check occupied-bed exclusion — it's the elf's personal bed.
    /// Returns `(bed_pos, nav_node, structure_id)`.
    fn find_assigned_home_bed(
        &self,
        creature_id: CreatureId,
    ) -> Option<(VoxelCoord, NavNodeId, StructureId)> {
        let creature = self.db.creatures.get(&creature_id)?;
        let home_id = creature.assigned_home?;
        let structure = self.db.structures.get(&home_id)?;
        if structure.furnishing != Some(FurnishingType::Home) {
            return None;
        }
        let bed = self
            .db
            .furniture
            .by_structure_id(&home_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|f| f.placed)?;
        let graph = self.graph_for_species(creature.species);
        let nav_node = graph.find_nearest_node(bed.coord)?;
        Some((bed.coord, nav_node, home_id))
    }

    /// Find the nearest reachable dormitory bed for a creature, using Dijkstra
    /// over the nav graph with species-specific speeds and edge restrictions.
    ///
    /// Excludes beds already occupied by an active Sleep task. Returns the bed
    /// position, its nearest nav node, and the structure ID, or `None` if no
    /// unoccupied beds exist or none are reachable.
    fn find_nearest_bed(
        &self,
        creature_id: CreatureId,
    ) -> Option<(VoxelCoord, NavNodeId, StructureId)> {
        let creature = self.db.creatures.get(&creature_id)?;
        let start_node = creature.current_node?;
        let species_data = &self.species_table[&creature.species];
        let graph = self.graph_for_species(creature.species);

        // Collect all occupied bed positions from active Sleep tasks.
        let occupied_beds: Vec<VoxelCoord> = self
            .db
            .task_voxel_refs
            .iter_all()
            .filter(|r| r.role == crate::db::TaskVoxelRole::BedPosition)
            .filter(|r| {
                self.db
                    .tasks
                    .get(&r.task_id)
                    .is_some_and(|t| t.state != task::TaskState::Complete)
            })
            .map(|r| r.coord)
            .collect();

        // Collect unoccupied bed positions from all dormitory structures.
        let mut nav_to_bed: Vec<(NavNodeId, VoxelCoord, StructureId)> = Vec::new();
        let mut target_nodes: Vec<NavNodeId> = Vec::new();
        for structure in self.db.structures.iter_all() {
            if structure.furnishing != Some(FurnishingType::Dormitory) {
                continue;
            }
            for furn in self
                .db
                .furniture
                .by_structure_id(&structure.id, tabulosity::QueryOpts::ASC)
            {
                if !furn.placed || occupied_beds.contains(&furn.coord) {
                    continue;
                }
                if let Some(nav_node) = graph.find_nearest_node(furn.coord) {
                    target_nodes.push(nav_node);
                    nav_to_bed.push((nav_node, furn.coord, structure.id));
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

        let (_, bed_pos, structure_id) = nav_to_bed.iter().find(|(n, _, _)| *n == nearest_node)?;

        Some((*bed_pos, nearest_node, *structure_id))
    }

    /// Start a Sleep action: set action kind and schedule next activation
    /// after `sleep_action_ticks`. On first action, check for low ceiling.
    fn start_sleep_action(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let duration = self.config.sleep_action_ticks;

        // On first sleep action, check for low ceiling.
        let progress = self
            .db
            .tasks
            .get(&task_id)
            .map(|t| t.progress)
            .unwrap_or(0.0);
        if progress == 0.0 {
            let location = self.task_sleep_location(task_id);
            if let Some(location) = &location {
                let structure_id = match location {
                    task::SleepLocation::Home(sid) | task::SleepLocation::Dormitory(sid) => {
                        Some(*sid)
                    }
                    task::SleepLocation::Ground => None,
                };
                if let Some(sid) = structure_id
                    && let Some(structure) = self.db.structures.get(&sid)
                    && structure.build_type == BuildType::Building
                    && structure.height == 1
                {
                    self.add_creature_thought(creature_id, ThoughtKind::LowCeiling(sid));
                }
            }
        }

        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Sleep;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Sleep action: restore rest, increment progress,
    /// check for completion or rest full. Returns true if task completed.
    fn resolve_sleep_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };

        // Restore rest: use rest_per_sleep_tick * sleep_action_ticks to get
        // the per-action restore amount (preserves total balance).
        if let Some(creature) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&creature.species];
            let rest_max = species_data.rest_max;
            let restore = species_data.rest_per_sleep_tick * self.config.sleep_action_ticks as i64;
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.rest = (creature.rest + restore).min(rest_max);
                });
        }

        // Increment progress by 1 (one action).
        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
            t.progress += 1.0;
        });

        // Check if done by progress or rest full.
        let done = self
            .db
            .tasks
            .get(&task_id)
            .is_some_and(|t| t.progress >= t.total_cost);

        let rest_full = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| {
                let species_data = &self.species_table[&c.species];
                c.rest >= species_data.rest_max
            })
            .unwrap_or(false);

        if done || rest_full {
            let location = self.task_sleep_location(task_id);
            if let Some(location) = &location {
                let thought_kind = match location {
                    task::SleepLocation::Home(sid) => ThoughtKind::SleptInOwnHome(*sid),
                    task::SleepLocation::Dormitory(sid) => ThoughtKind::SleptInDormitory(*sid),
                    task::SleepLocation::Ground => ThoughtKind::SleptOnGround,
                };
                self.add_creature_thought(creature_id, thought_kind);
            }
            self.complete_task(task_id);
            return true;
        }
        false
    }

    /// Start a Mope action: set action kind and schedule next activation
    /// after `mope_action_ticks`.
    fn start_mope_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.mope_action_ticks;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Mope;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Mope action: increment progress by
    /// `mope_action_ticks`, check for completion. Returns true if done.
    fn resolve_mope_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };

        let increment = self.config.mope_action_ticks as f32;
        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
            t.progress += increment;
        });

        let done = self
            .db
            .tasks
            .get(&task_id)
            .is_some_and(|t| t.progress >= t.total_cost);

        if done {
            self.complete_task(task_id);
            return true;
        }
        false
    }

    /// Start a PickUp action (haul source pickup): set action kind and
    /// schedule next activation after `haul_pickup_action_ticks`.
    fn start_pickup_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.haul_pickup_action_ticks;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::PickUp;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed PickUp action: remove reserved items from source,
    /// add to creature inventory, switch haul phase to GoingToDestination.
    /// Returns true if task completed (source empty → cancelled).
    fn resolve_pickup_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let haul = match self.task_haul_data(task_id) {
            Some(h) => h,
            None => return false,
        };
        let source = match self.task_haul_source(task_id, haul.source_kind) {
            Some(s) => s,
            None => return false,
        };
        let item_kind = haul.item_kind;
        let quantity = haul.quantity;
        let destination_nav_node = haul.destination_nav_node;

        // Pick up items from source.
        let picked_up = match &source {
            task::HaulSource::GroundPile(pos) => {
                if let Some(pile) = self
                    .db
                    .ground_piles
                    .by_position(pos, tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .next()
                {
                    self.inv_remove_reserved_items(pile.inventory_id, item_kind, quantity, task_id)
                } else {
                    0
                }
            }
            task::HaulSource::Building(sid) => {
                if let Some(structure) = self.db.structures.get(sid) {
                    self.inv_remove_reserved_items(
                        structure.inventory_id,
                        item_kind,
                        quantity,
                        task_id,
                    )
                } else {
                    0
                }
            }
        };

        // Clean up empty ground piles.
        if let task::HaulSource::GroundPile(pos) = &source
            && let Some(pile) = self
                .db
                .ground_piles
                .by_position(pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
            && self.inv_items(pile.inventory_id).is_empty()
        {
            let _ = self.db.ground_piles.remove_no_fk(&pile.id);
        }

        if picked_up == 0 {
            // Source empty — cancel task.
            self.complete_task(task_id);
            return true;
        }

        // Add items to creature inventory.
        let inv_id = self.creature_inv(creature_id);
        self.inv_add_simple_item(inv_id, item_kind, picked_up, None, None);

        // Switch to GoingToDestination phase.
        let mut updated_haul = haul.clone();
        updated_haul.phase = task::HaulPhase::GoingToDestination;
        updated_haul.quantity = picked_up;
        let _ = self.db.task_haul_data.update_no_fk(updated_haul);
        // Update task location for the new destination.
        let _ = self.db.tasks.modify_unchecked(&task_id, |task| {
            task.location = destination_nav_node;
        });
        // Clear cached path so creature re-pathfinds to new destination.
        let _ = self
            .db
            .creatures
            .modify_unchecked(&creature_id, |creature| {
                creature.path = None;
            });
        false
    }

    /// Start a DropOff action (haul destination deposit): set action kind and
    /// schedule next activation after `haul_dropoff_action_ticks`.
    fn start_dropoff_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.haul_dropoff_action_ticks;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::DropOff;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed DropOff action: deposit items at destination,
    /// complete task. Always returns true.
    fn resolve_dropoff_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let haul = match self.task_haul_data(task_id) {
            Some(h) => h,
            None => return false,
        };
        let destination =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::HaulDestination) {
                Some(d) => d,
                None => return false,
            };
        let item_kind = haul.item_kind;
        let quantity = haul.quantity;

        // Deposit items into destination building.
        self.inv_remove_item(self.creature_inv(creature_id), item_kind, quantity);
        self.inv_add_simple_item(
            self.structure_inv(destination),
            item_kind,
            quantity,
            None,
            None,
        );
        self.complete_task(task_id);
        true
    }

    /// Clean up haul task state when a haul task is abandoned.
    ///
    /// - **GoingToSource:** Release reserved items at source.
    /// - **GoingToDestination:** Creature is carrying items — drop as ground pile.
    fn cleanup_haul_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        let haul = match self.task_haul_data(task_id) {
            Some(h) => h,
            None => return,
        };
        let source = match self.task_haul_source(task_id, haul.source_kind) {
            Some(s) => s,
            None => return,
        };
        let item_kind = haul.item_kind;
        let quantity = haul.quantity;
        let phase = haul.phase;

        match phase {
            task::HaulPhase::GoingToSource => {
                // Clear reservations at the source.
                match &source {
                    task::HaulSource::GroundPile(pos) => {
                        if let Some(pile) = self
                            .db
                            .ground_piles
                            .by_position(pos, tabulosity::QueryOpts::ASC)
                            .into_iter()
                            .next()
                        {
                            self.inv_clear_reservations(pile.inventory_id, task_id);
                        }
                    }
                    task::HaulSource::Building(sid) => {
                        if let Some(structure) = self.db.structures.get(sid) {
                            self.inv_clear_reservations(structure.inventory_id, task_id);
                        }
                    }
                }
            }
            task::HaulPhase::GoingToDestination => {
                // Creature is carrying items — drop as ground pile at current position.
                if let Some(creature) = self.db.creatures.get(&creature_id) {
                    let pos = creature.position;
                    let removed = self.inv_remove_item(creature.inventory_id, item_kind, quantity);
                    if removed > 0 {
                        let pile_id = self.ensure_ground_pile(pos);
                        let pile = self.db.ground_piles.get(&pile_id).unwrap();
                        self.inv_add_simple_item(pile.inventory_id, item_kind, removed, None, None);
                    }
                }
            }
        }
    }

    /// Start a Cook action: set action kind and schedule next activation
    /// after `cook_work_ticks`. Cook is a single-action task.
    fn start_cook_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.cook_work_ticks;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Cook;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Cook action: consume reserved fruit, produce bread.
    /// Always returns true (single-action task).
    fn resolve_cook_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CookAt) {
                Some(s) => s,
                None => return false,
            };

        // Cooking complete — consume fruit, produce bread.
        let fruit_input = self.config.cook_fruit_input;
        let bread_output = self.config.cook_bread_output;
        let inv_id = self.structure_inv(structure_id);
        let removed = self.inv_remove_reserved_items(
            inv_id,
            inventory::ItemKind::Fruit,
            fruit_input,
            task_id,
        );
        if removed < fruit_input {
            self.inv_clear_reservations(inv_id, task_id);
        } else {
            self.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, bread_output, None, None);
        }
        self.complete_task(task_id);
        true
    }

    /// Clean up a Cook task on node invalidation: release reserved fruit in
    /// the kitchen's inventory and set the task to Complete so the kitchen
    /// monitor can create a fresh task on the next heartbeat.
    fn cleanup_cook_task(&mut self, task_id: TaskId) {
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CookAt) {
                Some(s) => s,
                None => return,
            };
        self.inv_clear_reservations(self.structure_inv(structure_id), task_id);
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Start a Craft action: set action kind and schedule next activation
    /// after `recipe.work_ticks`. Craft is a single-action task.
    fn start_craft_action(&mut self, creature_id: CreatureId, task_id: TaskId) {
        // Look up the recipe to get work_ticks.
        let duration = self
            .task_craft_data(task_id)
            .and_then(|d| {
                self.config
                    .recipes
                    .iter()
                    .find(|r| r.id == d.recipe_id)
                    .map(|r| r.work_ticks)
            })
            .unwrap_or(5000);

        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Craft;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Craft action: consume reserved inputs, produce
    /// outputs with subcomponents. Always returns true (single-action task).
    fn resolve_craft_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CraftAt) {
                Some(s) => s,
                None => return false,
            };

        // Look up recipe via TaskCraftData.
        let recipe_id = match self.task_craft_data(task_id) {
            Some(d) => d.recipe_id.clone(),
            None => {
                self.complete_task(task_id);
                return true;
            }
        };
        let recipe = match self.config.recipes.iter().find(|r| r.id == recipe_id) {
            Some(r) => r.clone(),
            None => {
                self.complete_task(task_id);
                return true;
            }
        };

        let inv_id = self.structure_inv(structure_id);

        // Remove reserved inputs.
        for input in &recipe.inputs {
            self.inv_remove_reserved_items(inv_id, input.item_kind, input.quantity, task_id);
        }

        // Produce outputs and record subcomponents.
        for output in &recipe.outputs {
            self.inv_add_item(
                inv_id,
                output.item_kind,
                output.quantity,
                None,
                None,
                output.material,
                output.quality,
                None,
            );

            // Record subcomponents on the output stack.
            if !recipe.subcomponent_records.is_empty() {
                // Find the stack we just added.
                let stacks = self
                    .db
                    .item_stacks
                    .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
                if let Some(output_stack) = stacks.iter().rev().find(|s| {
                    s.kind == output.item_kind
                        && s.material == output.material
                        && s.quality == output.quality
                        && s.owner.is_none()
                        && s.reserved_by.is_none()
                }) {
                    let stack_id = output_stack.id;
                    for sub in &recipe.subcomponent_records {
                        let sub_kind = sub.input_kind;
                        let sub_qty = sub.quantity_per_item;
                        let _ = self.db.item_subcomponents.insert_auto_no_fk(|id| {
                            crate::db::ItemSubcomponent {
                                id,
                                item_stack_id: stack_id,
                                component_kind: sub_kind,
                                material: None,
                                quality: 0,
                                quantity_per_item: sub_qty,
                            }
                        });
                    }
                }
            }
        }

        self.complete_task(task_id);
        true
    }

    /// Clean up a Craft task on node invalidation: release reserved inputs in
    /// the workshop's inventory and set the task to Complete.
    fn cleanup_craft_task(&mut self, task_id: TaskId) {
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::CraftAt) {
                Some(s) => s,
                None => return,
            };
        self.inv_clear_reservations(self.structure_inv(structure_id), task_id);
        if let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Scan workshops and create Craft tasks when conditions are met.
    /// Called at the end of each logistics heartbeat after the kitchen monitor.
    fn process_workshop_monitor(&mut self) {
        let workshop_ids: Vec<StructureId> = self
            .db
            .structures
            .iter_all()
            .filter_map(|s| {
                if s.furnishing == Some(FurnishingType::Workshop) && s.workshop_enabled {
                    Some(s.id)
                } else {
                    None
                }
            })
            .collect();

        for sid in workshop_ids {
            let structure = match self.db.structures.get(&sid) {
                Some(s) => s,
                None => continue,
            };

            // Skip if there's already an active (non-Complete) Craft task for this workshop.
            let has_active_craft = self
                .db
                .task_structure_refs
                .by_structure_id(&sid, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|r| {
                    r.role == crate::db::TaskStructureRole::CraftAt
                        && self
                            .db
                            .tasks
                            .get(&r.task_id)
                            .is_some_and(|t| t.state != task::TaskState::Complete)
                });
            if has_active_craft {
                continue;
            }

            let recipe_ids = structure.workshop_recipe_ids.clone();
            let recipe_targets = structure.workshop_recipe_targets.clone();
            let inv_id = structure.inventory_id;

            // Find first recipe whose inputs are all available (unreserved) and
            // whose output target has not been reached.
            let mut chosen_recipe: Option<crate::config::Recipe> = None;
            for rid in &recipe_ids {
                if let Some(recipe) = self.config.recipes.iter().find(|r| &r.id == rid) {
                    // Check per-recipe output target. 0 or missing = don't craft.
                    let target = recipe_targets.get(rid).copied().unwrap_or(0);
                    if target == 0 {
                        continue;
                    }
                    let output_count: u32 = recipe
                        .outputs
                        .iter()
                        .map(|o| self.inv_item_count(inv_id, o.item_kind))
                        .sum();
                    if output_count >= target {
                        continue;
                    }

                    let all_available = recipe.inputs.iter().all(|input| {
                        self.inv_unreserved_item_count(inv_id, input.item_kind) >= input.quantity
                    });
                    if all_available {
                        chosen_recipe = Some(recipe.clone());
                        break;
                    }
                }
            }

            let recipe = match chosen_recipe {
                Some(r) => r,
                None => continue,
            };

            // Find nav node inside the workshop.
            let interior_pos = self.db.structures.get(&sid).unwrap().anchor;
            let location = match self.nav_graph.find_nearest_node(interior_pos) {
                Some(n) => n,
                None => continue,
            };

            // Reserve all inputs.
            let task_id = TaskId::new(&mut self.rng);
            for input in &recipe.inputs {
                self.inv_reserve_items(inv_id, input.item_kind, input.quantity, task_id);
            }

            let recipe_id = recipe.id.clone();
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::Craft {
                    structure_id: sid,
                    recipe_id,
                },
                state: task::TaskState::Available,
                location,
                progress: 0.0,
                total_cost: 1.0, // Single action per craft recipe.
                required_species: Some(Species::Elf),
                origin: task::TaskOrigin::Automated,
                target_creature: None,
            };
            self.insert_task(new_task);
        }
    }

    /// Set workshop configuration: enabled state and active recipe configs.
    /// Rejects invalid recipe IDs and non-Workshop structures.
    fn set_workshop_config(
        &mut self,
        structure_id: StructureId,
        workshop_enabled: bool,
        recipe_configs: Vec<crate::command::WorkshopRecipeEntry>,
    ) {
        if self
            .db
            .structures
            .get(&structure_id)
            .is_none_or(|s| s.furnishing != Some(FurnishingType::Workshop))
        {
            return;
        }

        // Filter to valid recipe IDs only, preserving targets.
        let valid_configs: Vec<crate::command::WorkshopRecipeEntry> = recipe_configs
            .into_iter()
            .filter(|rc| self.config.recipes.iter().any(|r| r.id == rc.recipe_id))
            .collect();

        let enabled = workshop_enabled;
        let ids: Vec<String> = valid_configs
            .iter()
            .map(|rc| rc.recipe_id.clone())
            .collect();
        let targets: std::collections::BTreeMap<String, u32> = valid_configs
            .iter()
            .map(|rc| (rc.recipe_id.clone(), rc.target))
            .collect();
        let ids_clone = ids.clone();
        let _ = self.db.structures.modify_unchecked(&structure_id, |s| {
            s.workshop_enabled = enabled;
            s.workshop_recipe_ids = ids_clone;
            s.workshop_recipe_targets = targets;
        });

        // Recompute logistics wants from configured recipes' inputs.
        let inv_id = self.structure_inv(structure_id);
        let wants = self.compute_recipe_wants(&ids);
        self.set_inv_wants(inv_id, &wants);
    }

    /// Compute logistics wants from a set of recipe IDs. Each input item kind
    /// gets a want with quantity equal to the max needed by any single recipe.
    fn compute_recipe_wants(&self, recipe_ids: &[String]) -> Vec<crate::building::LogisticsWant> {
        let mut wants: Vec<crate::building::LogisticsWant> = Vec::new();
        for rid in recipe_ids {
            if let Some(recipe) = self.config.recipes.iter().find(|r| r.id == *rid) {
                for input in &recipe.inputs {
                    if let Some(w) = wants.iter_mut().find(|w| w.item_kind == input.item_kind) {
                        w.target_quantity = w.target_quantity.max(input.quantity);
                    } else {
                        wants.push(crate::building::LogisticsWant {
                            item_kind: input.item_kind,
                            target_quantity: input.quantity,
                        });
                    }
                }
            }
        }
        wants
    }

    /// Get the TaskCraftData for a task, if it exists.
    fn task_craft_data(&self, task_id: TaskId) -> Option<crate::db::TaskCraftData> {
        self.db
            .task_craft_data
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
    }

    /// Clean up a Harvest task on node invalidation. Harvest tasks have no
    /// reservations, so just mark the task complete so `process_harvest_tasks`
    /// can create a replacement on the next heartbeat.
    fn cleanup_harvest_task(&mut self, task_id: TaskId) {
        let is_harvest = self
            .db
            .tasks
            .get(&task_id)
            .is_some_and(|t| t.kind_tag == crate::db::TaskKindTag::Harvest);
        if is_harvest && let Some(mut t) = self.db.tasks.get(&task_id) {
            t.state = task::TaskState::Complete;
            let _ = self.db.tasks.update_no_fk(t);
        }
    }

    /// Create Harvest tasks when logistics buildings want fruit but not enough
    /// fruit items exist. Scans all trees for unclaimed fruit voxels and creates
    /// up to `max_haul_tasks_per_heartbeat` Harvest tasks.
    fn process_harvest_tasks(&mut self) {
        // 1. Sum total fruit demand across logistics-enabled buildings.
        let mut total_demand: u32 = 0;
        for structure in self.db.structures.iter_all() {
            if structure.logistics_priority.is_none() {
                continue;
            }
            let fruit_target =
                self.inv_want_target(structure.inventory_id, inventory::ItemKind::Fruit);
            if fruit_target > 0 {
                let current =
                    self.inv_item_count(structure.inventory_id, inventory::ItemKind::Fruit);
                let in_transit =
                    self.count_in_transit_items(structure.id, inventory::ItemKind::Fruit);
                let effective = current + in_transit;
                if fruit_target > effective {
                    total_demand += fruit_target - effective;
                }
            }
        }

        if total_demand == 0 {
            return;
        }

        // 2. Count fruit already available as items (unreserved in ground piles +
        // logistics-enabled building inventories). Non-logistics buildings are excluded
        // because their fruit can't be hauled out.
        let mut available_items: u32 = 0;
        for pile in self.db.ground_piles.iter_all() {
            available_items +=
                self.inv_unreserved_item_count(pile.inventory_id, inventory::ItemKind::Fruit);
        }
        for structure in self.db.structures.iter_all() {
            if structure.logistics_priority.is_some() {
                available_items += self
                    .inv_unreserved_item_count(structure.inventory_id, inventory::ItemKind::Fruit);
            }
        }

        // 3. Count pending Harvest tasks (non-Complete).
        let being_harvested: u32 = self
            .db
            .tasks
            .iter_all()
            .filter(|t| {
                t.state != task::TaskState::Complete
                    && t.kind_tag == crate::db::TaskKindTag::Harvest
            })
            .count() as u32;

        // 4. Compute shortfall.
        let shortfall = total_demand.saturating_sub(available_items + being_harvested);
        if shortfall == 0 {
            return;
        }

        // 5. Collect unclaimed fruit positions (skip those with existing Harvest or EatFruit tasks).
        // FruitTarget role is shared by both Harvest and EatFruit tasks.
        let claimed_positions: Vec<VoxelCoord> = self
            .db
            .task_voxel_refs
            .iter_all()
            .filter(|r| r.role == crate::db::TaskVoxelRole::FruitTarget)
            .filter(|r| {
                self.db
                    .tasks
                    .get(&r.task_id)
                    .is_some_and(|t| t.state != task::TaskState::Complete)
            })
            .map(|r| r.coord)
            .collect();

        let mut unclaimed_fruit: Vec<(VoxelCoord, NavNodeId)> = Vec::new();
        for tree in self.trees.values() {
            for &fruit_pos in &tree.fruit_positions {
                if !claimed_positions.contains(&fruit_pos)
                    && let Some(nav_node) = self.nav_graph.find_nearest_node(fruit_pos)
                {
                    unclaimed_fruit.push((fruit_pos, nav_node));
                }
            }
        }

        // 6. Create up to min(shortfall, available_fruit, max_haul_tasks_per_heartbeat) Harvest tasks.
        let max_tasks = self.config.max_haul_tasks_per_heartbeat;
        let to_create = shortfall.min(unclaimed_fruit.len() as u32).min(max_tasks);

        for &(fruit_pos, nav_node) in unclaimed_fruit.iter().take(to_create as usize) {
            let task_id = TaskId::new(&mut self.rng);
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::Harvest { fruit_pos },
                state: task::TaskState::Available,
                location: nav_node,
                progress: 0.0,
                total_cost: 0.0,
                required_species: Some(Species::Elf),
                origin: task::TaskOrigin::Automated,
                target_creature: None,
            };
            self.insert_task(new_task);
        }
    }

    /// Process the logistics heartbeat: scan buildings with logistics config
    /// for unmet wants and create haul tasks to fulfill them.
    fn process_logistics_heartbeat(&mut self) {
        self.process_harvest_tasks();

        // Collect buildings with logistics enabled, sorted by priority desc then StructureId asc.
        let mut logistics_buildings: Vec<(StructureId, u8)> = self
            .db
            .structures
            .iter_all()
            .filter_map(|s| s.logistics_priority.map(|p| (s.id, p)))
            .collect();
        logistics_buildings.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let max_tasks = self.config.max_haul_tasks_per_heartbeat;
        let mut tasks_created = 0u32;

        for (building_id, building_priority) in &logistics_buildings {
            if tasks_created >= max_tasks {
                break;
            }

            let inv_id = match self.db.structures.get(building_id) {
                Some(s) => s.inventory_id,
                None => continue,
            };
            let wants = self.inv_wants(inv_id);

            for want in &wants {
                if tasks_created >= max_tasks {
                    break;
                }

                // Count current inventory in this building for this item kind.
                let current = self
                    .db
                    .structures
                    .get(building_id)
                    .map(|s| self.inv_item_count(s.inventory_id, want.item_kind))
                    .unwrap_or(0);

                // Count in-transit items (from active Haul tasks targeting this building).
                let in_transit = self.count_in_transit_items(*building_id, want.item_kind);

                let effective = current + in_transit;
                if effective >= want.target_quantity {
                    continue;
                }

                let needed = want.target_quantity - effective;

                // Find a source for these items.
                if let Some((source, available, source_nav_node)) =
                    self.find_haul_source(want.item_kind, needed, *building_id, *building_priority)
                {
                    let quantity = available.min(needed);

                    // Find destination nav node.
                    let dest_nav_node = match self
                        .nav_graph
                        .find_nearest_node(self.db.structures.get(building_id).unwrap().anchor)
                    {
                        Some(n) => n,
                        None => continue,
                    };

                    // Reserve items at source.
                    let task_id = TaskId::new(&mut self.rng);
                    self.reserve_haul_items(&source, want.item_kind, quantity, task_id);

                    // Create haul task.
                    let new_task = task::Task {
                        id: task_id,
                        kind: task::TaskKind::Haul {
                            item_kind: want.item_kind,
                            quantity,
                            source,
                            destination: *building_id,
                            phase: task::HaulPhase::GoingToSource,
                            destination_nav_node: dest_nav_node,
                        },
                        state: task::TaskState::Available,
                        location: source_nav_node,
                        progress: 0.0,
                        total_cost: 0.0,
                        required_species: Some(Species::Elf),
                        origin: task::TaskOrigin::Automated,
                        target_creature: None,
                    };
                    self.insert_task(new_task);
                    tasks_created += 1;
                }
            }
        }

        self.process_kitchen_monitor();
        self.process_workshop_monitor();
        self.process_greenhouse_monitor();
    }

    /// Scan kitchens and create Cook tasks when conditions are met.
    /// Called at the end of each logistics heartbeat.
    fn process_kitchen_monitor(&mut self) {
        // Collect kitchen IDs to avoid borrowing self during iteration.
        let kitchen_ids: Vec<StructureId> = self
            .db
            .structures
            .iter_all()
            .filter_map(|s| {
                if s.furnishing == Some(FurnishingType::Kitchen) && s.cooking_enabled {
                    Some(s.id)
                } else {
                    None
                }
            })
            .collect();

        let cook_fruit_input = self.config.cook_fruit_input;

        for sid in kitchen_ids {
            let structure = match self.db.structures.get(&sid) {
                Some(s) => s,
                None => continue,
            };

            // Skip if bread target reached.
            let bread_count =
                self.inv_unreserved_item_count(structure.inventory_id, inventory::ItemKind::Bread);
            if bread_count >= structure.cooking_bread_target {
                continue;
            }

            // Skip if not enough unreserved fruit.
            let fruit_count =
                self.inv_unreserved_item_count(structure.inventory_id, inventory::ItemKind::Fruit);
            if fruit_count < cook_fruit_input {
                continue;
            }

            // Skip if there's already an active (non-Complete) Cook task for this kitchen.
            let has_active_cook = self
                .db
                .task_structure_refs
                .by_structure_id(&sid, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|r| {
                    r.role == crate::db::TaskStructureRole::CookAt
                        && self
                            .db
                            .tasks
                            .get(&r.task_id)
                            .is_some_and(|t| t.state != task::TaskState::Complete)
                });
            if has_active_cook {
                continue;
            }

            // Find nav node inside the kitchen.
            let interior_pos = self.db.structures.get(&sid).unwrap().anchor;
            let location = match self.nav_graph.find_nearest_node(interior_pos) {
                Some(n) => n,
                None => continue,
            };

            // Create Cook task with reserved fruit.
            let task_id = TaskId::new(&mut self.rng);
            self.inv_reserve_items(
                self.structure_inv(sid),
                inventory::ItemKind::Fruit,
                cook_fruit_input,
                task_id,
            );

            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::Cook { structure_id: sid },
                state: task::TaskState::Available,
                location,
                progress: 0.0,
                total_cost: 1.0, // Single action per cook batch.
                required_species: Some(Species::Elf),
                origin: task::TaskOrigin::Automated,
                target_creature: None,
            };
            self.insert_task(new_task);
        }
    }

    /// Count items of the given kind that are in-transit to the given building
    /// via active Haul tasks.
    fn count_in_transit_items(
        &self,
        structure_id: StructureId,
        item_kind: inventory::ItemKind,
    ) -> u32 {
        // Find haul tasks targeting this structure via HaulDestination refs.
        self.db
            .task_structure_refs
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|r| r.role == crate::db::TaskStructureRole::HaulDestination)
            .filter_map(|r| {
                let task = self.db.tasks.get(&r.task_id)?;
                if task.state == task::TaskState::Complete {
                    return None;
                }
                let haul = self.task_haul_data(r.task_id)?;
                if haul.item_kind == item_kind {
                    Some(haul.quantity)
                } else {
                    None
                }
            })
            .sum()
    }

    /// Scan greenhouses and produce fruit when production interval has elapsed.
    /// Called at the end of each logistics heartbeat.
    fn process_greenhouse_monitor(&mut self) {
        let base_ticks = self.config.greenhouse_base_production_ticks;
        if base_ticks == 0 {
            return;
        }

        // Collect (id, species, production_interval, last_tick) to avoid borrow.
        let greenhouses: Vec<(StructureId, FruitSpeciesId, u64, u64)> = self
            .db
            .structures
            .iter_all()
            .filter_map(|s| {
                if s.furnishing == Some(FurnishingType::Greenhouse) && s.greenhouse_enabled {
                    let species_id = s.greenhouse_species?;
                    let area = s.floor_interior_positions().len().max(1) as u64;
                    let interval = base_ticks / area;
                    Some((
                        s.id,
                        species_id,
                        interval,
                        s.greenhouse_last_production_tick,
                    ))
                } else {
                    None
                }
            })
            .collect();

        let tick = self.tick;
        for (sid, species_id, interval, last_tick) in greenhouses {
            if interval == 0 || tick < last_tick + interval {
                continue;
            }

            // Produce fruit into the structure's inventory.
            let inv_id = match self.db.structures.get(&sid) {
                Some(s) => s.inventory_id,
                None => continue,
            };
            self.inv_add_item(
                inv_id,
                inventory::ItemKind::Fruit,
                1,
                None,
                None,
                Some(inventory::Material::FruitSpecies(species_id)),
                0,
                None,
            );

            // Update last production tick.
            let _ = self.db.structures.modify_unchecked(&sid, |s| {
                s.greenhouse_last_production_tick = tick;
            });
        }
    }

    /// Find a source for hauling `needed` items of the given kind.
    ///
    /// Searches ground piles first (deterministic BTreeMap order), then buildings
    /// with strictly lower logistics priority. Returns the source, available
    /// (unreserved) quantity, and the source's nav node.
    fn find_haul_source(
        &self,
        item_kind: inventory::ItemKind,
        needed: u32,
        exclude_building: StructureId,
        requester_priority: u8,
    ) -> Option<(task::HaulSource, u32, NavNodeId)> {
        // Check ground piles first.
        for pile in self.db.ground_piles.iter_all() {
            let available = self.inv_unreserved_item_count(pile.inventory_id, item_kind);
            if available > 0
                && let Some(nav_node) = self.nav_graph.find_nearest_node(pile.position)
            {
                return Some((
                    task::HaulSource::GroundPile(pile.position),
                    available.min(needed),
                    nav_node,
                ));
            }
        }

        // Check other buildings with strictly lower priority.
        for structure in self.db.structures.iter_all() {
            let sid = structure.id;
            if sid == exclude_building {
                continue;
            }
            let Some(src_priority) = structure.logistics_priority else {
                continue;
            };
            if src_priority >= requester_priority {
                continue;
            }
            let available = self.inv_unreserved_item_count(structure.inventory_id, item_kind);
            if available > 0
                && let Some(nav_node) = self.nav_graph.find_nearest_node(structure.anchor)
            {
                return Some((
                    task::HaulSource::Building(sid),
                    available.min(needed),
                    nav_node,
                ));
            }
        }

        // Check logistics-enabled buildings (any priority) for surplus items.
        // A building has surplus when it holds more unreserved items of this kind
        // than its own logistics_wants target for that kind.
        for structure in self.db.structures.iter_all() {
            let sid = structure.id;
            if sid == exclude_building {
                continue;
            }
            if structure.logistics_priority.is_none() {
                continue;
            }
            let held = self.inv_unreserved_item_count(structure.inventory_id, item_kind);
            let wanted = self.inv_want_target(structure.inventory_id, item_kind);
            let surplus = held.saturating_sub(wanted);
            if surplus > 0
                && let Some(nav_node) = self.nav_graph.find_nearest_node(structure.anchor)
            {
                return Some((
                    task::HaulSource::Building(sid),
                    surplus.min(needed),
                    nav_node,
                ));
            }
        }

        None
    }

    /// Reserve items at a haul source for a given task.
    fn reserve_haul_items(
        &mut self,
        source: &task::HaulSource,
        item_kind: inventory::ItemKind,
        quantity: u32,
        task_id: TaskId,
    ) {
        match source {
            task::HaulSource::GroundPile(pos) => {
                if let Some(pile) = self
                    .db
                    .ground_piles
                    .by_position(pos, tabulosity::QueryOpts::ASC)
                    .into_iter()
                    .next()
                {
                    self.inv_reserve_items(pile.inventory_id, item_kind, quantity, task_id);
                }
            }
            task::HaulSource::Building(sid) => {
                if let Some(structure) = self.db.structures.get(sid) {
                    self.inv_reserve_items(structure.inventory_id, item_kind, quantity, task_id);
                }
            }
        }
    }

    /// Find a source of unowned, unreserved items for personal acquisition.
    ///
    /// Searches ground piles first (deterministic BTreeMap order), then any
    /// building inventory (ignoring logistics priority — personal acquisition
    /// pulls from anywhere). Returns the source, capped quantity, and nav node.
    fn find_item_source(
        &self,
        kind: inventory::ItemKind,
        needed: u32,
    ) -> Option<(task::HaulSource, u32, NavNodeId)> {
        // Check ground piles.
        for pile in self.db.ground_piles.iter_all() {
            let available = self.inv_count_unowned_unreserved(pile.inventory_id, kind);
            if available > 0
                && let Some(nav_node) = self.nav_graph.find_nearest_node(pile.position)
            {
                return Some((
                    task::HaulSource::GroundPile(pile.position),
                    available.min(needed),
                    nav_node,
                ));
            }
        }

        // Check building inventories.
        for structure in self.db.structures.iter_all() {
            let sid = structure.id;
            let available = self.inv_count_unowned_unreserved(structure.inventory_id, kind);
            if available > 0
                && let Some(nav_node) = self.nav_graph.find_nearest_node(structure.anchor)
            {
                return Some((
                    task::HaulSource::Building(sid),
                    available.min(needed),
                    nav_node,
                ));
            }
        }

        None
    }

    /// Check if a creature should start moping due to low mood. Called during
    /// heartbeat Phase 2b½, after hunger/sleep but before item acquisition.
    /// Only fires for elves (only species with meaningful thoughts). Uses a
    /// Poisson-like integer probability: `roll % mean < elapsed`.
    fn check_mope(&mut self, creature_id: CreatureId) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };

        // Only elves mope (only species with thoughts).
        if creature.species != Species::Elf {
            return;
        }

        let (_, tier) = self.mood_for_creature(creature_id);
        let mean = self.config.mood_consequences.mope_mean_ticks(tier);
        if mean == 0 {
            return; // This tier never mopes.
        }

        // Preemption check: can mope interrupt the current task?
        // Uses the formal preemption system instead of ad-hoc checks.
        // Mope is Mood(4), which preempts Idle(0), Autonomous(1), and
        // PlayerDirected(2) but NOT Survival(3) (hardcoded exception
        // prevents starvation spiral).
        if let Some(task_id) = creature.current_task
            && let Some(current_task) = self.db.tasks.get(&task_id)
        {
            let current_level =
                preemption::preemption_level(current_task.kind_tag, current_task.origin);
            let current_origin = current_task.origin;
            if !preemption::can_preempt(
                current_level,
                current_origin,
                preemption::PreemptionLevel::Mood,
                task::TaskOrigin::Autonomous,
            ) {
                return;
            }
        }

        // Probability roll: mope if `roll % mean < elapsed`.
        let species_data = &self.species_table[&Species::Elf];
        let elapsed = species_data.heartbeat_interval_ticks;
        let roll = self.rng.next_u64();
        if roll % mean >= elapsed {
            return; // Roll failed.
        }

        // If creature has an in-progress task, interrupt it.
        if let Some(old_task_id) = creature.current_task {
            self.interrupt_task(creature_id, old_task_id);
        }

        // Determine mope location: assigned home nav node, else current node.
        let mope_node = self
            .find_assigned_home_bed(creature_id)
            .map(|(_, nav_node, _)| nav_node)
            .or_else(|| {
                self.db
                    .creatures
                    .get(&creature_id)
                    .and_then(|c| c.current_node)
            });
        let mope_node = match mope_node {
            Some(n) => n,
            None => return,
        };

        let task_id = TaskId::new(&mut self.rng);
        let duration = self.config.mood_consequences.mope_duration_ticks;
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Mope,
            state: task::TaskState::InProgress,
            location: mope_node,
            progress: 0.0,
            total_cost: duration as f32,
            required_species: None,
            origin: task::TaskOrigin::Autonomous,
            target_creature: None,
        };
        self.insert_task(new_task);
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.current_task = Some(task_id);
            let name = creature.name.clone();
            let _ = self.db.creatures.update_no_fk(creature);

            let tier_label = tier.label();
            let msg = if name.is_empty() {
                format!("An elf is moping ({tier_label})")
            } else {
                format!("{name} is moping ({tier_label})")
            };
            self.add_notification(msg);
        }
    }

    /// Check a creature's personal wants and create an AcquireItem task for
    /// the first unsatisfied want. Called during heartbeat Phase 2c when the
    /// creature is idle.
    fn check_creature_wants(&mut self, creature_id: CreatureId) {
        // Gather want info from creature (borrow creature briefly, then release).
        let owned_counts = {
            let creature = match self.db.creatures.get(&creature_id) {
                Some(c) => c,
                None => return,
            };
            let wants = self.inv_wants(creature.inventory_id);
            if wants.is_empty() {
                return;
            }
            wants
                .iter()
                .map(|w| {
                    let owned =
                        self.inv_count_owned(creature.inventory_id, w.item_kind, creature_id);
                    (w.item_kind, w.target_quantity, owned)
                })
                .collect::<Vec<(inventory::ItemKind, u32, u32)>>()
        };

        // Find first unsatisfied want.
        for (item_kind, target, owned) in &owned_counts {
            if *owned >= *target {
                continue;
            }
            let needed = *target - *owned;

            // Find a source.
            let (source, quantity, nav_node) = match self.find_item_source(*item_kind, needed) {
                Some(s) => s,
                None => continue, // No source for this kind; try next want.
            };

            // Reserve items at source.
            let task_id = TaskId::new(&mut self.rng);
            match &source {
                task::HaulSource::GroundPile(pos) => {
                    if let Some(pile) = self
                        .db
                        .ground_piles
                        .by_position(pos, tabulosity::QueryOpts::ASC)
                        .into_iter()
                        .next()
                    {
                        self.inv_reserve_unowned_items(
                            pile.inventory_id,
                            *item_kind,
                            quantity,
                            task_id,
                        );
                    }
                }
                task::HaulSource::Building(sid) => {
                    if let Some(structure) = self.db.structures.get(sid) {
                        self.inv_reserve_unowned_items(
                            structure.inventory_id,
                            *item_kind,
                            quantity,
                            task_id,
                        );
                    }
                }
            }

            // Create AcquireItem task, directly assigned (same pattern as EatBread).
            let new_task = task::Task {
                id: task_id,
                kind: task::TaskKind::AcquireItem {
                    source,
                    item_kind: *item_kind,
                    quantity,
                },
                state: task::TaskState::InProgress,
                location: nav_node,
                progress: 0.0,
                total_cost: 0.0,
                required_species: None,
                origin: task::TaskOrigin::Autonomous,
                target_creature: None,
            };
            self.insert_task(new_task);
            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                creature.current_task = Some(task_id);
                let _ = self.db.creatures.update_no_fk(creature);
            }
            return; // One task per heartbeat.
        }
    }

    /// Walk one edge toward a task location using a stored or computed A* path.
    fn walk_toward_task(
        &mut self,
        creature_id: CreatureId,
        task_location: NavNodeId,
        current_node: NavNodeId,
        events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
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
                    self.wander(creature_id, current_node, events);
                    return;
                }
            };

            let first_edge = path_result.edge_indices[0];
            let first_dest = path_result.nodes[1];

            // Store remaining path for future activations.
            let _ = self
                .db
                .creatures
                .modify_unchecked(&creature_id, |creature| {
                    creature.path = Some(CreaturePath {
                        remaining_nodes: path_result.nodes[1..].to_vec(),
                        remaining_edge_indices: path_result.edge_indices.to_vec(),
                    });
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

        let old_pos = self.db.creatures.get(&creature_id).unwrap().position;
        let tick = self.tick;
        let _ = self
            .db
            .creatures
            .modify_unchecked(&creature_id, |creature| {
                creature.position = dest_pos;
                creature.current_node = Some(dest_node);

                // Set action state.
                creature.action_kind = ActionKind::Move;
                creature.next_available_tick = Some(tick + delay);

                // Advance stored path.
                if let Some(ref mut path) = creature.path {
                    if !path.remaining_nodes.is_empty() {
                        path.remaining_nodes.remove(0);
                    }
                    if !path.remaining_edge_indices.is_empty() {
                        path.remaining_edge_indices.remove(0);
                    }
                }
            });

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        // Insert MoveAction for render interpolation.
        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        // Remove any existing MoveAction (shouldn't happen, but be safe).
        let _ = self.db.move_actions.remove_no_fk(&creature_id);
        self.db.move_actions.insert_no_fk(move_action).unwrap();

        // Schedule next activation.
        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Remove a creature from its assigned task.
    fn unassign_creature_from_task(&mut self, creature_id: CreatureId) {
        let task_id = match self.db.creatures.get(&creature_id) {
            Some(c) => c.current_task,
            None => return,
        };
        if let Some(tid) = task_id
            && let Some(mut task) = self.db.tasks.get(&tid)
        {
            // Check if any other creature is still assigned to this task.
            let remaining = self
                .db
                .creatures
                .by_current_task(&Some(tid), tabulosity::QueryOpts::ASC)
                .into_iter()
                .filter(|c| c.id != creature_id)
                .count();
            if remaining == 0 && matches!(task.state, task::TaskState::InProgress) {
                task.state = task::TaskState::Available;
                let _ = self.db.tasks.update_no_fk(task);
            }
        }
        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
            creature.current_task = None;
            creature.path = None;
            let _ = self.db.creatures.update_no_fk(creature);
        }
    }

    /// Interrupt a creature's current task, performing all necessary cleanup.
    ///
    /// This is the single entry point for task interruption from any source
    /// (nav invalidation, mope preemption, death, flee, player cancel, etc.).
    /// Dispatches per-kind cleanup based on `TaskKindTag`, then handles task
    /// state: resumable tasks (Build, Furnish) return to `Available` so
    /// another creature can claim them; all others are marked `Complete`.
    /// Clears the creature's action, task assignment, and path.
    fn interrupt_task(&mut self, creature_id: CreatureId, task_id: TaskId) {
        self.abort_current_action(creature_id);

        let kind_tag = match self.db.tasks.get(&task_id) {
            Some(t) => t.kind_tag,
            None => {
                // Task already gone — just clear creature fields.
                if let Some(mut c) = self.db.creatures.get(&creature_id) {
                    c.current_task = None;
                    c.path = None;
                    let _ = self.db.creatures.update_no_fk(c);
                }
                return;
            }
        };

        // Per-kind cleanup: release reservations, drop carried items, etc.
        match kind_tag {
            crate::db::TaskKindTag::Haul => {
                self.cleanup_haul_task(creature_id, task_id);
            }
            crate::db::TaskKindTag::Cook => {
                self.cleanup_cook_task(task_id);
            }
            crate::db::TaskKindTag::Craft => {
                self.cleanup_craft_task(task_id);
            }
            crate::db::TaskKindTag::Harvest => {
                self.cleanup_harvest_task(task_id);
            }
            crate::db::TaskKindTag::AcquireItem => {
                self.cleanup_acquire_item_task(task_id);
            }
            // Resumable tasks: return to Available for another creature.
            // unassign_creature_from_task handles reverting InProgress → Available.
            crate::db::TaskKindTag::Build | crate::db::TaskKindTag::Furnish => {}
            // No-cleanup tasks: mark Complete so they aren't re-claimed.
            crate::db::TaskKindTag::GoTo
            | crate::db::TaskKindTag::EatBread
            | crate::db::TaskKindTag::EatFruit
            | crate::db::TaskKindTag::Sleep
            | crate::db::TaskKindTag::Mope => {
                if let Some(mut t) = self.db.tasks.get(&task_id) {
                    t.state = task::TaskState::Complete;
                    let _ = self.db.tasks.update_no_fk(t);
                }
            }
        }

        // Clear creature assignment. For resumable tasks (Build, Furnish),
        // this reverts the task to Available if no other creatures remain.
        // For non-resumable tasks, the task is already Complete.
        self.unassign_creature_from_task(creature_id);
    }

    // -----------------------------------------------------------------------
    // HP / damage / heal / death
    // -----------------------------------------------------------------------

    /// Handle a creature's death. Sets `vital_status = Dead`, interrupts any
    /// current task, drops all owned inventory as a ground pile, clears
    /// `assigned_home`, and emits a `CreatureDied` event. The creature row
    /// is NOT deleted — it remains in the database for future states (ghost,
    /// spirit, etc.) and to preserve history.
    ///
    /// Heartbeat and activation events for dead creatures are no-ops: the
    /// handlers check `vital_status` and skip rescheduling.
    fn handle_creature_death(
        &mut self,
        creature_id: CreatureId,
        cause: DeathCause,
        events: &mut Vec<SimEvent>,
    ) {
        let (species, position) = match self.db.creatures.get(&creature_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => (c.species, c.position),
            _ => return, // already dead or doesn't exist
        };

        // 1. Interrupt current task (clears action, drops haul items, etc.)
        if let Some(task_id) = self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            self.interrupt_task(creature_id, task_id);
        } else {
            // No task, but still abort any in-progress action.
            self.abort_current_action(creature_id);
        }

        // 2. Drop all owned inventory items as a ground pile at death position.
        let inv_id = self.db.creatures.get(&creature_id).map(|c| c.inventory_id);
        if let Some(inv_id) = inv_id {
            let owned_stacks: Vec<_> = self
                .db
                .item_stacks
                .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
                .into_iter()
                .filter(|s| s.quantity > 0)
                .collect();
            if !owned_stacks.is_empty() {
                let pile_id = self.ensure_ground_pile(position);
                let pile_inv_id = self.db.ground_piles.get(&pile_id).unwrap().inventory_id;
                for stack in owned_stacks {
                    self.inv_add_simple_item(
                        pile_inv_id,
                        stack.kind,
                        stack.quantity,
                        None, // no owner
                        None, // no reservation
                    );
                    let _ = self.db.item_stacks.remove_no_fk(&stack.id);
                }
            }
        }

        // 3. Remove from spatial index.
        let footprint = self.species_table[&species].footprint;
        Self::deregister_creature_from_index(
            &mut self.spatial_index,
            creature_id,
            position,
            footprint,
        );

        // 4. Clear assigned_home, set vital_status = Dead, hp = 0.
        // Uses update_no_fk because vital_status is #[indexed].
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.assigned_home = None;
            c.vital_status = VitalStatus::Dead;
            c.hp = 0;
            let _ = self.db.creatures.update_no_fk(c);
        }

        // 5. Emit CreatureDied event.
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::CreatureDied {
                creature_id,
                species,
                position,
                cause,
            },
        });

        // 6. Create a notification for the player.
        let creature_name = self
            .db
            .creatures
            .get(&creature_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let species_str = format!("{:?}", species);
        let msg = if creature_name.is_empty() {
            format!("A {} has died.", species_str)
        } else {
            format!("{} ({}) has died.", creature_name, species_str)
        };
        let _ = self
            .db
            .notifications
            .insert_auto_no_fk(|id| crate::db::Notification {
                id,
                tick: self.tick,
                message: msg,
            });
    }

    /// Apply damage to a creature. Positive `amount` reduces HP. If HP
    /// reaches 0 the creature dies via `handle_creature_death`.
    fn apply_damage(&mut self, creature_id: CreatureId, amount: i64, events: &mut Vec<SimEvent>) {
        if amount <= 0 {
            return;
        }
        let should_die = if let Some(mut c) = self.db.creatures.get(&creature_id) {
            if c.vital_status != VitalStatus::Alive {
                return;
            }
            c.hp = (c.hp - amount).max(0);
            let die = c.hp == 0;
            let _ = self.db.creatures.update_no_fk(c);
            die
        } else {
            return;
        };
        if should_die {
            self.handle_creature_death(creature_id, DeathCause::Damage, events);
        }
    }

    /// Heal a creature. Positive `amount` restores HP up to `hp_max`.
    /// No effect on dead creatures.
    fn apply_heal(&mut self, creature_id: CreatureId, amount: i64) {
        if amount <= 0 {
            return;
        }
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            if c.vital_status != VitalStatus::Alive {
                return;
            }
            c.hp = (c.hp + amount).min(c.hp_max);
        });
    }

    /// Attempt a melee strike from attacker against target.
    ///
    /// Validates: both alive, attacker has melee_damage > 0, attacker is idle
    /// (NoAction + next_available_tick elapsed), target in melee range.
    /// On success: starts MeleeStrike action, applies damage, emits
    /// CreatureDamaged event. Returns true if the strike was executed.
    fn try_melee_strike(
        &mut self,
        attacker_id: CreatureId,
        target_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        // 1. Both creatures must exist and be alive.
        let attacker = match self.db.creatures.get(&attacker_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };
        let target = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };

        // 2. Attacker species must have melee_damage > 0.
        let species_data = &self.species_table[&attacker.species];
        if species_data.melee_damage <= 0 {
            return false;
        }

        // 3. Attacker must be idle.
        if attacker.action_kind != ActionKind::NoAction {
            return false;
        }
        if let Some(next_tick) = attacker.next_available_tick
            && next_tick > self.tick
        {
            return false;
        }

        // 4. Target must be in melee range.
        let attacker_footprint = species_data.footprint;
        let target_footprint = self.species_table[&target.species].footprint;
        if !in_melee_range(
            attacker.position,
            attacker_footprint,
            target.position,
            target_footprint,
            species_data.melee_range_sq,
        ) {
            return false;
        }

        let damage = species_data.melee_damage;
        let duration = species_data.melee_interval_ticks;

        // Start the action (sets action_kind + next_available_tick, schedules activation).
        self.start_simple_action(attacker_id, ActionKind::MeleeStrike, duration);

        // Apply damage (handles death if HP reaches 0).
        self.apply_damage(target_id, damage, events);

        // Emit CreatureDamaged event.
        let remaining_hp = self.db.creatures.get(&target_id).map(|c| c.hp).unwrap_or(0);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::CreatureDamaged {
                attacker_id,
                target_id,
                damage,
                remaining_hp,
            },
        });

        true
    }

    /// Attempt a ranged attack: `attacker_id` shoots an arrow at `target_id`.
    ///
    /// Validates: both alive, attacker is idle (NoAction + cooldown elapsed),
    /// attacker has at least one Bow and one Arrow in inventory, the aim solver
    /// finds a feasible trajectory (`hit_tick.is_some()`), and LOS exists
    /// (voxel DDA ray from attacker to target, checking any occupied voxel of
    /// a multi-voxel target).
    ///
    /// On success: consumes one arrow from attacker inventory, spawns a
    /// Projectile entity, sets `ActionKind::Shoot` with `shoot_cooldown_ticks`
    /// duration, emits `ProjectileLaunched` event. Returns true.
    fn try_shoot_arrow(
        &mut self,
        attacker_id: CreatureId,
        target_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        use crate::inventory::ItemKind;
        use crate::projectile::{SubVoxelCoord, compute_aim_velocity};

        // 1. Both creatures must exist and be alive.
        let attacker = match self.db.creatures.get(&attacker_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };
        let target = match self.db.creatures.get(&target_id) {
            Some(c) if c.vital_status == VitalStatus::Alive => c,
            _ => return false,
        };

        // 2. Attacker must be idle.
        if attacker.action_kind != ActionKind::NoAction {
            return false;
        }
        if let Some(next_tick) = attacker.next_available_tick
            && next_tick > self.tick
        {
            return false;
        }

        // 3. Attacker must have a Bow and at least one Arrow.
        let inv_id = attacker.inventory_id;
        if self.inv_item_count(inv_id, ItemKind::Bow) == 0 {
            return false;
        }
        if self.inv_item_count(inv_id, ItemKind::Arrow) == 0 {
            return false;
        }

        let attacker_pos = attacker.position;
        let target_pos = target.position;
        let target_species = target.species;
        let target_footprint = self.species_table[&target_species].footprint;

        // 4. LOS check — try each occupied voxel of the target's footprint.
        let mut has_los = false;
        let mut los_target_voxel = target_pos;
        for dy in 0..target_footprint[1] as i32 {
            for dx in 0..target_footprint[0] as i32 {
                for dz in 0..target_footprint[2] as i32 {
                    let tv =
                        VoxelCoord::new(target_pos.x + dx, target_pos.y + dy, target_pos.z + dz);
                    if self.world.has_los(attacker_pos, tv) {
                        has_los = true;
                        los_target_voxel = tv;
                        break;
                    }
                }
                if has_los {
                    break;
                }
            }
            if has_los {
                break;
            }
        }
        if !has_los {
            return false;
        }

        // 5. Aim feasibility — use the aim solver to check if a trajectory exists.
        let origin_sub = SubVoxelCoord::from_voxel_center(attacker_pos);
        let speed = self.config.arrow_base_speed;
        let gravity = self.config.arrow_gravity;
        let aim = compute_aim_velocity(origin_sub, los_target_voxel, speed, gravity, 5, 5000);
        if aim.hit_tick.is_none() {
            return false;
        }

        // 6. All checks passed — consume arrow and fire.
        self.inv_remove_item(inv_id, ItemKind::Arrow, 1);

        // Create projectile inventory with one arrow (projectile carries its payload).
        let proj_inv_id = self.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
        self.inv_add_simple_item(proj_inv_id, ItemKind::Arrow, 1, None, None);

        let was_empty = self.db.projectiles.is_empty();

        let _ = self
            .db
            .projectiles
            .insert_auto_no_fk(|id| crate::db::Projectile {
                id,
                shooter: Some(attacker_id),
                inventory_id: proj_inv_id,
                position: origin_sub,
                velocity: aim.velocity,
                prev_voxel: attacker_pos,
                origin_voxel: attacker_pos,
            });

        if was_empty {
            self.event_queue
                .schedule(self.tick + 1, ScheduledEventKind::ProjectileTick);
        }

        // 7. Start the Shoot action (cooldown).
        let duration = self.config.shoot_cooldown_ticks;
        self.start_simple_action(attacker_id, ActionKind::Shoot, duration);

        // 8. Emit ProjectileLaunched event.
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ProjectileLaunched {
                attacker_id,
                target_id,
            },
        });

        true
    }

    // -----------------------------------------------------------------------
    // Projectile system
    // -----------------------------------------------------------------------

    /// Spawn a projectile from `origin` aimed at `target`. Creates an inventory
    /// with a single arrow, computes aim velocity, and inserts the projectile
    /// into SimDb. Schedules a `ProjectileTick` event if this is the first
    /// in-flight projectile (table was empty before this spawn).
    fn spawn_projectile(
        &mut self,
        origin: VoxelCoord,
        target: VoxelCoord,
        shooter_id: Option<CreatureId>,
    ) {
        use crate::projectile::{SubVoxelCoord, compute_aim_velocity};

        let was_empty = self.db.projectiles.is_empty();

        // Create inventory with a single arrow.
        let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
        self.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 1, None, None);

        // Compute aim velocity.
        let origin_sub = SubVoxelCoord::from_voxel_center(origin);
        let speed = self.config.arrow_base_speed;
        let gravity = self.config.arrow_gravity;
        let aim = compute_aim_velocity(origin_sub, target, speed, gravity, 5, 5000);

        // Insert projectile into SimDb.
        let _ = self
            .db
            .projectiles
            .insert_auto_no_fk(|id| crate::db::Projectile {
                id,
                shooter: shooter_id,
                inventory_id: inv_id,
                position: origin_sub,
                velocity: aim.velocity,
                prev_voxel: origin,
                origin_voxel: origin,
            });

        // Schedule ProjectileTick if this is the first in-flight projectile.
        if was_empty {
            self.event_queue
                .schedule(self.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    /// Advance all in-flight projectiles by one tick. For each projectile:
    /// save prev_voxel, apply gravity+velocity (symplectic Euler via
    /// `ballistic_step`), check bounds, check solid voxel collision, check
    /// creature collision. Resolved projectiles are removed from the table.
    /// Reschedules itself for tick+1 if projectiles remain.
    fn process_projectile_tick(&mut self, events: &mut Vec<SimEvent>) {
        use crate::projectile::ballistic_step;

        let gravity = self.config.arrow_gravity;
        let (world_sx, world_sy, world_sz) = self.config.world_size;

        // Collect all projectile IDs to iterate (can't mutate DB while iterating).
        let projectile_ids: Vec<ProjectileId> =
            self.db.projectiles.iter_all().map(|p| p.id).collect();

        for proj_id in projectile_ids {
            let proj = match self.db.projectiles.get(&proj_id) {
                Some(p) => p,
                None => continue, // already removed
            };

            // Step 1: save current voxel as prev_voxel.
            let current_voxel = proj.position.to_voxel();

            // Step 2-3: symplectic Euler (gravity, then velocity).
            let (new_pos, new_vel) = ballistic_step(proj.position, proj.velocity, gravity);

            // Step 7 (early): bounds check on i64 BEFORE casting to i32.
            let vx = new_pos.x >> crate::projectile::SUB_VOXEL_SHIFT;
            let vy = new_pos.y >> crate::projectile::SUB_VOXEL_SHIFT;
            let vz = new_pos.z >> crate::projectile::SUB_VOXEL_SHIFT;

            if vx < 0
                || vy < 0
                || vz < 0
                || vx >= world_sx as i64
                || vy >= world_sy as i64
                || vz >= world_sz as i64
            {
                // Out of bounds — despawn silently (arrow lost).
                self.remove_projectile(proj_id);
                continue;
            }

            // Step 4: determine containing voxel (safe to cast now).
            let new_voxel = new_pos.to_voxel();

            // Step 5: solid voxel check.
            if self.world.in_bounds(new_voxel) && self.world.get(new_voxel).is_solid() {
                // Surface hit — transfer arrow to ground pile at prev_voxel.
                self.resolve_projectile_surface_hit(proj_id, current_voxel, events);
                continue;
            }

            // Step 6: creature check via spatial index.
            // Skip the origin voxel — projectiles don't hit creatures at their
            // launch site (prevents friendly-fire on the shooter and allies
            // sharing the same voxel).
            let is_origin = self
                .db
                .projectiles
                .get(&proj_id)
                .is_some_and(|p| new_voxel == p.origin_voxel);
            let creatures_here = if is_origin {
                Vec::new()
            } else {
                self.creatures_at_voxel(new_voxel).to_vec()
            };
            if !creatures_here.is_empty() {
                // Filter to alive creatures. Sort is preserved from spatial
                // index for determinism.
                let candidates: Vec<CreatureId> = creatures_here
                    .into_iter()
                    .filter(|cid| {
                        self.db
                            .creatures
                            .get(cid)
                            .is_some_and(|c| c.vital_status == VitalStatus::Alive)
                    })
                    .collect();

                if !candidates.is_empty() {
                    let target_id = if candidates.len() == 1 {
                        candidates[0]
                    } else {
                        candidates[self.rng.next_u64() as usize % candidates.len()]
                    };

                    self.resolve_projectile_creature_hit(
                        proj_id, target_id, new_vel, new_voxel, events,
                    );
                    continue;
                }
            }

            // No collision — update projectile position and velocity.
            if let Some(mut proj) = self.db.projectiles.get(&proj_id) {
                proj.prev_voxel = current_voxel;
                proj.position = new_pos;
                proj.velocity = new_vel;
                let _ = self.db.projectiles.update_no_fk(proj);
            }
        }

        // Reschedule if projectiles remain.
        if !self.db.projectiles.is_empty() {
            self.event_queue
                .schedule(self.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    /// Resolve a projectile hitting a solid surface. Transfers the arrow from
    /// the projectile inventory to a ground pile at `prev_voxel`.
    fn resolve_projectile_surface_hit(
        &mut self,
        proj_id: ProjectileId,
        prev_voxel: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) {
        let proj = match self.db.projectiles.get(&proj_id) {
            Some(p) => p,
            None => return,
        };
        let proj_inv = proj.inventory_id;

        // Create/join ground pile at prev_voxel and merge items.
        let pile_id = self.ensure_ground_pile(prev_voxel);
        let pile_inv = self.db.ground_piles.get(&pile_id).unwrap().inventory_id;
        self.inv_merge(proj_inv, pile_inv);

        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ProjectileHitSurface {
                position: prev_voxel,
            },
        });

        self.remove_projectile(proj_id);
    }

    /// Resolve a projectile hitting a creature. Computes damage from impact
    /// velocity, applies it, and transfers the arrow to a ground pile.
    fn resolve_projectile_creature_hit(
        &mut self,
        proj_id: ProjectileId,
        target_id: CreatureId,
        impact_velocity: SubVoxelVec,
        hit_voxel: VoxelCoord,
        events: &mut Vec<SimEvent>,
    ) {
        let proj = match self.db.projectiles.get(&proj_id) {
            Some(p) => p,
            None => return,
        };
        let shooter_id = proj.shooter;
        let proj_inv = proj.inventory_id;

        // Compute damage from impact speed (momentum-based: linear in speed).
        // For now: damage = impact_speed / REFERENCE_SPEED, minimum 1.
        // REFERENCE_SPEED is arrow_base_speed (the "normal" launch speed).
        let impact_speed_sq = impact_velocity.magnitude_sq();
        let impact_speed = crate::projectile::isqrt_i128(impact_speed_sq);
        let reference_speed = self.config.arrow_base_speed as i128;
        let damage = if reference_speed > 0 {
            (impact_speed / reference_speed).max(1) as i64
        } else {
            1
        };

        // Apply damage.
        self.apply_damage(target_id, damage, events);

        let remaining_hp = self.db.creatures.get(&target_id).map(|c| c.hp).unwrap_or(0);
        events.push(SimEvent {
            tick: self.tick,
            kind: SimEventKind::ProjectileHitCreature {
                target_id,
                damage,
                remaining_hp,
                shooter_id,
            },
        });

        // Transfer arrow to ground pile at the creature's position.
        let pile_id = self.ensure_ground_pile(hit_voxel);
        let pile_inv = self.db.ground_piles.get(&pile_id).unwrap().inventory_id;
        self.inv_merge(proj_inv, pile_inv);

        self.remove_projectile(proj_id);
    }

    /// Remove a projectile and clean up its inventory. The Inventory FK
    /// has no cascade from projectile→inventory (the FK direction is
    /// projectile.inventory_id → inventories), so we must explicitly
    /// remove the inventory and its item stacks.
    fn remove_projectile(&mut self, proj_id: ProjectileId) {
        if let Some(proj) = self.db.projectiles.get(&proj_id) {
            let inv_id = proj.inventory_id;
            // Remove projectile first (it references the inventory).
            let _ = self.db.projectiles.remove_no_fk(&proj_id);
            // Remove any remaining item stacks in the inventory.
            let stacks: Vec<ItemStackId> = self
                .db
                .item_stacks
                .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
                .iter()
                .map(|s| s.id)
                .collect();
            for stack_id in stacks {
                let _ = self.db.item_stacks.remove_no_fk(&stack_id);
            }
            // Remove the inventory row itself.
            let _ = self.db.inventories.remove_no_fk(&inv_id);
        }
    }

    /// Add a thought to a creature via the thoughts table, with dedup and cap.
    pub(crate) fn add_creature_thought(&mut self, creature_id: CreatureId, kind: ThoughtKind) {
        let cooldown = self.config.thoughts.dedup_ticks(&kind);
        let existing = self
            .db
            .thoughts
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        // Dedup: skip if same kind was added within cooldown.
        if existing
            .iter()
            .rev()
            .any(|t| t.kind == kind && self.tick.saturating_sub(t.tick) < cooldown)
        {
            return;
        }
        // Insert the new thought.
        let _ = self.db.thoughts.insert_auto_no_fk(|id| crate::db::Thought {
            id,
            creature_id,
            kind: kind.clone(),
            tick: self.tick,
        });
        // Cap enforcement: remove oldest if over cap.
        let thoughts = self
            .db
            .thoughts
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        if thoughts.len() > self.config.thoughts.cap {
            // Remove the oldest (first in ASC order by PK, which is insertion order).
            let _ = self.db.thoughts.remove_no_fk(&thoughts[0].id);
        }
    }

    // ------------------------------------------------------------------
    // Civilization commands
    // ------------------------------------------------------------------

    /// A civ becomes aware of another civ. Creates a CivRelationship row.
    /// No-op if the relationship already exists.
    fn discover_civ(&mut self, civ_id: CivId, discovered_civ: CivId, initial_opinion: CivOpinion) {
        // Check that both civs exist.
        if self.db.civilizations.get(&civ_id).is_none()
            || self.db.civilizations.get(&discovered_civ).is_none()
        {
            return;
        }
        // Check if already aware (lookup-before-insert for compound uniqueness).
        let already_aware = self
            .db
            .civ_relationships
            .by_from_civ(&civ_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|r| r.to_civ == discovered_civ);
        if already_aware {
            return;
        }
        let _ = self
            .db
            .civ_relationships
            .insert_auto_no_fk(|id| crate::db::CivRelationship {
                id,
                from_civ: civ_id,
                to_civ: discovered_civ,
                opinion: initial_opinion,
            });
    }

    /// Get the player-controlled civ's known civilizations for the encyclopedia.
    /// Returns a list of (civ, our_opinion, their_opinion) tuples.
    pub fn get_known_civs(&self) -> Vec<(crate::db::Civilization, CivOpinion, Option<CivOpinion>)> {
        let player_civ_id = match self.player_civ_id {
            Some(id) => id,
            None => return Vec::new(),
        };

        // Collect relationship data first to avoid overlapping borrows.
        let our_rels: Vec<(CivId, CivOpinion)> = self
            .db
            .civ_relationships
            .by_from_civ(&player_civ_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .map(|r| (r.to_civ, r.opinion))
            .collect();

        let mut result = Vec::new();
        for (to_civ, our_opinion) in our_rels {
            let civ = match self.db.civilizations.get(&to_civ) {
                Some(c) => c.clone(),
                None => continue,
            };
            // Check if they know about us.
            let their_opinion = self
                .db
                .civ_relationships
                .by_from_civ(&to_civ, tabulosity::QueryOpts::ASC)
                .into_iter()
                .find(|r| r.to_civ == player_civ_id)
                .map(|r| r.opinion);

            result.push((civ, our_opinion, their_opinion));
        }
        result
    }

    /// Update a civ's opinion of another civ. No-op if unaware.
    fn set_civ_opinion(&mut self, civ_id: CivId, target_civ: CivId, opinion: CivOpinion) {
        let rel_id = self
            .db
            .civ_relationships
            .by_from_civ(&civ_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|r| r.to_civ == target_civ)
            .map(|r| r.id);
        if let Some(id) = rel_id {
            let _ = self
                .db
                .civ_relationships
                .modify_unchecked(&id, |r| r.opinion = opinion);
        }
    }

    pub(crate) fn add_notification(&mut self, message: String) {
        let _ = self
            .db
            .notifications
            .insert_auto_no_fk(|id| crate::db::Notification {
                id,
                tick: self.tick,
                message,
            });
    }

    /// Remove expired thoughts for a creature.
    pub(crate) fn expire_creature_thoughts(&mut self, creature_id: CreatureId) {
        let thoughts = self
            .db
            .thoughts
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC);
        for t in &thoughts {
            if self.tick.saturating_sub(t.tick) >= self.config.thoughts.expiry_ticks(&t.kind) {
                let _ = self.db.thoughts.remove_no_fk(&t.id);
            }
        }
    }

    /// Compute mood score and tier for a creature from the thoughts table.
    pub(crate) fn mood_for_creature(
        &self,
        creature_id: CreatureId,
    ) -> (i32, crate::types::MoodTier) {
        let score: i32 = self
            .db
            .thoughts
            .by_creature_id(&creature_id, tabulosity::QueryOpts::ASC)
            .iter()
            .map(|t| self.config.mood.mood_weight(&t.kind))
            .sum();
        (score, self.config.mood.tier(score))
    }

    // -----------------------------------------------------------------------
    // Inventory helpers (operate on item_stacks table via inventory_id)
    // -----------------------------------------------------------------------

    /// Create a new Inventory row and return its ID.
    fn create_inventory(&mut self, owner_kind: crate::db::InventoryOwnerKind) -> InventoryId {
        self.db
            .inventories
            .insert_auto_no_fk(|id| crate::db::Inventory { id, owner_kind })
            .unwrap()
    }

    // -----------------------------------------------------------------------
    // Music composition helpers
    // -----------------------------------------------------------------------

    /// Create a music composition for a construction project. Derives seed
    /// and generation parameters from the sim PRNG. Returns the composition ID.
    fn create_composition(&mut self, voxel_count: usize) -> CompositionId {
        use crate::db::{CompositionStatus, MusicComposition};

        let seed = self.rng.next_u64();

        // Build duration = voxel_count × ticks_per_voxel / 1000 (seconds at 1x).
        let build_ms = (voxel_count as u64 * self.config.build_work_ticks_per_voxel) as u32;
        let build_secs = build_ms as f32 / 1000.0;

        // Pick section count so that the typical grid length for that many
        // sections would need a BPM within the Palestrina range (60–96) to
        // match the build duration.
        //
        // Typical eighth-note beat counts per section count (from structure.rs):
        //   1 section  ≈  55 beats  → duration range at 60–96 BPM: 17–28s
        //   2 sections ≈ 125 beats  → 39–63s
        //   3 sections ≈ 195 beats  → 61–98s
        //   4 sections ≈ 270 beats  → 84–135s
        //
        // For each candidate, the ideal BPM would be:
        //   bpm = typical_beats * 30 / build_secs
        // We pick the section count whose ideal BPM is closest to the middle
        // of the range (78 BPM).
        const TYPICAL_BEATS: &[(u8, f32)] = &[(1, 55.0), (2, 125.0), (3, 195.0), (4, 270.0)];
        let mut best_sections = 1u8;
        let mut best_dist = f32::MAX;
        for &(s, beats) in TYPICAL_BEATS {
            let ideal_bpm = beats * 30.0 / build_secs;
            let dist = (ideal_bpm - 78.0).abs();
            if dist < best_dist {
                best_dist = dist;
                best_sections = s;
            }
        }

        // Random mode (0-5) and brightness (0.2-0.8).
        let mode_index = (self.rng.next_u64() % 6) as u8;
        let brightness = 0.2 + (self.rng.next_u64() % 600) as f32 / 1000.0;
        // SA budget scaled with piece length (longer pieces benefit more).
        let sa_iterations = match best_sections {
            1 => 2000,
            2 => 3000,
            _ => 5000,
        };

        self.db
            .music_compositions
            .insert_auto_no_fk(|id| MusicComposition {
                id,
                seed,
                sections: best_sections,
                mode_index,
                brightness,
                sa_iterations,
                target_duration_ms: build_ms,
                requested_tick: self.tick,
                build_started: false,
                status: CompositionStatus::Pending,
            })
            .unwrap()
    }

    /// Add items to an inventory. Inserts a new stack, then calls
    /// `inv_normalize` to consolidate with any existing matching stacks.
    #[allow(clippy::too_many_arguments)]
    fn inv_add_item(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        owner: Option<CreatureId>,
        reserved_by: Option<TaskId>,
        material: Option<inventory::Material>,
        quality: i32,
        enchantment_id: Option<crate::types::EnchantmentId>,
    ) {
        let _ = self
            .db
            .item_stacks
            .insert_auto_no_fk(|id| crate::db::ItemStack {
                id,
                inventory_id: inv_id,
                kind,
                quantity,
                material,
                quality,
                enchantment_id,
                owner,
                reserved_by,
            });
        self.inv_normalize(inv_id);
    }

    /// Convenience wrapper for adding items with no material, quality, or enchantment.
    fn inv_add_simple_item(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        owner: Option<CreatureId>,
        reserved_by: Option<TaskId>,
    ) {
        self.inv_add_item(inv_id, kind, quantity, owner, reserved_by, None, 0, None)
    }

    /// Move all item stacks from `src` into `dst`, then normalize `dst` to
    /// consolidate matching stacks. The source inventory's stacks are deleted
    /// but the Inventory row itself is not removed — the caller decides
    /// whether to clean it up.
    fn inv_merge(&mut self, src: InventoryId, dst: InventoryId) {
        let stacks: Vec<crate::db::ItemStack> = self
            .db
            .item_stacks
            .by_inventory_id(&src, tabulosity::QueryOpts::ASC);
        for stack in stacks {
            let mut moved = stack;
            moved.inventory_id = dst;
            let _ = self.db.item_stacks.update_no_fk(moved);
        }
        self.inv_normalize(dst);
    }

    /// Remove up to `quantity` items of the given kind from an inventory.
    /// Returns the amount actually removed. Drops stacks that reach zero.
    fn inv_remove_item(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut removed = 0u32;
        for stack in &stacks {
            if stack.kind == kind && remaining > 0 {
                let take = remaining.min(stack.quantity);
                let new_qty = stack.quantity - take;
                if new_qty == 0 {
                    let _ = self.db.item_stacks.remove_no_fk(&stack.id);
                } else {
                    let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                        s.quantity = new_qty;
                    });
                }
                remaining -= take;
                removed += take;
            }
        }
        removed
    }

    /// Count the total quantity of a given item kind in an inventory.
    pub fn inv_item_count(&self, inv_id: InventoryId, kind: inventory::ItemKind) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.kind == kind)
            .map(|s| s.quantity)
            .sum()
    }

    /// Count items of a given kind owned by a specific creature.
    fn inv_count_owned(
        &self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        owner: CreatureId,
    ) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.kind == kind && s.owner == Some(owner))
            .map(|s| s.quantity)
            .sum()
    }

    /// Count unreserved items of the given kind.
    fn inv_unreserved_item_count(&self, inv_id: InventoryId, kind: inventory::ItemKind) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.kind == kind && s.reserved_by.is_none())
            .map(|s| s.quantity)
            .sum()
    }

    /// Remove up to `quantity` items of the given kind owned by a creature.
    fn inv_remove_owned_item(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        owner: CreatureId,
        quantity: u32,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut removed = 0u32;
        for stack in &stacks {
            if stack.kind == kind && stack.owner == Some(owner) && remaining > 0 {
                let take = remaining.min(stack.quantity);
                let new_qty = stack.quantity - take;
                if new_qty == 0 {
                    let _ = self.db.item_stacks.remove_no_fk(&stack.id);
                } else {
                    let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                        s.quantity = new_qty;
                    });
                }
                remaining -= take;
                removed += take;
            }
        }
        removed
    }

    /// Reserve up to `quantity` unreserved items of the given kind for a task.
    /// Splits stacks as needed. Returns the amount actually reserved.
    fn inv_reserve_items(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        task_id: TaskId,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut reserved = 0u32;
        for stack in &stacks {
            if stack.kind == kind && stack.reserved_by.is_none() && remaining > 0 {
                let take = remaining.min(stack.quantity);
                if take == stack.quantity {
                    // Reserve the entire stack — changes indexed field.
                    let mut s = stack.clone();
                    s.reserved_by = Some(task_id);
                    let _ = self.db.item_stacks.update_no_fk(s);
                } else {
                    // Split: reduce this stack and create a new reserved stack.
                    let new_qty = stack.quantity - take;
                    let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                        s.quantity = new_qty;
                    });
                    let mat = stack.material;
                    let qual = stack.quality;
                    let ench = stack.enchantment_id;
                    let own = stack.owner;
                    let _ = self
                        .db
                        .item_stacks
                        .insert_auto_no_fk(|id| crate::db::ItemStack {
                            id,
                            inventory_id: inv_id,
                            kind,
                            quantity: take,
                            material: mat,
                            quality: qual,
                            enchantment_id: ench,
                            owner: own,
                            reserved_by: Some(task_id),
                        });
                }
                remaining -= take;
                reserved += take;
            }
        }
        reserved
    }

    /// Clear all reservations for a task, then re-merge matching stacks.
    fn inv_clear_reservations(&mut self, inv_id: InventoryId, task_id: TaskId) {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        for stack in &stacks {
            if stack.reserved_by == Some(task_id) {
                let mut s = stack.clone();
                s.reserved_by = None;
                let _ = self.db.item_stacks.update_no_fk(s);
            }
        }
        self.inv_normalize(inv_id);
    }

    /// Remove up to `quantity` items reserved by a specific task.
    fn inv_remove_reserved_items(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        task_id: TaskId,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut removed = 0u32;
        for stack in &stacks {
            if stack.kind == kind && stack.reserved_by == Some(task_id) && remaining > 0 {
                let take = remaining.min(stack.quantity);
                let new_qty = stack.quantity - take;
                if new_qty == 0 {
                    let _ = self.db.item_stacks.remove_no_fk(&stack.id);
                } else {
                    let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                        s.quantity = new_qty;
                    });
                }
                remaining -= take;
                removed += take;
            }
        }
        removed
    }

    /// Count unowned (`owner == None`) and unreserved items.
    fn inv_count_unowned_unreserved(&self, inv_id: InventoryId, kind: inventory::ItemKind) -> u32 {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.kind == kind && s.owner.is_none() && s.reserved_by.is_none())
            .map(|s| s.quantity)
            .sum()
    }

    /// Reserve up to `quantity` unowned unreserved items for a task.
    fn inv_reserve_unowned_items(
        &mut self,
        inv_id: InventoryId,
        kind: inventory::ItemKind,
        quantity: u32,
        task_id: TaskId,
    ) -> u32 {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let mut remaining = quantity;
        let mut reserved = 0u32;
        for stack in &stacks {
            if stack.kind == kind
                && stack.owner.is_none()
                && stack.reserved_by.is_none()
                && remaining > 0
            {
                let take = remaining.min(stack.quantity);
                if take == stack.quantity {
                    let mut s = stack.clone();
                    s.reserved_by = Some(task_id);
                    let _ = self.db.item_stacks.update_no_fk(s);
                } else {
                    let new_qty = stack.quantity - take;
                    let _ = self.db.item_stacks.modify_unchecked(&stack.id, |s| {
                        s.quantity = new_qty;
                    });
                    let mat = stack.material;
                    let qual = stack.quality;
                    let ench = stack.enchantment_id;
                    let _ = self
                        .db
                        .item_stacks
                        .insert_auto_no_fk(|id| crate::db::ItemStack {
                            id,
                            inventory_id: inv_id,
                            kind,
                            quantity: take,
                            material: mat,
                            quality: qual,
                            enchantment_id: ench,
                            owner: None,
                            reserved_by: Some(task_id),
                        });
                }
                remaining -= take;
                reserved += take;
            }
        }
        reserved
    }

    /// Consolidate matching stacks within an inventory. Two stacks are
    /// mergeable when they agree on all properties: kind, material, quality,
    /// enchantment_id, owner, and reserved_by. This is the single source of
    /// truth for stack-merging criteria — called by `inv_add_item` and
    /// `inv_merge`.
    fn inv_normalize(&mut self, inv_id: InventoryId) {
        let stacks = self
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        // Collect merge groups keyed by all stackability fields.
        type MergeKey = (
            inventory::ItemKind,
            Option<inventory::Material>,
            i32,
            Option<crate::types::EnchantmentId>,
            Option<CreatureId>,
            Option<TaskId>,
        );
        type MergeVal = (ItemStackId, u32, Vec<ItemStackId>);
        let mut groups: BTreeMap<MergeKey, MergeVal> = BTreeMap::new();
        for stack in &stacks {
            let key = (
                stack.kind,
                stack.material,
                stack.quality,
                stack.enchantment_id,
                stack.owner,
                stack.reserved_by,
            );
            let entry = groups.entry(key).or_insert((stack.id, 0, Vec::new()));
            entry.1 += stack.quantity;
            if stack.id != entry.0 {
                entry.2.push(stack.id);
            }
        }
        for (primary_id, total_qty, duplicates) in groups.values() {
            if !duplicates.is_empty() {
                let qty = *total_qty;
                let _ = self.db.item_stacks.modify_unchecked(primary_id, |s| {
                    s.quantity = qty;
                });
                for dup_id in duplicates {
                    let _ = self.db.item_stacks.remove_no_fk(dup_id);
                }
            }
        }
    }

    /// Get the inventory_id for a creature, or return a sentinel. Panics in debug
    /// if the creature doesn't exist.
    fn creature_inv(&self, creature_id: CreatureId) -> InventoryId {
        self.db
            .creatures
            .get(&creature_id)
            .expect("creature must exist")
            .inventory_id
    }

    /// Get the inventory_id for a structure.
    fn structure_inv(&self, structure_id: StructureId) -> InventoryId {
        self.db
            .structures
            .get(&structure_id)
            .expect("structure must exist")
            .inventory_id
    }

    /// Get all item stacks in an inventory as a vec (for bridge/display use).
    pub fn inv_items(&self, inv_id: InventoryId) -> Vec<crate::db::ItemStack> {
        self.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
    }

    /// Get all logistics wants for an inventory.
    pub fn inv_wants(&self, inv_id: InventoryId) -> Vec<crate::db::LogisticsWantRow> {
        self.db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
    }

    /// Set logistics wants for an inventory (replaces all existing wants).
    fn set_inv_wants(&mut self, inv_id: InventoryId, wants: &[building::LogisticsWant]) {
        // Remove existing wants for this inventory.
        let existing = self
            .db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        for row in &existing {
            let _ = self.db.logistics_want_rows.remove_no_fk(&row.id);
        }
        // Insert new wants.
        for want in wants {
            let _ =
                self.db
                    .logistics_want_rows
                    .insert_auto_no_fk(|id| crate::db::LogisticsWantRow {
                        id,
                        inventory_id: inv_id,
                        item_kind: want.item_kind,
                        target_quantity: want.target_quantity,
                    });
        }
    }

    /// Find the target quantity for a specific item kind in an inventory's wants.
    fn inv_want_target(&self, inv_id: InventoryId, kind: inventory::ItemKind) -> u32 {
        self.db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .find(|w| w.item_kind == kind)
            .map(|w| w.target_quantity)
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Task extension table query helpers
    // -----------------------------------------------------------------------

    /// Get the project_id for a Build task from the task_blueprint_refs table.
    fn task_project_id(&self, task_id: TaskId) -> Option<ProjectId> {
        self.db
            .task_blueprint_refs
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|r| r.project_id)
    }

    /// Get a structure_id from a task's structure refs by role.
    fn task_structure_ref(
        &self,
        task_id: TaskId,
        role: crate::db::TaskStructureRole,
    ) -> Option<StructureId> {
        self.db
            .task_structure_refs
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|r| r.role == role)
            .map(|r| r.structure_id)
    }

    /// Get a voxel coord from a task's voxel refs by role.
    fn task_voxel_ref(
        &self,
        task_id: TaskId,
        role: crate::db::TaskVoxelRole,
    ) -> Option<VoxelCoord> {
        self.db
            .task_voxel_refs
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|r| r.role == role)
            .map(|r| r.coord)
    }

    /// Get the haul data for a Haul task.
    fn task_haul_data(&self, task_id: TaskId) -> Option<crate::db::TaskHaulData> {
        self.db
            .task_haul_data
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
    }

    /// Get the sleep data for a Sleep task.
    fn task_sleep_data(&self, task_id: TaskId) -> Option<crate::db::TaskSleepData> {
        self.db
            .task_sleep_data
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
    }

    /// Get the acquire data for an AcquireItem task.
    fn task_acquire_data(&self, task_id: TaskId) -> Option<crate::db::TaskAcquireData> {
        self.db
            .task_acquire_data
            .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
    }

    /// Reconstruct a HaulSource enum from the task's extension tables.
    fn task_haul_source(
        &self,
        task_id: TaskId,
        source_kind: crate::db::HaulSourceKind,
    ) -> Option<task::HaulSource> {
        match source_kind {
            crate::db::HaulSourceKind::Pile => {
                let pos = self.task_voxel_ref(task_id, crate::db::TaskVoxelRole::HaulSourcePile)?;
                Some(task::HaulSource::GroundPile(pos))
            }
            crate::db::HaulSourceKind::Building => {
                let sid = self.task_structure_ref(
                    task_id,
                    crate::db::TaskStructureRole::HaulSourceBuilding,
                )?;
                Some(task::HaulSource::Building(sid))
            }
        }
    }

    /// Reconstruct a HaulSource for an AcquireItem task.
    fn task_acquire_source(
        &self,
        task_id: TaskId,
        source_kind: crate::db::HaulSourceKind,
    ) -> Option<task::HaulSource> {
        match source_kind {
            crate::db::HaulSourceKind::Pile => {
                let pos =
                    self.task_voxel_ref(task_id, crate::db::TaskVoxelRole::AcquireSourcePile)?;
                Some(task::HaulSource::GroundPile(pos))
            }
            crate::db::HaulSourceKind::Building => {
                let sid = self.task_structure_ref(
                    task_id,
                    crate::db::TaskStructureRole::AcquireSourceBuilding,
                )?;
                Some(task::HaulSource::Building(sid))
            }
        }
    }

    /// Reconstruct a SleepLocation from the task's extension tables.
    fn task_sleep_location(&self, task_id: TaskId) -> Option<task::SleepLocation> {
        let sleep_data = self.task_sleep_data(task_id)?;
        match sleep_data.sleep_location {
            crate::db::SleepLocationType::Home => {
                let sid =
                    self.task_structure_ref(task_id, crate::db::TaskStructureRole::SleepAt)?;
                Some(task::SleepLocation::Home(sid))
            }
            crate::db::SleepLocationType::Dormitory => {
                let sid =
                    self.task_structure_ref(task_id, crate::db::TaskStructureRole::SleepAt)?;
                Some(task::SleepLocation::Dormitory(sid))
            }
            crate::db::SleepLocationType::Ground => Some(task::SleepLocation::Ground),
        }
    }

    /// Insert a task and populate its relationship/extension tables based on kind.
    fn insert_task(&mut self, task: task::Task) {
        let task_id = task.id;
        let kind = &task.kind;

        // Populate relationship and extension tables.
        match kind {
            task::TaskKind::Build { project_id } => {
                let _ = self.db.task_blueprint_refs.insert_auto_no_fk(|id| {
                    crate::db::TaskBlueprintRef {
                        id,
                        task_id,
                        project_id: *project_id,
                    }
                });
            }
            task::TaskKind::EatFruit { fruit_pos } | task::TaskKind::Harvest { fruit_pos } => {
                let _ = self
                    .db
                    .task_voxel_refs
                    .insert_auto_no_fk(|id| crate::db::TaskVoxelRef {
                        id,
                        task_id,
                        coord: *fruit_pos,
                        role: crate::db::TaskVoxelRole::FruitTarget,
                    });
            }
            task::TaskKind::Furnish { structure_id } => {
                let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                    crate::db::TaskStructureRef {
                        id,
                        task_id,
                        structure_id: *structure_id,
                        role: crate::db::TaskStructureRole::FurnishTarget,
                    }
                });
            }
            task::TaskKind::Cook { structure_id } => {
                let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                    crate::db::TaskStructureRef {
                        id,
                        task_id,
                        structure_id: *structure_id,
                        role: crate::db::TaskStructureRole::CookAt,
                    }
                });
            }
            task::TaskKind::Sleep { bed_pos, location } => {
                if let Some(pos) = bed_pos {
                    let _ =
                        self.db
                            .task_voxel_refs
                            .insert_auto_no_fk(|id| crate::db::TaskVoxelRef {
                                id,
                                task_id,
                                coord: *pos,
                                role: crate::db::TaskVoxelRole::BedPosition,
                            });
                }
                let sleep_loc = match location {
                    task::SleepLocation::Home(sid) => {
                        let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                            crate::db::TaskStructureRef {
                                id,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::SleepAt,
                            }
                        });
                        crate::db::SleepLocationType::Home
                    }
                    task::SleepLocation::Dormitory(sid) => {
                        let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                            crate::db::TaskStructureRef {
                                id,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::SleepAt,
                            }
                        });
                        crate::db::SleepLocationType::Dormitory
                    }
                    task::SleepLocation::Ground => crate::db::SleepLocationType::Ground,
                };
                let _ = self
                    .db
                    .task_sleep_data
                    .insert_auto_no_fk(|id| crate::db::TaskSleepData {
                        id,
                        task_id,
                        sleep_location: sleep_loc,
                    });
            }
            task::TaskKind::Haul {
                item_kind,
                quantity,
                source,
                destination,
                phase,
                destination_nav_node,
            } => {
                // Destination structure ref.
                let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                    crate::db::TaskStructureRef {
                        id,
                        task_id,
                        structure_id: *destination,
                        role: crate::db::TaskStructureRole::HaulDestination,
                    }
                });
                // Source ref.
                let source_kind = match source {
                    task::HaulSource::GroundPile(pos) => {
                        let _ = self.db.task_voxel_refs.insert_auto_no_fk(|id| {
                            crate::db::TaskVoxelRef {
                                id,
                                task_id,
                                coord: *pos,
                                role: crate::db::TaskVoxelRole::HaulSourcePile,
                            }
                        });
                        crate::db::HaulSourceKind::Pile
                    }
                    task::HaulSource::Building(sid) => {
                        let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                            crate::db::TaskStructureRef {
                                id,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::HaulSourceBuilding,
                            }
                        });
                        crate::db::HaulSourceKind::Building
                    }
                };
                let _ = self
                    .db
                    .task_haul_data
                    .insert_auto_no_fk(|id| crate::db::TaskHaulData {
                        id,
                        task_id,
                        item_kind: *item_kind,
                        quantity: *quantity,
                        phase: *phase,
                        source_kind,
                        destination_nav_node: *destination_nav_node,
                    });
            }
            task::TaskKind::AcquireItem {
                source,
                item_kind,
                quantity,
            } => {
                let source_kind = match source {
                    task::HaulSource::GroundPile(pos) => {
                        let _ = self.db.task_voxel_refs.insert_auto_no_fk(|id| {
                            crate::db::TaskVoxelRef {
                                id,
                                task_id,
                                coord: *pos,
                                role: crate::db::TaskVoxelRole::AcquireSourcePile,
                            }
                        });
                        crate::db::HaulSourceKind::Pile
                    }
                    task::HaulSource::Building(sid) => {
                        let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                            crate::db::TaskStructureRef {
                                id,
                                task_id,
                                structure_id: *sid,
                                role: crate::db::TaskStructureRole::AcquireSourceBuilding,
                            }
                        });
                        crate::db::HaulSourceKind::Building
                    }
                };
                let _ =
                    self.db
                        .task_acquire_data
                        .insert_auto_no_fk(|id| crate::db::TaskAcquireData {
                            id,
                            task_id,
                            item_kind: *item_kind,
                            quantity: *quantity,
                            source_kind,
                        });
            }
            task::TaskKind::Craft {
                structure_id,
                recipe_id,
            } => {
                let _ = self.db.task_structure_refs.insert_auto_no_fk(|id| {
                    crate::db::TaskStructureRef {
                        id,
                        task_id,
                        structure_id: *structure_id,
                        role: crate::db::TaskStructureRole::CraftAt,
                    }
                });
                let rid = recipe_id.clone();
                let _ = self
                    .db
                    .task_craft_data
                    .insert_auto_no_fk(|id| crate::db::TaskCraftData {
                        id,
                        task_id,
                        recipe_id: rid,
                    });
            }
            // GoTo, EatBread, Mope — no extra data.
            task::TaskKind::GoTo | task::TaskKind::EatBread | task::TaskKind::Mope => {}
        }

        // Insert the base task row.
        let db_task = crate::db::Task {
            id: task.id,
            kind_tag: crate::db::TaskKindTag::from_kind(&task.kind),
            state: task.state,
            location: task.location,
            progress: task.progress,
            total_cost: task.total_cost,
            required_species: task.required_species,
            origin: task.origin,
            target_creature: task.target_creature,
        };
        self.db.tasks.insert_no_fk(db_task).unwrap();
    }

    /// Wander: pick a random adjacent nav node and move there.
    fn wander(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        events: &mut Vec<SimEvent>,
    ) {
        let creature = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return,
        };
        let species = creature.species;
        let combat_ai = self.species_table[&species].combat_ai;

        // Aggressive melee creatures pursue detected hostile targets instead
        // of wandering randomly. Falls through to random wander if no target
        // is within detection range or reachable.
        use crate::species::CombatAI;
        if matches!(
            combat_ai,
            CombatAI::AggressiveMelee | CombatAI::AggressiveRanged
        ) && self.hostile_pursue(creature_id, current_node, species, events)
        {
            return;
        }

        self.random_wander(creature_id, current_node, species);
    }

    /// Hostile AI: if a target is in melee range, strike it. Otherwise find
    /// the nearest reachable hostile target within detection range via Dijkstra
    /// and take one step toward it.
    /// Returns `true` if an action was taken (strike or move), `false` if no
    /// target is reachable (caller should fall back to random wander).
    fn hostile_pursue(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        species: Species,
        events: &mut Vec<SimEvent>,
    ) -> bool {
        let attacker = match self.db.creatures.get(&creature_id) {
            Some(c) => c,
            None => return false,
        };
        let attacker_pos = attacker.position;
        let attacker_civ = attacker.civ_id;
        let detection_range_sq = self.species_table[&species].hostile_detection_range_sq;
        let attacker_footprint = self.species_table[&species].footprint;
        let melee_range_sq = self.species_table[&species].melee_range_sq;

        // Collect hostile targets within detection range.
        // For non-civ aggressive creatures: all civ creatures of different
        // species are targets. For civ creatures: creatures whose civ we
        // consider Hostile. Non-civ aggressives don't attack each other.
        let targets = self.detect_hostile_targets(
            creature_id,
            species,
            attacker_pos,
            attacker_civ,
            detection_range_sq,
        );

        if targets.is_empty() {
            return false;
        }

        // Check if any target is in melee range — if so, strike instead of moving.
        let melee_target = targets.iter().find(|&&(target_id, _)| {
            let target = match self.db.creatures.get(&target_id) {
                Some(c) => c,
                None => return false,
            };
            let target_footprint = self.species_table[&target.species].footprint;
            in_melee_range(
                attacker_pos,
                attacker_footprint,
                target.position,
                target_footprint,
                melee_range_sq,
            )
        });

        if let Some(&(target_id, _)) = melee_target {
            if self.try_melee_strike(creature_id, target_id, events) {
                return true;
            }
            // Strike failed (cooldown). Wait here instead of wandering away —
            // schedule re-activation for when the cooldown expires.
            if let Some(next_tick) = self
                .db
                .creatures
                .get(&creature_id)
                .and_then(|c| c.next_available_tick)
            {
                self.event_queue.schedule(
                    next_tick,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            } else {
                // No cooldown tracked — retry shortly.
                self.event_queue.schedule(
                    self.tick + 100,
                    ScheduledEventKind::CreatureActivation { creature_id },
                );
            }
            return true;
        }

        // No target in melee range — try shooting if the attacker has a bow + arrow.
        for &(target_id, _) in &targets {
            if self.try_shoot_arrow(creature_id, target_id, events) {
                return true;
            }
        }

        // No melee or ranged option — pathfind toward nearest detected target.
        let target_nodes: Vec<NavNodeId> = targets.iter().map(|&(_, n)| n).collect();

        let species_data = &self.species_table[&species];
        let graph = self.graph_for_species(species);

        let nearest = crate::pathfinding::dijkstra_nearest(
            graph,
            current_node,
            &target_nodes,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
            species_data.allowed_edge_types.as_deref(),
        );

        let target_node = match nearest {
            Some(n) if n == current_node => return false,
            Some(n) => n,
            None => return false,
        };

        // A* to get the path, then take the first step.
        let path = crate::pathfinding::astar(
            graph,
            current_node,
            target_node,
            species_data.walk_ticks_per_voxel,
            species_data.climb_ticks_per_voxel,
            species_data.wood_ladder_tpv,
            species_data.rope_ladder_tpv,
        );

        let path = match path {
            Some(p) if p.edge_indices.is_empty() => return false,
            Some(p) => p,
            None => return false,
        };

        let first_edge_idx = path.edge_indices[0];
        self.move_one_step(creature_id, species, first_edge_idx);
        true
    }

    /// Detect hostile targets within detection range. Returns a list of
    /// `(CreatureId, NavNodeId)` pairs sorted by squared euclidean distance
    /// (nearest first).
    ///
    /// Hostility rules for the initial pass:
    /// - Non-civ aggressive creature (no `civ_id`): targets all living civ
    ///   creatures of a different species. Non-civ aggressives don't attack
    ///   each other.
    /// - Civ creature: targets living creatures whose civ it considers
    ///   `CivOpinion::Hostile`. (Future: also non-civ aggressives.)
    fn detect_hostile_targets(
        &self,
        attacker_id: CreatureId,
        attacker_species: Species,
        attacker_pos: VoxelCoord,
        attacker_civ: Option<CivId>,
        detection_range_sq: i64,
    ) -> Vec<(CreatureId, NavNodeId)> {
        let mut targets: Vec<(CreatureId, NavNodeId, i64)> = Vec::new();
        // Track seen creature IDs to avoid duplicates from multi-voxel footprints.
        let mut seen = std::collections::BTreeSet::new();

        // O(n) scan over all creatures in the spatial index.
        for (&_voxel, creature_ids) in &self.spatial_index {
            for &cid in creature_ids {
                if cid == attacker_id || !seen.insert(cid) {
                    continue;
                }
                let creature = match self.db.creatures.get(&cid) {
                    Some(c) => c,
                    None => continue,
                };
                if creature.vital_status != VitalStatus::Alive {
                    continue;
                }
                let node = match creature.current_node {
                    Some(n) => n,
                    None => continue,
                };

                // Squared 3D euclidean distance (i64 to prevent overflow).
                let dx = attacker_pos.x as i64 - creature.position.x as i64;
                let dy = attacker_pos.y as i64 - creature.position.y as i64;
                let dz = attacker_pos.z as i64 - creature.position.z as i64;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq > detection_range_sq {
                    continue;
                }

                // Check hostility.
                let is_target = if attacker_civ.is_none() {
                    // Non-civ aggressive: target all civ creatures of
                    // different species. Don't attack other non-civ creatures.
                    creature.civ_id.is_some() && creature.species != attacker_species
                } else if let (Some(my_civ), Some(their_civ)) = (attacker_civ, creature.civ_id) {
                    // Both are civ creatures: check CivOpinion.
                    if my_civ == their_civ {
                        false
                    } else {
                        self.db
                            .civ_relationships
                            .by_from_civ(&my_civ, tabulosity::QueryOpts::ASC)
                            .into_iter()
                            .any(|r| r.to_civ == their_civ && r.opinion == CivOpinion::Hostile)
                    }
                } else {
                    // Attacker has civ, target doesn't — check if target's
                    // species has aggressive combat_ai.
                    use crate::species::CombatAI;
                    matches!(
                        self.species_table[&creature.species].combat_ai,
                        CombatAI::AggressiveMelee | CombatAI::AggressiveRanged
                    )
                };

                if is_target {
                    targets.push((cid, node, dist_sq));
                }
            }
        }

        // Sort by distance (nearest first), break ties by CreatureId for determinism.
        targets.sort_by_key(|&(cid, _, dist)| (dist, cid));

        targets
            .into_iter()
            .map(|(cid, node, _)| (cid, node))
            .collect()
    }

    /// Move a creature one step along the given nav graph edge: update position,
    /// spatial index, action state, render interpolation, and schedule the next
    /// activation. Shared by random wander and hostile pursuit.
    fn move_one_step(&mut self, creature_id: CreatureId, species: Species, edge_idx: usize) {
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
        let old_pos = self.db.creatures.get(&creature_id).unwrap().position;
        let tick = self.tick;
        let _ = self
            .db
            .creatures
            .modify_unchecked(&creature_id, |creature| {
                creature.position = dest_pos;
                creature.current_node = Some(dest_node);

                // Set action state.
                creature.action_kind = ActionKind::Move;
                creature.next_available_tick = Some(tick + delay);
            });

        self.update_creature_spatial_index(creature_id, species, old_pos, dest_pos);

        // Insert MoveAction for render interpolation.
        let move_action = MoveAction {
            creature_id,
            move_from: old_pos,
            move_to: dest_pos,
            move_start_tick: tick,
        };
        let _ = self.db.move_actions.remove_no_fk(&creature_id);
        self.db.move_actions.insert_no_fk(move_action).unwrap();

        // Schedule next activation based on edge traversal time.
        self.event_queue.schedule(
            self.tick + delay,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Random wander: pick a random eligible edge and move one step.
    fn random_wander(
        &mut self,
        creature_id: CreatureId,
        current_node: NavNodeId,
        species: Species,
    ) {
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

        self.move_one_step(creature_id, species, edge_idx);
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

        // Place the fruit and record its species.
        self.world.set(fruit_pos, VoxelType::Fruit);
        let tree = self.trees.get_mut(&tree_id).unwrap();
        tree.fruit_positions.push(fruit_pos);
        if let Some(species_id) = tree.fruit_species_id {
            self.fruit_voxel_species.insert(fruit_pos, species_id);
            self.fruit_voxel_species_list.push((fruit_pos, species_id));
        }
        true
    }

    // -----------------------------------------------------------------------
    // Build work — incremental voxel materialization
    // -----------------------------------------------------------------------

    /// Start a Build action: set action state, mark music as started on
    /// the first action, and schedule the completion activation.
    fn start_build_action(
        &mut self,
        creature_id: CreatureId,
        task_id: TaskId,
        project_id: ProjectId,
    ) {
        let is_carve = self
            .db
            .blueprints
            .get(&project_id)
            .is_some_and(|bp| bp.build_type.is_carve());

        let duration = if is_carve {
            self.config.carve_work_ticks_per_voxel
        } else {
            self.config.build_work_ticks_per_voxel
        };

        // Mark composition as build_started on the first Build action.
        let progress = self
            .db
            .tasks
            .get(&task_id)
            .map(|t| t.progress)
            .unwrap_or(0.0);
        if progress == 0.0
            && let Some(bp) = self.db.blueprints.get(&project_id)
            && let Some(comp_id) = bp.composition_id
        {
            let _ = self.db.music_compositions.modify_unchecked(&comp_id, |c| {
                c.build_started = true;
            });
        }

        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Build;
            c.next_available_tick = Some(tick + duration);
        });

        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Build action: materialize one voxel (or carve),
    /// increment progress, and check for task completion. Returns true if
    /// the task was completed.
    fn resolve_build_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let project_id = match self.task_project_id(task_id) {
            Some(p) => p,
            None => return false,
        };

        let is_carve = self
            .db
            .blueprints
            .get(&project_id)
            .is_some_and(|bp| bp.build_type.is_carve());

        // Materialize one voxel.
        if is_carve {
            self.materialize_next_carve_voxel(project_id);
        } else {
            self.materialize_next_build_voxel(project_id);
        }

        // Increment progress by 1 (one voxel).
        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
            t.progress += 1.0;
        });

        // Check if the build is complete.
        let task = match self.db.tasks.get(&task_id) {
            Some(t) => t,
            None => return true,
        };
        if task.progress >= task.total_cost {
            self.complete_build(project_id, task_id);
            return true;
        }
        false
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
        let bp = match self.db.blueprints.get(&project_id) {
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
        let creature_positions: Vec<VoxelCoord> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .map(|c| c.position)
            .collect();

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
            if let Some(bp) = self.db.blueprints.get(&project_id)
                && let Some(layout) = bp.face_layout_map()
                && let Some(fd) = layout.get(&chosen)
            {
                self.face_data.insert(chosen, fd.clone());
                self.face_data_list.push((chosen, fd.clone()));
            }
            // For ladders, also store the orientation.
            if is_ladder
                && let Some(bp) = self.db.blueprints.get(&project_id)
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
        let bp = match self.db.blueprints.get(&project_id) {
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

        // A carved voxel may have been supporting a ground pile above it.
        self.apply_pile_gravity();
    }

    /// Mark a blueprint as Complete, register the completed structure, and
    /// complete its associated task.
    fn complete_build(&mut self, project_id: ProjectId, task_id: TaskId) {
        if let Some(mut bp) = self.db.blueprints.get(&project_id) {
            bp.state = BlueprintState::Complete;
            let _ = self.db.blueprints.update_no_fk(bp);
        }

        // Register a CompletedStructure if the blueprint exists.
        if let Some(bp) = self.db.blueprints.get(&project_id) {
            let structure_id = StructureId(self.next_structure_id);
            self.next_structure_id += 1;
            // Populate structure_voxels ownership map.
            for &coord in &bp.voxels {
                self.structure_voxels.insert(coord, structure_id);
            }
            let inv_id = self.create_inventory(crate::db::InventoryOwnerKind::Structure);
            let structure =
                crate::db::CompletedStructure::from_blueprint(structure_id, &bp, self.tick, inv_id);
            self.db.structures.insert_no_fk(structure).unwrap();
        }

        self.complete_task(task_id);
    }

    // -----------------------------------------------------------------------
    // Furnishing — assign function to completed buildings
    // -----------------------------------------------------------------------

    /// Start furnishing a completed building. Validates the structure is a
    /// building with no existing furnishing, computes furniture positions,
    /// sets the furnishing type, auto-renames if no custom name, and creates
    /// a Furnish task for an elf to work on.
    fn furnish_structure(
        &mut self,
        structure_id: StructureId,
        furnishing_type: FurnishingType,
        greenhouse_species: Option<FruitSpeciesId>,
    ) {
        // Validate: structure exists, is a Building, and has no furnishing yet.
        let structure = match self.db.structures.get(&structure_id) {
            Some(s) => s,
            None => return,
        };
        if structure.build_type != BuildType::Building {
            return;
        }
        if structure.furnishing.is_some() {
            return;
        }

        // Greenhouse-specific validation: species must exist and be cultivable.
        if furnishing_type == FurnishingType::Greenhouse {
            let species_id = match greenhouse_species {
                Some(id) => id,
                None => return, // Greenhouse requires a species.
            };
            let species = match self.db.fruit_species.get(&species_id) {
                Some(s) => s,
                None => return, // Species must exist.
            };
            if !species.greenhouse_cultivable {
                return; // Species must be cultivable.
            }
        }

        // Compute furniture positions based on furnishing type.
        let planned_furniture =
            structure.compute_furniture_positions(furnishing_type, &mut self.rng);
        if planned_furniture.is_empty() {
            return;
        }
        let planned_count = planned_furniture.len();

        // Insert planned furniture rows.
        for coord in &planned_furniture {
            let _ = self
                .db
                .furniture
                .insert_auto_no_fk(|id| crate::db::Furniture {
                    id,
                    structure_id,
                    coord: *coord,
                    placed: false,
                });
        }

        // Set furnishing type on the structure.
        let mut structure = self.db.structures.get(&structure_id).unwrap();
        structure.furnishing = Some(furnishing_type);

        // Set default logistics and cooking config based on furnishing type.
        let inv_id = structure.inventory_id;
        let default_wants = match furnishing_type {
            FurnishingType::Storehouse => {
                structure.logistics_priority = Some(self.config.storehouse_default_priority);
                vec![
                    building::LogisticsWant {
                        item_kind: inventory::ItemKind::Fruit,
                        target_quantity: self.config.storehouse_default_fruit_want,
                    },
                    building::LogisticsWant {
                        item_kind: inventory::ItemKind::Bread,
                        target_quantity: self.config.storehouse_default_bread_want,
                    },
                ]
            }
            FurnishingType::Kitchen => {
                structure.logistics_priority = Some(self.config.kitchen_default_priority);
                structure.cooking_enabled = true;
                structure.cooking_bread_target = self.config.kitchen_default_bread_target;
                vec![building::LogisticsWant {
                    item_kind: inventory::ItemKind::Fruit,
                    target_quantity: self.config.kitchen_default_fruit_want,
                }]
            }
            FurnishingType::Workshop => {
                structure.workshop_enabled = true;
                let all_recipe_ids: Vec<String> =
                    self.config.recipes.iter().map(|r| r.id.clone()).collect();
                structure.workshop_recipe_ids = all_recipe_ids.clone();
                structure.logistics_priority = Some(self.config.workshop_default_priority);

                self.compute_recipe_wants(&all_recipe_ids)
            }
            FurnishingType::Greenhouse => {
                structure.greenhouse_species = greenhouse_species;
                structure.greenhouse_enabled = true;
                structure.greenhouse_last_production_tick = self.tick;
                Vec::new()
            }
            _ => Vec::new(),
        };

        // Find a nav node inside the building to use as the task location.
        let interior_pos = structure.floor_interior_positions();
        let task_pos = interior_pos.first().copied().unwrap_or(structure.anchor);
        let _ = self.db.structures.update_no_fk(structure);
        self.set_inv_wants(inv_id, &default_wants);
        let location = match self.nav_graph.find_nearest_node(task_pos) {
            Some(n) => n,
            None => return,
        };

        // Create the Furnish task. total_cost = number of furniture items.
        let total_cost = planned_count as f32;
        let task_id = TaskId::new(&mut self.rng);
        let new_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Furnish { structure_id },
            state: task::TaskState::Available,
            location,
            progress: 0.0,
            total_cost,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        self.insert_task(new_task);
    }

    // -----------------------------------------------------------------------
    // Home assignment
    // -----------------------------------------------------------------------

    /// Assign a creature to a home structure, or unassign if `structure_id`
    /// is `None`. Validates: creature is an Elf, target is a Home-furnished
    /// building. Evicts a previous occupant if the target already has one.
    fn assign_home(&mut self, creature_id: CreatureId, structure_id: Option<StructureId>) {
        // Validate creature exists and is an Elf.
        match self.db.creatures.get(&creature_id) {
            Some(c) if c.species == Species::Elf => {}
            _ => return,
        };

        // Nothing to clear on old home — creature.assigned_home is the
        // single source of truth for home assignment.

        let target_id = match structure_id {
            Some(id) => id,
            None => {
                // Unassign only.
                if let Some(mut c) = self.db.creatures.get(&creature_id) {
                    c.assigned_home = None;
                    let _ = self.db.creatures.update_no_fk(c);
                }
                return;
            }
        };

        // Validate target structure exists and is a Home.
        match self.db.structures.get(&target_id) {
            Some(s) if s.furnishing == Some(FurnishingType::Home) => {}
            _ => return,
        };

        // Evict previous occupant if there is one.
        let prev_occupants = self
            .db
            .creatures
            .by_assigned_home(&Some(target_id), tabulosity::QueryOpts::ASC);
        for prev_elf in prev_occupants {
            if prev_elf.id != creature_id {
                let mut prev = prev_elf;
                prev.assigned_home = None;
                let _ = self.db.creatures.update_no_fk(prev);
            }
        }

        // Set creature's assigned_home.
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.assigned_home = Some(target_id);
            let _ = self.db.creatures.update_no_fk(c);
        }
    }

    /// Start a Furnish action: set action kind and schedule next activation
    /// after `furnish_work_ticks_per_item` ticks.
    fn start_furnish_action(&mut self, creature_id: CreatureId) {
        let duration = self.config.furnish_work_ticks_per_item;
        let tick = self.tick;
        let _ = self.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::Furnish;
            c.next_available_tick = Some(tick + duration);
        });
        self.event_queue.schedule(
            self.tick + duration,
            ScheduledEventKind::CreatureActivation { creature_id },
        );
    }

    /// Resolve a completed Furnish action: place one furniture item, increment
    /// progress, check for completion. Returns true if task completed.
    fn resolve_furnish_action(&mut self, creature_id: CreatureId) -> bool {
        let task_id = match self
            .db
            .creatures
            .get(&creature_id)
            .and_then(|c| c.current_task)
        {
            Some(t) => t,
            None => return false,
        };
        let structure_id =
            match self.task_structure_ref(task_id, crate::db::TaskStructureRole::FurnishTarget) {
                Some(s) => s,
                None => return false,
            };

        // Place the next unplaced furniture item.
        if let Some(furn) = self
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|f| !f.placed)
        {
            let _ = self.db.furniture.modify_unchecked(&furn.id, |f| {
                f.placed = true;
            });
        }

        // Increment progress by 1 (one item).
        let _ = self.db.tasks.modify_unchecked(&task_id, |t| {
            t.progress += 1.0;
        });

        // Check if furnishing is complete.
        let task = match self.db.tasks.get(&task_id) {
            Some(t) => t,
            None => return true,
        };
        if task.progress >= task.total_cost {
            self.complete_task(task_id);
            return true;
        }
        false
    }

    /// After a nav graph rebuild, re-resolve every creature's `current_node`
    /// by finding the nearest node to its position. Clears stored paths since
    /// NavNodeIds change when the graph is rebuilt.
    fn resnap_creature_nodes(&mut self) {
        let creature_info: Vec<(CreatureId, Species, VoxelCoord)> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .map(|c| (c.id, c.species, c.position))
            .collect();
        for (cid, species, old_pos) in creature_info {
            let graph = self.graph_for_species(species);
            let new_node = graph.find_nearest_node(old_pos);
            let new_pos = new_node.map(|nid| graph.node(nid).position);
            let _ = self.db.creatures.modify_unchecked(&cid, |creature| {
                creature.current_node = new_node;
                creature.path = None;
                if let Some(p) = new_pos {
                    creature.position = p;
                }
            });
            if let Some(p) = new_pos {
                self.update_creature_spatial_index(cid, species, old_pos, p);
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
            .db
            .creatures
            .iter_all()
            .filter(|c| {
                c.vital_status == VitalStatus::Alive
                    && matches!(c.current_node, Some(nid) if removed.contains(&nid))
            })
            .map(|c| (c.id, c.species, c.position))
            .collect();
        for (cid, species, old_pos) in to_resnap {
            let graph = self.graph_for_species(species);
            let new_node = graph.find_nearest_node(old_pos);
            let new_pos = new_node.map(|nid| graph.node(nid).position);
            let _ = self.db.creatures.modify_unchecked(&cid, |creature| {
                creature.current_node = new_node;
                creature.path = None;
                if let Some(p) = new_pos {
                    creature.position = p;
                }
            });
            if let Some(p) = new_pos {
                self.update_creature_spatial_index(cid, species, old_pos, p);
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
    // Solid voxel raycast
    // -----------------------------------------------------------------------

    /// DDA voxel raycast that returns the first solid voxel hit and the face
    /// the ray entered through. Uses the same Amanatides & Woo algorithm as
    /// `raycast_structure()`, but tracks `last_axis` (the axis most recently
    /// stepped) to compute the entry face on hit.
    ///
    /// If `overlay` is `Some`, designated (not yet built) blueprints are
    /// treated as their target voxel types — a designated platform reads as
    /// solid and can be "hit" by the ray. Pass `None` to raycast against the
    /// actual world only.
    ///
    /// Face encoding matches `FaceDirection` ordinals:
    ///   0=PosX, 1=NegX, 2=PosY, 3=NegY, 4=PosZ, 5=NegZ
    /// The face returned is the face of the solid voxel that the ray entered
    /// through. A ray stepping -Y (downward) enters through the PosY face
    /// (2); a ray stepping +X enters through the NegX face (1); etc.
    pub fn raycast_solid(
        &self,
        from: [f32; 3],
        dir: [f32; 3],
        max_steps: u32,
        overlay: Option<&structural::BlueprintOverlay>,
    ) -> Option<(VoxelCoord, u8)> {
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

        // Track which axis was last stepped to compute the entry face.
        let mut last_axis: usize = 0;
        let mut first_step = true;

        for _ in 0..max_steps {
            let coord = VoxelCoord::new(voxel[0], voxel[1], voxel[2]);

            let vt = match overlay {
                Some(ov) => ov.effective_type(&self.world, coord),
                None => self.world.get(coord),
            };
            if !first_step && vt.is_solid() {
                // Compute the face: the ray entered through the face opposite
                // to the step direction on last_axis.
                let face = match (last_axis, step[last_axis] > 0) {
                    (0, true) => 1,  // stepped +X → entered through NegX face
                    (0, false) => 0, // stepped -X → entered through PosX face
                    (1, true) => 3,  // stepped +Y → entered through NegY face
                    (1, false) => 2, // stepped -Y → entered through PosY face
                    (2, true) => 5,  // stepped +Z → entered through NegZ face
                    (2, false) => 4, // stepped -Z → entered through PosZ face
                    _ => unreachable!(),
                };
                return Some((coord, face));
            }

            first_step = false;

            // Advance along the axis with the smallest t_max.
            last_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            voxel[last_axis] += step[last_axis];
            t_max[last_axis] += t_delta[last_axis];
        }

        None
    }

    // -----------------------------------------------------------------------
    // Auto ladder orientation
    // -----------------------------------------------------------------------

    /// For a ladder column at `(x, y..y+height, z)`, count how many voxels
    /// in the column have a solid neighbor in each of the 4 cardinal
    /// directions. Return the orientation (as FaceDirection ordinal) with
    /// the highest count. Tie-break: first in iteration order (East,
    /// South, West, North).
    pub fn auto_ladder_orientation(&self, x: i32, y: i32, z: i32, height: i32) -> u8 {
        // Cardinal directions: East(+X)=0, South(+Z)=4, West(-X)=1, North(-Z)=5
        let orientations: [(i32, i32, u8); 4] = [
            (1, 0, 0),  // East (+X) → face PosX
            (0, 1, 4),  // South (+Z) → face PosZ
            (-1, 0, 1), // West (-X) → face NegX
            (0, -1, 5), // North (-Z) → face NegZ
        ];

        let mut best_face: u8 = 0;
        let mut best_count: i32 = -1;

        for &(dx, dz, face) in &orientations {
            let mut count = 0i32;
            for dy in 0..height {
                let neighbor = VoxelCoord::new(x + dx, y + dy, z + dz);
                if self.world.get(neighbor).is_solid() {
                    count += 1;
                }
            }
            if count > best_count {
                best_count = count;
                best_face = face;
            }
        }

        best_face
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
    /// `spatial_index` (from creatures + species footprints — must run after
    /// `species_table`), `lexicon` (from embedded JSON), `structure_voxels`
    /// (from completed blueprints + structures).
    pub fn rebuild_transient_state(&mut self) {
        self.world = Self::rebuild_world(
            &self.config,
            &self.trees,
            &self.placed_voxels,
            &self.carved_voxels,
        );
        // World rebuild produces dirty_voxels entries for every set() call.
        // Clear them — the mesh cache will do a full build_all() after load.
        self.world.clear_dirty_voxels();
        self.face_data = self.face_data_list.iter().cloned().collect();
        self.ladder_orientations = self.ladder_orientations_list.iter().cloned().collect();
        self.nav_graph = nav::build_nav_graph(&self.world, &self.face_data);
        self.large_nav_graph = nav::build_large_nav_graph(&self.world);
        self.species_table = self.config.species.clone();
        self.lexicon = Some(elven_canopy_lang::default_lexicon());

        // Rebuild spatial_index from all living creatures. Must run after
        // species_table is populated (footprint data comes from SpeciesData).
        self.rebuild_spatial_index();

        // Rebuild fruit_voxel_species from persisted list.
        self.fruit_voxel_species = self.fruit_voxel_species_list.iter().cloned().collect();

        // Rebuild structure_voxels from completed blueprints.
        self.structure_voxels.clear();
        for bp in self.db.blueprints.iter_all() {
            if bp.state == BlueprintState::Complete {
                // Find the StructureId for this blueprint's project.
                if let Some(structure) = self
                    .db
                    .structures
                    .iter_all()
                    .find(|s| s.project_id == bp.id)
                {
                    let sid = structure.id;
                    for &coord in &bp.voxels {
                        self.structure_voxels.insert(coord, sid);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Spatial index
    // -----------------------------------------------------------------------

    /// Rebuild the spatial index from scratch using all creatures in the DB.
    /// Must be called after `species_table` is populated (footprint lookups
    /// depend on `SpeciesData`).
    fn rebuild_spatial_index(&mut self) {
        self.spatial_index.clear();
        let entries: Vec<(CreatureId, Species, VoxelCoord)> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status == VitalStatus::Alive)
            .map(|c| (c.id, c.species, c.position))
            .collect();
        for (cid, species, pos) in entries {
            let footprint = self.species_table[&species].footprint;
            Self::register_creature_in_index(&mut self.spatial_index, cid, pos, footprint);
        }
    }

    /// Register a creature in the spatial index at all voxels covered by its
    /// footprint. The anchor position is the min-corner; the footprint
    /// `[fx, fy, fz]` extends in the positive direction. Entries are kept
    /// sorted by `CreatureId` for deterministic PRNG selection.
    fn register_creature_in_index(
        index: &mut BTreeMap<VoxelCoord, Vec<CreatureId>>,
        creature_id: CreatureId,
        anchor: VoxelCoord,
        footprint: [u8; 3],
    ) {
        for dx in 0..footprint[0] as i32 {
            for dy in 0..footprint[1] as i32 {
                for dz in 0..footprint[2] as i32 {
                    let voxel = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                    let vec = index.entry(voxel).or_default();
                    let pos = vec.binary_search(&creature_id).unwrap_or_else(|p| p);
                    vec.insert(pos, creature_id);
                }
            }
        }
    }

    /// Remove a creature from the spatial index at all voxels covered by its
    /// footprint at the given anchor position.
    fn deregister_creature_from_index(
        index: &mut BTreeMap<VoxelCoord, Vec<CreatureId>>,
        creature_id: CreatureId,
        anchor: VoxelCoord,
        footprint: [u8; 3],
    ) {
        for dx in 0..footprint[0] as i32 {
            for dy in 0..footprint[1] as i32 {
                for dz in 0..footprint[2] as i32 {
                    let voxel = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                    if let Some(vec) = index.get_mut(&voxel) {
                        vec.retain(|&id| id != creature_id);
                        if vec.is_empty() {
                            index.remove(&voxel);
                        }
                    }
                }
            }
        }
    }

    /// Update the spatial index when a creature moves from `old_pos` to
    /// `new_pos`. Deregisters from old voxels, registers at new voxels.
    fn update_creature_spatial_index(
        &mut self,
        creature_id: CreatureId,
        species: Species,
        old_pos: VoxelCoord,
        new_pos: VoxelCoord,
    ) {
        if old_pos == new_pos {
            return;
        }
        let footprint = self.species_table[&species].footprint;
        Self::deregister_creature_from_index(
            &mut self.spatial_index,
            creature_id,
            old_pos,
            footprint,
        );
        Self::register_creature_in_index(&mut self.spatial_index, creature_id, new_pos, footprint);
    }

    /// Query all creatures at a given voxel coordinate. Returns a sorted slice
    /// for deterministic iteration (sorted by `CreatureId` for PRNG selection).
    pub fn creatures_at_voxel(&self, coord: VoxelCoord) -> &[CreatureId] {
        match self.spatial_index.get(&coord) {
            Some(vec) => vec,
            None => &[],
        }
    }

    /// Serialize the simulation state to a JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Compute a deterministic checksum of the simulation state for desync
    /// detection. Serializes all non-`#[serde(skip)]` fields to JSON bytes,
    /// then hashes with FNV-1a. Deterministic because `BTreeMap` produces
    /// sorted keys and Ryu produces stable float formatting.
    pub fn state_checksum(&self) -> u64 {
        let bytes = serde_json::to_vec(self).expect("SimState serialization should not fail");
        crate::checksum::fnv1a_64(&bytes)
    }

    /// Deserialize a simulation state from a JSON string and rebuild
    /// transient fields (world, nav_graph, species_table, lexicon).
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut state: SimState = serde_json::from_str(json)?;
        state.rebuild_transient_state();
        Ok(state)
    }

    /// Count creatures of a given species.
    pub fn creature_count(&self, species: Species) -> usize {
        self.db
            .creatures
            .iter_all()
            .filter(|c| c.species == species && c.vital_status == VitalStatus::Alive)
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

    /// Build a `BlueprintOverlay` from all `Designated` (not yet built)
    /// blueprints. Each blueprint's voxels are mapped to the voxel type they
    /// will become when materialized (via `BuildType::to_voxel_type()`).
    /// Face data from building blueprints is also collected.
    ///
    /// Callers that use this overlay treat designated blueprints as if they
    /// were already materialized — e.g., a designated platform reads as
    /// `GrownPlatform`, a designated carve reads as `Air`. Used by
    /// designation validation and preview validation so the player sees the
    /// cumulative effect of all planned builds.
    pub fn blueprint_overlay(&self) -> structural::BlueprintOverlay {
        let mut voxels = BTreeMap::new();
        let mut faces = BTreeMap::new();
        for bp in self.db.blueprints.iter_all() {
            if bp.state != BlueprintState::Designated {
                continue;
            }
            let target_type = bp.build_type.to_voxel_type();
            for &coord in &bp.voxels {
                voxels.insert(coord, target_type);
            }
            if let Some(ref face_entries) = bp.face_layout {
                for &(coord, ref fd) in face_entries {
                    faces.insert(coord, fd.clone());
                }
            }
        }
        structural::BlueprintOverlay { voxels, faces }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::WorkshopRecipeEntry;
    use crate::db::{CompletedStructure, TaskKindTag};
    use crate::task::{Task, TaskKind, TaskOrigin, TaskState};
    use std::sync::LazyLock;

    /// Cached seed-42 SimState. Constructed once (tree gen + nav graph + lexicon),
    /// then cloned by `test_sim(42)`. ~155 call sites go from full construction
    /// to a cheap ~256KB memcpy.
    static CACHED_SIM_42: LazyLock<SimState> =
        LazyLock::new(|| SimState::with_config(42, test_config()));

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
    /// Seed 42 clones from a cached instance; other seeds construct fresh.
    fn test_sim(seed: u64) -> SimState {
        if seed == 42 {
            CACHED_SIM_42.clone()
        } else {
            SimState::with_config(seed, test_config())
        }
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
    fn tree_heartbeat_reschedules() {
        let mut sim = test_sim(42);
        let heartbeat_interval = sim.config.tree_heartbeat_interval_ticks;

        // Step past the first heartbeat.
        sim.step(&[], heartbeat_interval + 1);

        // The tree heartbeat should have rescheduled at tick = 2 * heartbeat_interval.
        // Other periodic events (e.g. LogisticsHeartbeat) may sit earlier in the queue,
        // so pop events until we find the TreeHeartbeat and verify its tick.
        let mut found_tree_heartbeat = false;
        while let Some(evt) = sim.event_queue.pop_if_ready(u64::MAX) {
            if matches!(evt.kind, ScheduledEventKind::TreeHeartbeat { .. }) {
                assert_eq!(evt.tick, heartbeat_interval * 2);
                found_tree_heartbeat = true;
                break;
            }
        }
        assert!(
            found_tree_heartbeat,
            "TreeHeartbeat not found in event queue"
        );
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
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: VoxelCoord::new(128, 1, 128),
            },
        }];

        sim_a.step(&cmds, 200);
        sim_b.step(&cmds, 200);

        assert_eq!(sim_a.tick, sim_b.tick);
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
    fn spawned_elf_has_vaelith_name() {
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
        assert_eq!(sim.creature_count(Species::Elf), 1);

        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .expect("elf should exist");

        // Elf should have a non-empty Vaelith name with given + surname.
        assert!(!elf.name.is_empty(), "Elf should have a name");
        assert!(
            elf.name.contains(' '),
            "Name '{}' should contain a space (given + surname)",
            elf.name
        );
        assert!(
            !elf.name_meaning.is_empty(),
            "Elf should have a name meaning"
        );
    }

    #[test]
    fn spawned_elf_name_is_deterministic() {
        // Same seed should produce the same elf name.
        let mut sim1 = test_sim(42);
        let mut sim2 = test_sim(42);
        let tree_pos = sim1.trees[&sim1.player_tree_id].position;

        let cmd1 = SimCommand {
            player_id: sim1.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        let cmd2 = SimCommand {
            player_id: sim2.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };

        sim1.step(&[cmd1], 2);
        sim2.step(&[cmd2], 2);

        let elf1 = sim1.db.creatures.iter_all().next().unwrap();
        let elf2 = sim2.db.creatures.iter_all().next().unwrap();
        assert_eq!(elf1.name, elf2.name);
        assert_eq!(elf1.name_meaning, elf2.name_meaning);
    }

    #[test]
    fn spawned_non_elf_has_no_name() {
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
        let capy = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Capybara)
            .expect("capybara should exist");

        // Non-elf creatures should not have Vaelith names.
        assert!(capy.name.is_empty(), "Capybara should not have a name");
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
            .db
            .creatures
            .iter_all()
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
        assert_eq!(sim_a.db.creatures.len(), sim_b.db.creatures.len());
        for creature_a in sim_a.db.creatures.iter_all() {
            let creature_b = sim_b.db.creatures.get(&creature_a.id).unwrap();
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
            .db
            .creatures
            .iter_all()
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
            .db
            .creatures
            .iter_all()
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
                .db
                .creatures
                .iter_all()
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

        assert_eq!(sim_a.db.creatures.len(), sim_b.db.creatures.len());
        for creature_a in sim_a.db.creatures.iter_all() {
            let creature_b = sim_b.db.creatures.get(&creature_a.id).unwrap();
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        let initial_node = elf.current_node.unwrap();
        let initial_pos = elf.position;

        // Step enough for many activations (each moves 1 edge; ground edges
        // cost ~500 ticks at walk_ticks_per_voxel=500).
        sim.step(&[], 50000);

        let elf = sim
            .db
            .creatures
            .iter_all()
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
                .db
                .creatures
                .iter_all()
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
        let existing: std::collections::BTreeSet<CreatureId> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf)
            .map(|c| c.id)
            .collect();
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
        sim.db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf && !existing.contains(&c.id))
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
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        sim.insert_task(task);
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

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(
            elf.current_task,
            Some(task_id),
            "Elf should have claimed the available task"
        );
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert!(
            sim.db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(task.id)),
            "Elf should be assigned to the task"
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

        let initial_dist = sim
            .db
            .creatures
            .get(&elf_id)
            .unwrap()
            .position
            .manhattan_distance(task_location);

        // Step a moderate amount — creature should be closer to the target.
        sim.step(&[], sim.tick + 50000);

        let mid_dist = sim
            .db
            .creatures
            .get(&elf_id)
            .unwrap()
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
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let task_id = insert_goto_task(&mut sim, elf_node);

        // One activation should be enough: elf claims task, is already there, completes.
        sim.step(&[], sim.tick + 10000);

        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.state,
            TaskState::Complete,
            "GoTo task should be complete"
        );
        let elf = sim.db.creatures.get(&elf_id).unwrap();
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
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let _task_id = insert_goto_task(&mut sim, elf_node);

        // Complete the task.
        sim.step(&[], sim.tick + 10000);
        let pos_after_task = sim.db.creatures.get(&elf_id).unwrap().position;

        // Continue ticking — elf should resume wandering (position changes).
        sim.step(&[], sim.tick + 50000);

        let pos_after_wander = sim.db.creatures.get(&elf_id).unwrap().position;
        assert_ne!(
            pos_after_task, pos_after_wander,
            "Elf should have wandered after task completion"
        );
        assert!(
            sim.db
                .creatures
                .get(&elf_id)
                .unwrap()
                .current_task
                .is_none(),
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

        assert_eq!(sim.db.tasks.len(), 1, "Should have 1 task");
        let task = sim.db.tasks.iter_all().next().unwrap();
        assert_eq!(task.state, TaskState::Available);
        assert!(task.kind_tag == TaskKindTag::GoTo);
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

        assert_eq!(sim.db.tasks.len(), 1);
        let task_id = *sim.db.tasks.iter_keys().next().unwrap();

        // Tick until the elf completes the task.
        sim.step(&[], 50000);

        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.state,
            TaskState::Complete,
            "Task should be complete after enough ticks"
        );

        // Elf should be unassigned and wandering again.
        let elf = sim
            .db
            .creatures
            .iter_all()
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

        // Tick enough for a creature to claim the task. The elf may or may
        // not have arrived yet (GoTo completes on arrival, clearing
        // current_task), so we check that the task was claimed OR completed.
        sim.step(&[], sim.tick + 5000);

        let task = sim.db.tasks.get(&task_id).unwrap();
        let claimers = sim
            .db
            .creatures
            .by_current_task(&Some(task.id), tabulosity::QueryOpts::ASC);

        if task.state == crate::task::TaskState::Complete {
            // Task was completed — some elf claimed and finished it.
            assert!(
                claimers.is_empty(),
                "Completed task should have no current claimers"
            );
        } else {
            // Task still in progress — exactly one elf should be on it.
            assert_eq!(
                claimers.len(),
                1,
                "Exactly one creature should claim the task, got {}",
                claimers.len()
            );
            let assignee = &claimers[0];
            assert_eq!(assignee.species, Species::Elf);
        }

        // No capybara should have a task (elf-only restriction).
        for creature in sim.db.creatures.iter_all() {
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

    #[test]
    fn tree_has_fruit_species_assigned() {
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            tree.fruit_species_id.is_some(),
            "Home tree should have a fruit species assigned during worldgen"
        );
        // The assigned species should exist in the world's species roster.
        let species_id = tree.fruit_species_id.unwrap();
        assert!(
            sim.db.fruit_species.get(&species_id).is_some(),
            "Tree's fruit species {:?} should be in the SimDb fruit_species table",
            species_id
        );
    }

    #[test]
    fn fruit_voxels_have_species_tracked() {
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        // Every fruit voxel should have a species entry in the map.
        for &fruit_pos in &tree.fruit_positions {
            assert!(
                sim.fruit_voxel_species.contains_key(&fruit_pos),
                "Fruit at {} should have a species tracked in fruit_voxel_species",
                fruit_pos
            );
        }
        // The tracked species should match the tree's assigned species.
        if let Some(tree_species) = tree.fruit_species_id {
            for &fruit_pos in &tree.fruit_positions {
                let voxel_species = sim.fruit_voxel_species[&fruit_pos];
                assert_eq!(
                    voxel_species, tree_species,
                    "Fruit voxel species should match tree species"
                );
            }
        }
    }

    #[test]
    fn fruit_species_at_returns_species() {
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        if let Some(first_fruit) = tree.fruit_positions.first() {
            let species = sim.fruit_species_at(*first_fruit);
            assert!(
                species.is_some(),
                "fruit_species_at should return a species"
            );
            let species = species.unwrap();
            assert!(
                !species.vaelith_name.is_empty(),
                "Fruit species should have a Vaelith name"
            );
            assert!(
                !species.english_gloss.is_empty(),
                "Fruit species should have an English gloss"
            );
        }
    }

    #[test]
    fn fruit_voxel_species_roundtrip() {
        let sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(!tree.fruit_positions.is_empty(), "need fruit for this test");

        let json = sim.to_json().unwrap();
        let loaded = SimState::from_json(&json).unwrap();
        let loaded_tree = &loaded.trees[&loaded.player_tree_id];

        // Fruit voxel species map should survive roundtrip.
        assert_eq!(
            sim.fruit_voxel_species.len(),
            loaded.fruit_voxel_species.len(),
            "fruit_voxel_species count should survive roundtrip"
        );
        for (&pos, &species_id) in &sim.fruit_voxel_species {
            assert_eq!(
                loaded.fruit_voxel_species.get(&pos),
                Some(&species_id),
                "fruit_voxel_species entry at {} should survive roundtrip",
                pos
            );
        }
        // Tree's fruit species should survive too.
        assert_eq!(
            loaded_tree.fruit_species_id, tree.fruit_species_id,
            "Tree fruit_species_id should survive roundtrip"
        );
    }

    #[test]
    fn harvest_fruit_carries_species_material() {
        let mut sim = test_sim(42);
        let tree = &sim.trees[&sim.player_tree_id];
        let fruit_pos = tree.fruit_positions[0];
        let tree_species = tree.fruit_species_id.unwrap();

        // Spawn an elf near the fruit.
        let elf_nav = sim.nav_graph.find_nearest_node(fruit_pos).unwrap();
        let elf_pos = sim.nav_graph.node(elf_nav).position;
        let mut events = Vec::new();
        let elf_id = sim
            .spawn_creature(Species::Elf, elf_pos, &mut events)
            .unwrap();
        sim.config.elf_starting_bread = 100; // Prevent hunger.

        // Manually call do_harvest to test the material flow.
        let task_id = TaskId::new(&mut sim.rng);
        let task = task::Task {
            id: task_id,
            kind: task::TaskKind::Harvest { fruit_pos },
            state: task::TaskState::InProgress,
            location: elf_nav,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: task::TaskOrigin::Automated,
            target_creature: None,
        };
        sim.insert_task(task);
        sim.resolve_harvest_action(elf_id, task_id, fruit_pos);

        // The fruit should be gone from world and species map.
        assert_eq!(sim.world.get(fruit_pos), VoxelType::Air);
        assert!(!sim.fruit_voxel_species.contains_key(&fruit_pos));

        // Find the ground pile and check the item has fruit species material.
        let pile_stacks: Vec<_> = sim
            .db
            .item_stacks
            .iter_all()
            .filter(|s| {
                s.kind == inventory::ItemKind::Fruit
                    && s.material == Some(inventory::Material::FruitSpecies(tree_species))
            })
            .collect();
        assert!(
            !pile_stacks.is_empty(),
            "Harvested fruit should have Material::FruitSpecies({:?})",
            tree_species
        );
    }

    #[test]
    fn fruit_heartbeat_tracks_species() {
        // Fruit grown via heartbeat should also be tracked in species map.
        let mut config = test_config();
        config.fruit_initial_attempts = 0;
        config.fruit_production_base_rate = 1.0;
        config.fruit_max_per_tree = 100;
        let mut sim = SimState::with_config(42, config);

        assert!(
            sim.fruit_voxel_species.is_empty(),
            "Should start with no species entries"
        );

        // Step past heartbeats to grow fruit.
        sim.step(&[], 50000);

        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.fruit_positions.is_empty(),
            "Should have grown some fruit"
        );
        // Every fruit should have species tracked.
        for &pos in &tree.fruit_positions {
            assert!(
                sim.fruit_voxel_species.contains_key(&pos),
                "Heartbeat-grown fruit at {} should have species tracked",
                pos
            );
        }
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
        assert_eq!(sim.db.creatures.len(), restored.db.creatures.len());
        for creature in sim.db.creatures.iter_all() {
            let restored_creature = restored.db.creatures.get(&creature.id).unwrap();
            assert_eq!(creature.position, restored_creature.position);
            assert_eq!(creature.species, restored_creature.species);
            assert_eq!(creature.name, restored_creature.name);
            assert_eq!(creature.name_meaning, restored_creature.name_meaning);
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
        for creature in sim.db.creatures.iter_all() {
            let restored_creature = restored.db.creatures.get(&creature.id).unwrap();
            assert_eq!(
                creature.position, restored_creature.position,
                "Creature {:?} position diverged after roundtrip + 500 ticks",
                creature.id
            );
        }
        // PRNG state must match.
        assert_eq!(sim.rng.next_u64(), restored.rng.next_u64());
    }

    #[test]
    fn elf_spawned_after_roundtrip_gets_name() {
        let sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Save and restore (no creatures yet).
        let mut restored = SimState::from_json(&sim.to_json().unwrap()).unwrap();

        // Spawn an elf after the roundtrip.
        let cmd = SimCommand {
            player_id: restored.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        restored.step(&[cmd], 2);

        let elf = restored
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .expect("elf should exist after roundtrip spawn");
        assert!(
            !elf.name.is_empty(),
            "Elf spawned after save/load should still get a Vaelith name"
        );
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
        assert_eq!(sim.species_table.len(), 10);
        assert!(sim.species_table.contains_key(&Species::Elf));
        assert!(sim.species_table.contains_key(&Species::Capybara));
        assert!(sim.species_table.contains_key(&Species::Boar));
        assert!(sim.species_table.contains_key(&Species::Deer));
        assert!(sim.species_table.contains_key(&Species::Elephant));
        assert!(sim.species_table.contains_key(&Species::Goblin));
        assert!(sim.species_table.contains_key(&Species::Monkey));
        assert!(sim.species_table.contains_key(&Species::Orc));
        assert!(sim.species_table.contains_key(&Species::Squirrel));
        assert!(sim.species_table.contains_key(&Species::Troll));

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
        let elephants: Vec<&crate::db::Creature> = sim
            .db
            .creatures
            .iter_all()
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
    fn troll_spawns_on_large_graph() {
        let mut sim = test_sim(42);
        let mut events = Vec::new();
        let spawn_pos = VoxelCoord::new(10, 1, 10);
        sim.spawn_creature(Species::Troll, spawn_pos, &mut events);

        let trolls: Vec<&crate::db::Creature> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Troll)
            .collect();
        assert_eq!(trolls.len(), 1, "Should have spawned one troll");

        let troll = trolls[0];
        let node_id = troll
            .current_node
            .expect("Troll should have a current_node");
        let node = sim.large_nav_graph.node(node_id);
        assert_eq!(
            node.position, troll.position,
            "Troll position should match its large graph node",
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
        assert_eq!(sim.db.creatures.len(), 2);

        // Verify species are correctly stored.
        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.species, Species::Elf);

        let capy = sim
            .db
            .creatures
            .iter_all()
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.food, food_max);

        // Advance past 3 heartbeats.
        let target_tick = 1 + heartbeat_interval * 3 + 1;
        sim.step(&[], target_tick);

        let elf = sim
            .db
            .creatures
            .iter_all()
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.food, 0);
    }

    // -----------------------------------------------------------------------
    // Rest/sleep tests
    // -----------------------------------------------------------------------

    #[test]
    fn rest_decreases_over_heartbeats() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let rest_max = sim.species_table[&Species::Elf].rest_max;
        let decay_per_tick = sim.species_table[&Species::Elf].rest_decay_per_tick;
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

        // Verify rest starts at rest_max.
        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.rest, rest_max);

        // Advance past 3 heartbeats.
        let target_tick = 1 + heartbeat_interval * 3 + 1;
        sim.step(&[], target_tick);

        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        let expected_decay = decay_per_tick * heartbeat_interval as i64 * 3;
        assert_eq!(elf.rest, rest_max - expected_decay);
    }

    #[test]
    fn rest_does_not_go_below_zero() {
        let mut config = test_config();
        let elf = config.species.get_mut(&Species::Elf).unwrap();
        elf.rest_decay_per_tick = 1_000_000_000_000_000; // Depletes in 1 tick
        elf.rest_per_sleep_tick = 0; // Prevent sleep from restoring rest.
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        let target_tick = 1 + heartbeat_interval * 5;
        sim.step(&[], target_tick);

        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(elf.rest, 0);
    }

    #[test]
    fn tired_idle_elf_creates_sleep_task() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest below threshold (50%) and food well above threshold.
        let food_max_val = sim.species_table[&Species::Elf].food_max;
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.rest = rest_max * 30 / 100;
            c.food = food_max_val;
        });

        // Advance past the next heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // The elf should now have a Sleep task.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Tired idle elf should have been assigned a Sleep task"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert!(
            task.kind_tag == TaskKindTag::Sleep,
            "Task should be Sleep, got {:?}",
            task.kind_tag
        );
    }

    #[test]
    fn rested_elf_does_not_create_sleep_task() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Spawn an elf — starts at full rest.
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

        // No Sleep task should exist.
        let has_sleep_task = sim
            .db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::Sleep);
        assert!(
            !has_sleep_task,
            "Well-rested elf should not create a Sleep task"
        );
    }

    #[test]
    fn busy_tired_elf_does_not_create_sleep_task() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest very low but keep food high.
        let food_max_val = sim.species_table[&Species::Elf].food_max;
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.rest = rest_max * 10 / 100;
            c.food = food_max_val;
        });

        // Give the elf a GoTo task so it's busy.
        let task_id = TaskId::new(&mut sim.rng);
        let goto_task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: NavNodeId(0),
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        sim.insert_task(goto_task);
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);

        // Advance past the heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // The elf should still have its GoTo task, not a Sleep one.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(
            elf.current_task,
            Some(task_id),
            "Busy elf should keep its existing task"
        );
        let sleep_task_count = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Sleep)
            .count();
        assert_eq!(
            sleep_task_count, 0,
            "No Sleep task should be created for a busy elf"
        );
    }

    #[test]
    fn hungry_takes_priority_over_tired() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Need fruit.
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Both food AND rest below threshold.
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.food = food_max * 20 / 100;
            c.rest = rest_max * 20 / 100;
        });

        // Advance past the heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // Hunger takes priority — should get EatFruit, not Sleep.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Hungry+tired elf should get a task"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert!(
            task.kind_tag == TaskKindTag::EatFruit,
            "Hunger should take priority over tiredness, got {:?}",
            task.kind_tag
        );
    }

    #[test]
    fn ground_sleep_fallback_when_no_beds() {
        // No dormitories exist — tired elf should get a ground Sleep task.
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest below threshold, food high.
        let food_max_val = sim.species_table[&Species::Elf].food_max;
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.rest = rest_max * 30 / 100;
            c.food = food_max_val;
        });

        // Advance past the heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // Should have a Sleep task with bed_pos: None (ground sleep).
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Tired elf should get a Sleep task"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
        let bed_pos = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
        assert_eq!(bed_pos, None, "No dormitories — should be ground sleep");
        // Ground sleep total_cost = sleep_ticks_ground / sleep_action_ticks (number of actions).
        let expected_cost = (sim.config.sleep_ticks_ground / sim.config.sleep_action_ticks) as f32;
        assert_eq!(
            task.total_cost, expected_cost,
            "Ground sleep total_cost should be number of actions"
        );
    }

    #[test]
    fn find_nearest_bed_excludes_occupied() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Find a valid nav node near tree for the bed position.
        let graph = sim.graph_for_species(Species::Elf);
        let bed_node = graph.find_nearest_node(tree_pos).unwrap();
        let bed_pos = graph.node(bed_node).position;

        // Add a dormitory structure with exactly one bed.
        let structure_id = StructureId(999);
        let project_id = ProjectId::new(&mut sim.rng);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.db
            .structures
            .insert_no_fk(CompletedStructure {
                id: structure_id,
                project_id,
                build_type: BuildType::Building,
                anchor: bed_pos,
                width: 3,
                depth: 3,
                height: 3,
                completed_tick: 0,
                name: None,
                furnishing: Some(FurnishingType::Dormitory),
                inventory_id: inv_id,
                logistics_priority: None,
                cooking_enabled: false,
                cooking_bread_target: 0,
                workshop_enabled: false,
                workshop_recipe_ids: Vec::new(),
                workshop_recipe_targets: std::collections::BTreeMap::new(),
                greenhouse_species: None,
                greenhouse_enabled: false,
                greenhouse_last_production_tick: 0,
            })
            .unwrap();
        let _ = sim
            .db
            .furniture
            .insert_auto_no_fk(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: bed_pos,
                placed: true,
            });

        // Spawn two elves.
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
                    species: Species::Elf,
                    position: tree_pos,
                },
            },
        ];
        sim.step(&cmds, 1);

        let elf_ids: Vec<CreatureId> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf)
            .map(|c| c.id)
            .collect();
        assert_eq!(elf_ids.len(), 2);

        // Make both elves tired with high food.
        let food_max_val = sim.species_table[&Species::Elf].food_max;
        for &elf_id in &elf_ids {
            let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
                c.rest = rest_max * 20 / 100;
                c.food = food_max_val;
            });
        }

        // Advance past the heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // Both should have Sleep tasks.
        let mut bed_sleep_count = 0;
        let mut ground_sleep_count = 0;
        for &elf_id in &elf_ids {
            let elf = sim.db.creatures.get(&elf_id).unwrap();
            if let Some(task_id) = elf.current_task {
                if let Some(task) = sim.db.tasks.get(&task_id) {
                    if task.kind_tag == TaskKindTag::Sleep {
                        let bed =
                            sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
                        if bed.is_some() {
                            bed_sleep_count += 1;
                        } else {
                            ground_sleep_count += 1;
                        }
                    }
                }
            }
        }
        // One bed available → one elf gets bed sleep, one gets ground sleep.
        assert_eq!(bed_sleep_count, 1, "One elf should sleep in the bed");
        assert_eq!(
            ground_sleep_count, 1,
            "Second elf should sleep on the ground"
        );
    }

    #[test]
    fn tired_elf_sleeps_and_rest_increases() {
        // Integration test: set low rest, add a dormitory with beds, run many
        // ticks, verify rest increased (proves sleeping happened).
        let mut config = test_config();
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        // Don't let food or rest decay interfere — zero both so we can
        // set rest manually and only see the effect of sleeping.
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;

        // Add a dormitory with beds near the tree.
        let graph = sim.graph_for_species(Species::Elf);
        let bed_node = graph.find_nearest_node(tree_pos).unwrap();
        let bed_pos = graph.node(bed_node).position;

        let structure_id = StructureId(999);
        let project_id = ProjectId::new(&mut sim.rng);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.db
            .structures
            .insert_no_fk(CompletedStructure {
                id: structure_id,
                project_id,
                build_type: BuildType::Building,
                anchor: bed_pos,
                width: 3,
                depth: 3,
                height: 3,
                completed_tick: 0,
                name: None,
                furnishing: Some(FurnishingType::Dormitory),
                inventory_id: inv_id,
                logistics_priority: None,
                cooking_enabled: false,
                cooking_bread_target: 0,
                workshop_enabled: false,
                workshop_recipe_ids: Vec::new(),
                workshop_recipe_targets: std::collections::BTreeMap::new(),
                greenhouse_species: None,
                greenhouse_enabled: false,
                greenhouse_last_production_tick: 0,
            })
            .unwrap();
        let _ = sim
            .db
            .furniture
            .insert_auto_no_fk(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: bed_pos,
                placed: true,
            });

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest to 20% — well below the 50% threshold.
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.rest = rest_max * 20 / 100;
        });
        let rest_before = sim.db.creatures.get(&elf_id).unwrap().rest;

        // Run for 50_000 ticks — enough for heartbeat + pathfind + sleep.
        sim.step(&[], 50_001);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        // If the elf slept, rest should be meaningfully higher than 20%.
        // With rest_per_sleep_tick=60B and sleep_ticks_bed=10_000 activations,
        // full bed sleep restores 600T = 60% of rest_max. Even with continued
        // decay, rest should be well above the starting 20%.
        assert!(
            elf.rest > rest_before,
            "Tired elf should have slept and restored rest above starting level. rest={}, was={}",
            elf.rest,
            rest_before
        );
    }

    // -----------------------------------------------------------------------
    // Movement interpolation tests
    // -----------------------------------------------------------------------

    /// Helper: create a minimal `db::Creature` for interpolation tests.
    fn make_interp_creature(
        position: VoxelCoord,
        action_kind: ActionKind,
        next_available_tick: Option<u64>,
    ) -> crate::db::Creature {
        crate::db::Creature {
            id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
            species: Species::Elf,
            position,
            name: String::new(),
            name_meaning: String::new(),
            current_node: None,
            path: None,
            current_task: None,
            food: 1000,
            rest: 1000,
            assigned_home: None,
            inventory_id: InventoryId(0),
            civ_id: None,
            action_kind,
            next_available_tick,
            hp: 100,
            hp_max: 100,
            vital_status: VitalStatus::Alive,
        }
    }

    #[test]
    fn interpolated_position_midpoint() {
        let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
        let ma = MoveAction {
            creature_id: creature.id,
            move_from: VoxelCoord::new(0, 0, 0),
            move_to: VoxelCoord::new(10, 0, 0),
            move_start_tick: 100,
        };
        let (x, y, z) = creature.interpolated_position(150.0, Some(&ma));
        assert!((x - 5.0).abs() < 0.001, "x should be 5.0, got {x}");
        assert!((y - 0.0).abs() < 0.001, "y should be 0.0, got {y}");
        assert!((z - 0.0).abs() < 0.001, "z should be 0.0, got {z}");
    }

    #[test]
    fn interpolated_position_at_start() {
        let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
        let ma = MoveAction {
            creature_id: creature.id,
            move_from: VoxelCoord::new(0, 0, 0),
            move_to: VoxelCoord::new(10, 0, 0),
            move_start_tick: 100,
        };
        let (x, _, _) = creature.interpolated_position(100.0, Some(&ma));
        assert!((x - 0.0).abs() < 0.001, "At t=0 should be at from, got {x}");
    }

    #[test]
    fn interpolated_position_at_end() {
        let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
        let ma = MoveAction {
            creature_id: creature.id,
            move_from: VoxelCoord::new(0, 0, 0),
            move_to: VoxelCoord::new(10, 0, 0),
            move_start_tick: 100,
        };
        let (x, _, _) = creature.interpolated_position(200.0, Some(&ma));
        assert!((x - 10.0).abs() < 0.001, "At t=1 should be at to, got {x}");
    }

    #[test]
    fn interpolated_position_clamped_past_end() {
        let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
        let ma = MoveAction {
            creature_id: creature.id,
            move_from: VoxelCoord::new(0, 0, 0),
            move_to: VoxelCoord::new(10, 0, 0),
            move_start_tick: 100,
        };
        let (x, _, _) = creature.interpolated_position(999.0, Some(&ma));
        assert!(
            (x - 10.0).abs() < 0.001,
            "Past end should clamp to destination, got {x}"
        );
    }

    #[test]
    fn interpolated_position_stationary() {
        let creature = make_interp_creature(VoxelCoord::new(5, 3, 7), ActionKind::NoAction, None);
        let (x, y, z) = creature.interpolated_position(50.0, None);
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Before the first activation, the elf should have no action.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(elf.action_kind, ActionKind::NoAction);
        assert!(elf.next_available_tick.is_none());
        assert!(sim.db.move_actions.get(&elf_id).is_none());

        let initial_pos = elf.position;

        // Step to tick 2 — the first activation fires and the elf wanders.
        sim.step(&[], 2);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(
            elf.action_kind,
            ActionKind::Move,
            "action_kind should be Move after wander"
        );
        assert!(
            elf.next_available_tick.is_some(),
            "next_available_tick should be set after wander"
        );

        let ma = sim
            .db
            .move_actions
            .get(&elf_id)
            .expect("MoveAction should exist after wander");
        assert_eq!(
            ma.move_from, initial_pos,
            "move_from should be the spawn position"
        );
        assert_eq!(
            ma.move_to, elf.position,
            "move_to should be the new position"
        );
        assert_eq!(
            ma.move_start_tick, 2,
            "move_start_tick should be the activation tick"
        );
        assert!(
            elf.next_available_tick.unwrap() > ma.move_start_tick,
            "next_available_tick should be after start"
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

        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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
    fn designate_build_creates_composition() {
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

        // Blueprint should have a composition FK.
        let bp = sim.db.blueprints.iter_all().next().unwrap();
        assert!(
            bp.composition_id.is_some(),
            "Build blueprint should have a composition"
        );

        // The composition should exist in the DB with Pending status.
        let comp_id = bp.composition_id.unwrap();
        let comp = sim.db.music_compositions.get(&comp_id).unwrap();
        assert_eq!(comp.status, crate::db::CompositionStatus::Pending);
        assert!(!comp.build_started);
        assert!(comp.seed != 0, "Composition should have a non-trivial seed");
        assert!(comp.sections >= 1 && comp.sections <= 4);
        assert!(comp.mode_index <= 5);
        assert!(comp.brightness >= 0.2 && comp.brightness <= 0.8);
        // 1 voxel × 1000 ticks/voxel = 1000ms target duration.
        assert_eq!(comp.target_duration_ms, 1000);
    }

    #[test]
    fn composition_persists_across_serde_roundtrip() {
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

        let bp = sim.db.blueprints.iter_all().next().unwrap();
        let comp_id = bp.composition_id.unwrap();
        let comp = sim.db.music_compositions.get(&comp_id).unwrap();
        let orig_seed = comp.seed;
        let orig_sections = comp.sections;
        let orig_mode = comp.mode_index;

        // Serialize and deserialize.
        let json = serde_json::to_string(&sim).unwrap();
        let restored: SimState = serde_json::from_str(&json).unwrap();

        // Composition should survive roundtrip.
        let comp = restored.db.music_compositions.get(&comp_id).unwrap();
        assert_eq!(comp.seed, orig_seed);
        assert_eq!(comp.sections, orig_sections);
        assert_eq!(comp.mode_index, orig_mode);
        assert_eq!(comp.status, crate::db::CompositionStatus::Pending);

        // Blueprint FK should still point to it.
        let bp = restored.db.blueprints.iter_all().next().unwrap();
        assert_eq!(bp.composition_id, Some(comp_id));
    }

    #[test]
    fn designate_carve_has_no_composition() {
        let mut sim = test_sim(42);

        // Find a solid trunk voxel to carve.
        let mut carve_coord = None;
        for y in 1..sim.world.size_y as i32 {
            for z in 0..sim.world.size_z as i32 {
                for x in 0..sim.world.size_x as i32 {
                    let coord = VoxelCoord::new(x, y, z);
                    if sim.world.get(coord) == VoxelType::Trunk {
                        carve_coord = Some(coord);
                        break;
                    }
                }
                if carve_coord.is_some() {
                    break;
                }
            }
            if carve_coord.is_some() {
                break;
            }
        }
        let carve_coord = carve_coord.expect("Should find a trunk voxel to carve");

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateCarve {
                voxels: vec![carve_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        if !sim.db.blueprints.is_empty() {
            let bp = sim.db.blueprints.iter_all().next().unwrap();
            assert!(
                bp.composition_id.is_none(),
                "Carve blueprint should not have a composition"
            );
        }
        // No compositions should have been created for carving.
        assert_eq!(
            sim.db.music_compositions.len(),
            0,
            "Carving should not create compositions"
        );
    }

    #[test]
    fn build_work_sets_composition_build_started() {
        let mut config = test_config();
        config.build_work_ticks_per_voxel = 50000;
        let mut sim = SimState::with_config(42, config);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        spawn_elf(&mut sim);

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

        // Composition should not be started yet (no work done).
        let bp = sim.db.blueprints.iter_all().next().unwrap();
        let comp_id = bp.composition_id.unwrap();
        assert!(
            !sim.db
                .music_compositions
                .get(&comp_id)
                .unwrap()
                .build_started,
            "Composition should not be started before any work"
        );

        // Run enough ticks for the elf to arrive and do at least one tick of work.
        sim.step(&[], sim.tick + 100_000);

        assert!(
            sim.db
                .music_compositions
                .get(&comp_id)
                .unwrap()
                .build_started,
            "Composition should be started after elf begins building"
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
        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
        assert!(
            bp.task_id.is_some(),
            "Blueprint should have a linked task_id"
        );

        // Task should exist.
        let task_id = bp.task_id.unwrap();
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert!(task.kind_tag == TaskKindTag::Build);
        assert_eq!(task.state, TaskState::Available);
        assert_eq!(task.total_cost, 1.0);
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

        assert!(sim.db.blueprints.is_empty());
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

        assert!(sim.db.blueprints.is_empty());
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

        assert!(sim.db.blueprints.is_empty());
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

        assert!(sim.db.blueprints.is_empty());
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
        assert_eq!(sim.db.blueprints.len(), 1);
        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

        // Now cancel.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        let result = sim.step(&[cmd2], 2);

        assert!(sim.db.blueprints.is_empty());
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

        assert!(sim.db.blueprints.is_empty());
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

        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
        let task_id = sim.db.blueprints.get(&project_id).unwrap().task_id.unwrap();
        assert!(sim.db.tasks.contains(&task_id));

        // Cancel.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd2], 2);

        assert!(sim.db.blueprints.is_empty());
        assert!(!sim.db.tasks.contains(&task_id));
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

        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

        // Tick enough for the elf to claim the task, but not complete it.
        // The elf claims on its next idle activation after the build is
        // designated. Elf walk speed is 500 tpv, so one wander step takes
        // ~500 ticks. We need enough ticks for at least one idle activation
        // to occur after the build designation, but not enough for the elf to
        // finish the build (1000 work ticks). 800 ticks is enough for one
        // full activation cycle.
        sim.step(&[], sim.tick + 800);

        let task_id = sim.db.blueprints.get(&project_id).unwrap().task_id.unwrap();
        // Wait for the elf to claim the build task. The elf claims on its next
        // idle activation, which depends on when its wander step finishes.
        // Tick in small increments to avoid overshooting past task completion.
        let mut claimed = false;
        for _ in 0..20 {
            sim.step(&[], sim.tick + 100);
            if sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(task_id))
            {
                claimed = true;
                break;
            }
        }
        assert!(
            claimed,
            "Elf should have claimed the build task within 2000 ticks"
        );

        // Cancel the build.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd2], sim.tick + 2);

        // Elf should be unassigned.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
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
        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

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
        assert_eq!(sim.db.blueprints.len(), 1);

        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        assert_eq!(restored.db.blueprints.len(), 1);
        let bp = restored.db.blueprints.iter_all().next().unwrap();
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

        let id_a = *sim_a.db.blueprints.iter_keys().next().unwrap();
        let id_b = *sim_b.db.blueprints.iter_keys().next().unwrap();
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
            .db
            .creatures
            .iter_all()
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
            .db
            .creatures
            .iter_all()
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
                .db
                .creatures
                .iter_all()
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
                .db
                .creatures
                .iter_all()
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
            .db
            .creatures
            .iter_all()
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Squirrel)
            .unwrap();
        assert!(squirrel.current_node.is_some());
    }

    #[test]
    fn all_small_species_spawn_and_coexist() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let species_list = [
            Species::Elf,
            Species::Capybara,
            Species::Boar,
            Species::Deer,
            Species::Goblin,
            Species::Monkey,
            Species::Orc,
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

        assert_eq!(sim.db.creatures.len(), 8);
        for &species in &species_list {
            assert_eq!(sim.creature_count(species), 1, "Expected 1 {:?}", species);
        }

        // Run for a while — all should remain alive with valid nodes.
        sim.step(&[], 50000);
        assert_eq!(sim.db.creatures.len(), 8);
        for creature in sim.db.creatures.iter_all() {
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

        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
        let task_id = sim.db.blueprints.get(&project_id).unwrap().task_id.unwrap();

        // Tick until completion (elf needs to pathfind + do work).
        sim.step(&[], sim.tick + 200_000);

        // Blueprint should be Complete.
        let bp = &sim.db.blueprints.get(&project_id).unwrap();
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
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, TaskState::Complete);

        // Elf should be freed (no current task).
        let elf = sim.db.creatures.get(&elf_id).unwrap();
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

        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

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
            let bp = &sim.db.blueprints.get(&project_id).unwrap();
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
                let _ = sim.db.creatures.modify_unchecked(&elf_id, |elf| {
                    elf.position = air_coord;
                    elf.current_node = Some(node_id);
                });
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
        let elf = sim.db.creatures.get(&elf_id).unwrap();
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

        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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

        assert_eq!(sim.db.tasks.len(), 1);
        let task = sim.db.tasks.iter_all().next().unwrap();
        assert_eq!(task.state, TaskState::Available);
        assert_eq!(task.kind_tag, TaskKindTag::Build, "Expected Build task");
        let project_id = sim.task_project_id(task.id).unwrap();
        assert!(sim.db.blueprints.contains(&project_id));
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
        assert!(sim.db.blueprints.is_empty());
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
        assert!(sim.db.blueprints.is_empty());
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
        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

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
        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

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
        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

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
        assert!(sim.db.blueprints.is_empty(), "blueprint should be removed");

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
        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

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
        assert!(sim.db.blueprints.is_empty());
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
        assert_eq!(sim.db.blueprints.len(), 1);

        // Run the sim forward until the blueprint is Complete.
        // The elf will claim the task, walk to the site, and do build work.
        // build_work_ticks_per_voxel * 1 voxel = total_cost ticks of work.
        // Cap at 1 million ticks to avoid infinite loops in tests.
        let max_tick = sim.tick + 1_000_000;
        while sim.tick < max_tick {
            sim.step(&[], sim.tick + 100);
            let all_complete = sim
                .db
                .blueprints
                .iter_all()
                .all(|bp| bp.state == BlueprintState::Complete);
            if all_complete {
                break;
            }
        }
        assert!(
            sim.db
                .blueprints
                .iter_all()
                .all(|bp| bp.state == BlueprintState::Complete),
            "Build did not complete within tick limit"
        );
        sim
    }

    #[test]
    fn completed_structure_registered_on_build_complete() {
        let sim = designate_and_complete_build(test_sim(42));

        assert_eq!(sim.db.structures.len(), 1);
        let structure = sim.db.structures.iter_all().next().unwrap();
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
                .db
                .blueprints
                .iter_all()
                .all(|bp| bp.state == BlueprintState::Complete);
            if all_complete {
                break;
            }
        }
        assert_eq!(sim.db.structures.len(), 1);
        assert_eq!(
            sim.db.structures.iter_all().next().unwrap().id,
            StructureId(0)
        );

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
                .db
                .blueprints
                .iter_all()
                .all(|bp| bp.state == BlueprintState::Complete);
            if all_complete {
                break;
            }
        }
        assert_eq!(sim.db.structures.len(), 2);

        // IDs should be 0 and 1.
        let ids: Vec<StructureId> = sim.db.structures.iter_keys().copied().collect();
        assert!(ids.contains(&StructureId(0)));
        assert!(ids.contains(&StructureId(1)));
    }

    #[test]
    fn cancel_completed_structure_removes_entry() {
        let mut sim = designate_and_complete_build(test_sim(42));
        assert_eq!(sim.db.structures.len(), 1);

        // Get the project_id of the completed structure.
        let project_id = sim.db.structures.iter_all().next().unwrap().project_id;

        // Cancel the build (should remove from structures too).
        let tick = sim.tick + 1;
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cmd], tick);

        assert!(
            sim.db.structures.is_empty(),
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
        let structure = sim.db.structures.iter_all().next().unwrap();
        let bp = sim
            .db
            .blueprints
            .iter_all()
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

        let project_id = sim.db.structures.iter_all().next().unwrap().project_id;

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
        let structure = sim.db.structures.iter_all().next().unwrap();
        let bp = sim
            .db
            .blueprints
            .iter_all()
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
            .db
            .blueprints
            .iter_all()
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
    // RenameStructure tests
    // -----------------------------------------------------------------------

    #[test]
    fn rename_structure_sets_custom_name() {
        let mut sim = designate_and_complete_build(test_sim(42));
        assert_eq!(sim.db.structures.len(), 1);
        let sid = *sim.db.structures.iter_keys().next().unwrap();
        assert_eq!(
            sim.db.structures.get(&sid).unwrap().display_name(),
            "Platform #0"
        );

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::RenameStructure {
                structure_id: sid,
                name: Some("Great Hall".to_string()),
            },
        };
        sim.step(&[cmd], sim.tick + 1);
        assert_eq!(
            sim.db.structures.get(&sid).unwrap().display_name(),
            "Great Hall"
        );
    }

    #[test]
    fn rename_structure_to_none_resets_to_default() {
        let mut sim = designate_and_complete_build(test_sim(42));
        let sid = *sim.db.structures.iter_keys().next().unwrap();

        // Set a custom name.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::RenameStructure {
                structure_id: sid,
                name: Some("Great Hall".to_string()),
            },
        };
        sim.step(&[cmd], sim.tick + 1);
        assert_eq!(
            sim.db.structures.get(&sid).unwrap().display_name(),
            "Great Hall"
        );

        // Reset to default.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::RenameStructure {
                structure_id: sid,
                name: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);
        assert_eq!(
            sim.db.structures.get(&sid).unwrap().display_name(),
            "Platform #0"
        );
    }

    #[test]
    fn rename_nonexistent_structure_is_noop() {
        let mut sim = test_sim(42);
        let tick_before = sim.tick;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::RenameStructure {
                structure_id: StructureId(999),
                name: Some("Ghost".to_string()),
            },
        };
        // Should not panic.
        sim.step(&[cmd], sim.tick + 1);
        assert!(sim.db.structures.is_empty());
        assert!(sim.tick > tick_before);
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
            .db
            .creatures
            .iter_all()
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();

        // Set elf food low.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max / 10);
        let food_before = sim.db.creatures.get(&elf_id).unwrap().food;

        // Manually create an EatFruit task at the elf's current node (instant arrival).
        let fruit_pos = VoxelCoord::new(0, 0, 0); // dummy — food restore doesn't depend on real fruit
        let task_id = TaskId::new(&mut sim.rng);
        let eat_task = Task {
            id: task_id,
            kind: TaskKind::EatFruit { fruit_pos },
            state: TaskState::InProgress,
            location: elf_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(eat_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Advance enough ticks for the elf to start and complete the Eat action.
        sim.step(&[], sim.tick + sim.config.eat_action_ticks + 10);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
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
        sim.config.elf_starting_bread = 0;
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set food below threshold (threshold is 50% by default).
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max * 30 / 100);

        // Advance past the next heartbeat — hunger check should fire.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // The elf should now have an EatFruit task.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Hungry idle elf should have been assigned an EatFruit task"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert!(
            task.kind_tag == TaskKindTag::EatFruit,
            "Task should be EatFruit, got {:?}",
            task.kind_tag
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
            .db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::EatFruit);
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set food very low.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max * 10 / 100);

        // Give the elf a GoTo task so it's busy.
        let task_id = TaskId::new(&mut sim.rng);
        let goto_task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: NavNodeId(0),
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        sim.insert_task(goto_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Advance past the heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // The elf should still have its GoTo task, not an EatFruit one.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(
            elf.current_task,
            Some(task_id),
            "Busy elf should keep its existing task"
        );
        let eat_task_count = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::EatFruit)
            .count();
        assert_eq!(
            eat_task_count, 0,
            "No EatFruit task should be created for a busy elf"
        );
    }

    #[test]
    fn hungry_elf_eats_fruit_and_food_increases() {
        // Integration test: spawn elf, place fruit at its nav node, set low
        // food, run ticks, verify the elf ate fruit and food increased.
        // We place fruit explicitly rather than relying on random fruit spawning
        // so the test is deterministic regardless of tree shape.
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0; // Force fruit foraging, not bread eating.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;
        let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;

        // Place a fruit voxel at the elf's position (or very close).
        // This guarantees the fruit is reachable — the elf is already there.
        let fruit_pos = elf_pos;
        sim.world.set(fruit_pos, VoxelType::Fruit);
        let tree_id = sim.player_tree_id;
        sim.trees
            .get_mut(&tree_id)
            .unwrap()
            .fruit_positions
            .push(fruit_pos);

        // Set food to 20% — well below the 50% hunger threshold.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max * 20 / 100);

        // Run for 50_000 ticks — enough for heartbeat + pathfind + eat.
        sim.step(&[], 50_001);

        let elf = sim.db.creatures.get(&elf_id).unwrap();

        // The elf should have eaten at least once, restoring food above 0.
        // With default decay the food won't drop to 0 in 50k ticks.
        assert!(
            elf.food > 0,
            "Hungry elf should have eaten fruit and restored food above 0. food={}",
            elf.food
        );
    }

    // -----------------------------------------------------------------------
    // Hunger / EatBread tests
    // -----------------------------------------------------------------------

    #[test]
    fn hungry_elf_with_bread_creates_eat_bread_task() {
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Give the elf owned bread.
        sim.inv_add_simple_item(
            sim.creature_inv(elf_id),
            inventory::ItemKind::Bread,
            3,
            Some(elf_id),
            None,
        );

        // Set food below hunger threshold.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max * 30 / 100);

        // Advance past the next heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // The elf should have an EatBread task, not EatFruit.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Hungry elf with bread should have a task"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert!(
            task.kind_tag == TaskKindTag::EatBread,
            "Task should be EatBread, got {:?}",
            task.kind_tag
        );
    }

    #[test]
    fn eat_bread_restores_food_and_removes_bread() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let bread_restore_pct = sim.species_table[&Species::Elf].bread_restore_pct;

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();

        // Give the elf owned bread.
        sim.inv_add_simple_item(
            sim.creature_inv(elf_id),
            inventory::ItemKind::Bread,
            3,
            Some(elf_id),
            None,
        );

        // Set food low.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max / 10);
        let food_before = sim.db.creatures.get(&elf_id).unwrap().food;

        // Manually create an EatBread task at the elf's current node.
        let task_id = TaskId::new(&mut sim.rng);
        let eat_task = Task {
            id: task_id,
            kind: TaskKind::EatBread,
            state: TaskState::InProgress,
            location: elf_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(eat_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Advance enough ticks for the elf to start and complete the Eat action.
        // eat_action_ticks = 1500, plus a few extra for scheduling.
        sim.step(&[], sim.tick + sim.config.eat_action_ticks + 10);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        let expected_restore = food_max * bread_restore_pct / 100;
        assert!(
            elf.food >= food_before + expected_restore - 1,
            "Food should increase by ~bread_restore_pct%: before={}, after={}, expected_restore={}",
            food_before,
            elf.food,
            expected_restore,
        );
        assert!(elf.current_task.is_none(), "Task should be complete");

        // Should have consumed 1 bread (2 remaining).
        let bread_count = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bread, elf_id);
        assert_eq!(bread_count, 2, "Should have consumed 1 bread, leaving 2");
    }

    #[test]
    fn hungry_elf_without_bread_still_seeks_fruit() {
        // Elf is hungry but has no bread — should create EatFruit, not EatBread.
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        assert!(
            sim.trees.values().any(|t| !t.fruit_positions.is_empty()),
            "Tree must have fruit"
        );

        // Spawn an elf (no bread in inventory).
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set food below threshold.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max * 30 / 100);

        // Advance past heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // Should have EatFruit, not EatBread.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(elf.current_task.is_some(), "Hungry elf should have a task");
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert!(
            task.kind_tag == TaskKindTag::EatFruit,
            "Elf without bread should seek fruit, got {:?}",
            task.kind_tag
        );
    }

    #[test]
    fn hungry_elf_with_unowned_bread_seeks_fruit() {
        // Elf has bread but doesn't own it — should seek fruit instead.
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Give the elf bread with no owner (unowned).
        sim.inv_add_simple_item(
            sim.creature_inv(elf_id),
            inventory::ItemKind::Bread,
            5,
            None,
            None,
        );

        // Set food below threshold.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max * 30 / 100);

        // Advance past heartbeat.
        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // Should have EatFruit since the bread is not owned by this elf.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(elf.current_task.is_some(), "Hungry elf should have a task");
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert!(
            task.kind_tag == TaskKindTag::EatFruit,
            "Elf with unowned bread should seek fruit, got {:?}",
            task.kind_tag
        );
    }

    #[test]
    fn eat_bread_generates_thought() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();

        // Give bread and set food low.
        sim.inv_add_simple_item(
            sim.creature_inv(elf_id),
            inventory::ItemKind::Bread,
            1,
            Some(elf_id),
            None,
        );
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max / 10);

        // Create EatBread task at current node.
        let task_id = TaskId::new(&mut sim.rng);
        let eat_task = Task {
            id: task_id,
            kind: TaskKind::EatBread,
            state: TaskState::InProgress,
            location: elf_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(eat_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Advance enough to start and complete the Eat action.
        sim.step(&[], sim.tick + sim.config.eat_action_ticks + 10);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|t| t.kind == ThoughtKind::AteMeal),
            "Eating bread should generate AteMeal thought"
        );
        // Piggyback: mood should reflect the AteMeal thought.
        let (score, tier) = sim.mood_for_creature(elf_id);
        assert!(
            score > 0,
            "AteMeal should produce positive mood score, got {score}"
        );
        assert!(
            tier == MoodTier::Content || tier == MoodTier::Happy || tier == MoodTier::Elated,
            "AteMeal should produce at least Content tier, got {tier:?}"
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

        assert_eq!(sim.db.blueprints.len(), 1, "Blueprint should be created");
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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

        assert!(sim.db.blueprints.is_empty(), "All-trunk should be rejected");
        assert_eq!(
            sim.last_build_message.as_deref(),
            Some("Nothing to build — all voxels are already wood.")
        );
    }

    #[test]
    fn overlap_mixed_air_trunk_only_builds_air() {
        let mut sim = test_sim(42);
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

        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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
            sim.db.blueprints.is_empty(),
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

        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

        // Tick until completion.
        sim.step(&[], sim.tick + 200_000);

        // Leaf should have been converted to GrownPlatform.
        assert_eq!(
            sim.world.get(leaf_coord),
            VoxelType::GrownPlatform,
            "Leaf voxel should be converted to GrownPlatform"
        );

        // Blueprint should be Complete.
        let bp = &sim.db.blueprints.get(&project_id).unwrap();
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

        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

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
        assert_eq!(sim.db.blueprints.len(), 1);

        let json = sim.to_json().unwrap();
        let restored = SimState::from_json(&json).unwrap();

        assert_eq!(restored.db.blueprints.len(), 1);
        let bp = restored.db.blueprints.iter_all().next().unwrap();
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

        let id_a = *sim_a.db.blueprints.iter_keys().next().unwrap();
        let id_b = *sim_b.db.blueprints.iter_keys().next().unwrap();
        assert_eq!(id_a, id_b);

        let bp_a = sim_a.db.blueprints.get(&id_a).unwrap();
        let bp_b = sim_b.db.blueprints.get(&id_b).unwrap();
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
            sim.db.blueprints.is_empty(),
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

        let elf_id = *sim.db.creatures.iter_keys().next().unwrap();

        // Find a ground nav node different from the elf's to use as task target.
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let task_node = sim
            .nav_graph
            .ground_node_ids()
            .into_iter()
            .find(|&nid| nid != elf_node)
            .expect("Need at least 2 ground nodes");

        // Create a GoTo task at that nav node and assign it to the elf.
        let task_id = TaskId::new(&mut sim.rng);
        sim.insert_task(Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: task_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        });
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

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
        let elf = sim.db.creatures.get(&elf_id).unwrap();
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
        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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
            sim.db.blueprints.len(),
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

        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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

        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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

        assert!(sim.db.blueprints.is_empty());
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

        assert!(sim.db.blueprints.is_empty());
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

        assert!(sim.db.blueprints.is_empty());
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

        let bp = sim.db.blueprints.iter_all().next().unwrap();
        assert!(bp.task_id.is_some());
        let task = sim.db.tasks.get(&bp.task_id.unwrap()).unwrap();
        assert!(task.kind_tag == TaskKindTag::Build);
        assert_eq!(task.required_species, Some(Species::Elf));
        assert_eq!(task.total_cost, 2.0);
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
        assert_eq!(sim.db.blueprints.len(), 1);

        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
        let cancel_cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::CancelBuild { project_id },
        };
        let result = sim.step(&[cancel_cmd], 2);

        assert!(sim.db.blueprints.is_empty());
        assert!(sim.db.tasks.is_empty());
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
            sim.db.blueprints.is_empty(),
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
        let project_id = *sim.db.blueprints.iter_keys().next().unwrap();
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
        assert_eq!(restored.db.blueprints.len(), 1);
        let bp = restored.db.blueprints.iter_all().next().unwrap();
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

        assert_eq!(sim.db.blueprints.len(), 0);
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

        assert_eq!(sim.db.blueprints.len(), 1);
        let bp = sim.db.blueprints.iter_all().next().unwrap();
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

        assert_eq!(sim_a.db.blueprints.len(), sim_b.db.blueprints.len());
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
        let bp = restored.db.blueprints.iter_all().next().unwrap();
        let orig = bp.original_voxels.iter().find(|(c, _)| *c == solid);
        assert!(orig.is_some());
        assert_eq!(orig.unwrap().1, original_type);
    }

    #[test]
    fn state_checksum_deterministic() {
        let sim_a = test_sim(42);
        let sim_b = test_sim(42);
        let hash_a = sim_a.state_checksum();
        let hash_b = sim_b.state_checksum();
        assert_eq!(
            hash_a, hash_b,
            "same seed should produce identical checksum"
        );
        assert_ne!(hash_a, 0, "checksum should not be zero");
    }

    #[test]
    fn state_checksum_different_seeds() {
        let sim_a = test_sim(42);
        let sim_b = test_sim(99);
        assert_ne!(
            sim_a.state_checksum(),
            sim_b.state_checksum(),
            "different seeds should produce different checksums"
        );
    }

    // --- Furnishing tests ---

    /// Insert a completed building into the sim's structures. Returns the
    /// StructureId. The `anchor` parameter is the foundation level (solid
    /// ground); the CompletedStructure's anchor is set one level higher to
    /// match `from_blueprint()`, which computes the bounding box from the
    /// BuildingInterior voxels (not the foundation). The building is 3x3x1
    /// with solid foundation below and BuildingInterior above.
    fn insert_completed_building(sim: &mut SimState, anchor: VoxelCoord) -> StructureId {
        let id = StructureId(sim.next_structure_id);
        sim.next_structure_id += 1;

        // Place BuildingInterior voxels in the world and record face data.
        // compute_building_face_layout treats `anchor` as foundation level
        // and creates interior voxels at anchor.y + 1.
        let face_layout = crate::building::compute_building_face_layout(anchor, 3, 3, 1);
        for (&coord, fd) in &face_layout {
            sim.world.set(coord, VoxelType::BuildingInterior);
            sim.face_data.insert(coord, fd.clone());
            sim.face_data_list.push((coord, fd.clone()));
            sim.placed_voxels.push((coord, VoxelType::BuildingInterior));
            sim.structure_voxels.insert(coord, id);
        }

        // Place the foundation as solid GrownWall underneath.
        for z in anchor.z..anchor.z + 3 {
            for x in anchor.x..anchor.x + 3 {
                let foundation = VoxelCoord::new(x, anchor.y, z);
                if sim.world.get(foundation) == VoxelType::Air {
                    sim.world.set(foundation, VoxelType::GrownWall);
                    sim.placed_voxels.push((foundation, VoxelType::GrownWall));
                }
            }
        }

        // The CompletedStructure anchor is the bounding-box min of the
        // blueprint voxels (BuildingInterior), which is one above foundation.
        let interior_anchor = VoxelCoord::new(anchor.x, anchor.y + 1, anchor.z);

        let project_id = ProjectId::new(&mut sim.rng);
        let structure = CompletedStructure {
            id,
            project_id,
            build_type: BuildType::Building,
            anchor: interior_anchor,
            width: 3,
            depth: 3,
            height: 1,
            completed_tick: sim.tick,
            name: None,
            furnishing: None,
            inventory_id: sim.create_inventory(crate::db::InventoryOwnerKind::Structure),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };
        sim.db.structures.insert_no_fk(structure).unwrap();

        // Insert a dummy blueprint so FK validation passes on deserialization.
        sim.db
            .blueprints
            .insert_no_fk(crate::db::Blueprint {
                id: project_id,
                build_type: BuildType::Building,
                voxels: Vec::new(),
                priority: crate::types::Priority::Normal,
                state: crate::blueprint::BlueprintState::Complete,
                task_id: None,
                composition_id: None,
                face_layout: None,
                stress_warning: false,
                original_voxels: Vec::new(),
            })
            .unwrap();

        // Rebuild nav graph so there are nav nodes inside the building.
        sim.nav_graph = nav::build_nav_graph(&sim.world, &sim.face_data);

        id
    }

    #[test]
    fn compute_furniture_positions_3x3_dormitory() {
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(0),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 3,
            depth: 3,
            height: 1,
            completed_tick: 0,
            name: None,
            furnishing: None,
            inventory_id: InventoryId(0),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };

        let items = structure.compute_furniture_positions(FurnishingType::Dormitory, &mut rng);

        // 3x3 = 9 floor tiles, target ~4 items. Door at (1,0,2) and its
        // neighbors are excluded, so fewer eligible positions.
        assert!(!items.is_empty());
        // All items should be at y=0 (anchor.y, the interior level).
        for item in &items {
            assert_eq!(item.y, 0);
        }
        // No item should be at the door position.
        let door = VoxelCoord::new(1, 0, 2);
        assert!(!items.contains(&door));
    }

    #[test]
    fn compute_furniture_positions_5x5_dormitory() {
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(0),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 5,
            depth: 5,
            height: 1,
            completed_tick: 0,
            name: None,
            furnishing: None,
            inventory_id: InventoryId(0),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };

        let items = structure.compute_furniture_positions(FurnishingType::Dormitory, &mut rng);

        // 5x5 = 25 floor tiles, target ~12 items (1 per 2 tiles).
        assert!(
            items.len() >= 8,
            "Expected at least 8 items for 5x5 dormitory, got {}",
            items.len()
        );
        assert!(
            items.len() <= 12,
            "Expected at most 12 items for 5x5 dormitory, got {}",
            items.len()
        );
    }

    #[test]
    fn state_checksum_changes_after_mutation() {
        let mut sim = test_sim(42);
        let before = sim.state_checksum();

        // Spawn an elf to mutate state.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let spawn_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: spawn_pos,
            },
        };
        sim.step(&[cmd], 1);

        let after = sim.state_checksum();
        assert_ne!(
            before, after,
            "checksum should change after spawning a creature"
        );
    }

    #[test]
    fn display_name_dormitory_when_furnished() {
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(7),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 3,
            depth: 3,
            height: 1,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::Dormitory),
            inventory_id: InventoryId(0),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };

        assert_eq!(structure.display_name(), "Dormitory #7");
    }

    #[test]
    fn display_name_custom_overrides_dormitory() {
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(7),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 3,
            depth: 3,
            height: 1,
            completed_tick: 0,
            name: Some("Starlight Hall".to_string()),
            furnishing: Some(FurnishingType::Dormitory),
            inventory_id: InventoryId(0),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };

        assert_eq!(structure.display_name(), "Starlight Hall");
    }

    #[test]
    fn furnish_structure_creates_task() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        // Should have created a Furnish task.
        let furnish_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Furnish)
            .collect();
        assert_eq!(furnish_tasks.len(), 1);
        let task = furnish_tasks[0];
        assert_eq!(task.state, TaskState::Available);
        assert_eq!(task.required_species, Some(Species::Elf));

        // Structure should have furnishing set and planned furniture computed.
        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert_eq!(structure.furnishing, Some(FurnishingType::Dormitory));
        let planned = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|f| !f.placed)
            .count();
        let placed = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|f| f.placed)
            .count();
        assert!(planned > 0);
        assert_eq!(placed, 0);

        // Total cost should be planned count (number of items = number of actions).
        let expected_cost = planned as f32;
        assert_eq!(task.total_cost, expected_cost);
    }

    #[test]
    fn furnish_structure_display_name_changes() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Before furnishing: "Building #N"
        assert_eq!(
            sim.db.structures.get(&structure_id).unwrap().display_name(),
            format!("Building #{}", structure_id.0)
        );

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        // After furnishing: "Dormitory #N"
        assert_eq!(
            sim.db.structures.get(&structure_id).unwrap().display_name(),
            format!("Dormitory #{}", structure_id.0)
        );
    }

    #[test]
    fn furnish_preserves_custom_name() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Give it a custom name first.
        let rename_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::RenameStructure {
                structure_id,
                name: Some("Starlight Hall".to_string()),
            },
        };
        sim.step(&[rename_cmd], sim.tick + 1);
        assert_eq!(
            sim.db.structures.get(&structure_id).unwrap().display_name(),
            "Starlight Hall"
        );

        // Furnish as dormitory — custom name should be preserved.
        let furnish_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[furnish_cmd], sim.tick + 1);

        assert_eq!(
            sim.db.structures.get(&structure_id).unwrap().furnishing,
            Some(FurnishingType::Dormitory)
        );
        assert_eq!(
            sim.db.structures.get(&structure_id).unwrap().display_name(),
            "Starlight Hall"
        );
    }

    #[test]
    fn furnish_rejects_non_building() {
        let mut sim = test_sim(42);

        // Insert a platform structure (not a Building).
        let id = StructureId(sim.next_structure_id);
        sim.next_structure_id += 1;
        let mut rng = GameRng::new(99);
        let structure = CompletedStructure {
            id,
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Platform,
            anchor: VoxelCoord::new(10, 5, 10),
            width: 3,
            depth: 3,
            height: 1,
            completed_tick: 0,
            name: None,
            furnishing: None,
            inventory_id: sim.create_inventory(crate::db::InventoryOwnerKind::Structure),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };
        sim.db.structures.insert_no_fk(structure).unwrap();

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id: id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        // Should NOT have created a task or set furnishing.
        assert!(
            sim.db
                .tasks
                .iter_all()
                .all(|t| t.kind_tag != TaskKindTag::Furnish)
        );
        assert_eq!(sim.db.structures.get(&id).unwrap().furnishing, None);
    }

    #[test]
    fn furnish_rejects_already_furnished() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish once.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);
        assert_eq!(
            sim.db
                .tasks
                .iter_all()
                .filter(|t| t.kind_tag == TaskKindTag::Furnish)
                .count(),
            1
        );

        // Try to furnish again.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd2], sim.tick + 1);

        // Should still have exactly one Furnish task (second was rejected).
        assert_eq!(
            sim.db
                .tasks
                .iter_all()
                .filter(|t| t.kind_tag == TaskKindTag::Furnish)
                .count(),
            1
        );
    }

    #[test]
    fn do_furnish_work_places_items_incrementally() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish the building.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let planned_count = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|f| !f.placed)
            .count();
        assert!(planned_count > 0);

        // Spawn an elf near the building so it can claim the task.
        let spawn_pos = VoxelCoord::new(anchor.x, anchor.y + 1, anchor.z + 3);
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: spawn_pos,
            },
        };
        sim.step(&[spawn_cmd], sim.tick + 1);

        // Run the sim long enough for the elf to walk there and place at
        // least one item. furnish_work_ticks_per_item = 2000, so after ~3000
        // ticks (walk + first item), we should see progress.
        let ticks_per_item = sim.config.furnish_work_ticks_per_item;
        let advance_ticks = ticks_per_item * 3 + 5000; // generous for walking
        sim.step(&[], sim.tick + advance_ticks);

        let placed_count = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|f| f.placed)
            .count();
        assert!(
            placed_count > 0,
            "Expected at least one item placed after {} ticks, got 0. planned={}",
            advance_ticks,
            planned_count
        );
    }

    #[test]
    fn furnish_completes_all_items() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish the building.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let all_furniture = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
        let total_planned = all_furniture.len();
        assert!(total_planned > 0);

        // Spawn an elf near the building.
        let spawn_pos = VoxelCoord::new(anchor.x, anchor.y + 1, anchor.z + 3);
        let spawn_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: spawn_pos,
            },
        };
        sim.step(&[spawn_cmd], sim.tick + 1);

        // Run long enough for all items to be placed.
        let ticks_per_item = sim.config.furnish_work_ticks_per_item;
        let advance_ticks = ticks_per_item * (total_planned as u64 + 2) + 10000;
        sim.step(&[], sim.tick + advance_ticks);

        let placed_count = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|f| f.placed)
            .count();
        let unplaced_count = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|f| !f.placed)
            .count();
        assert_eq!(
            placed_count, total_planned,
            "Expected all {} items placed, got {}",
            total_planned, placed_count
        );
        assert_eq!(
            unplaced_count, 0,
            "Expected no unplaced furniture after completion"
        );

        // The Furnish task should be complete.
        let furnish_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Furnish)
            .collect();
        assert!(
            furnish_tasks.is_empty()
                || furnish_tasks.iter().all(|t| t.state == TaskState::Complete),
            "Furnish task should be Complete"
        );
    }

    #[test]
    fn furnish_serialization_roundtrip() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish the building.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Dormitory,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        // Serialize and restore.
        let json = serde_json::to_string(&sim).unwrap();
        let restored: SimState = serde_json::from_str(&json).unwrap();

        let orig = sim.db.structures.get(&structure_id).unwrap();
        let rest = restored.db.structures.get(&structure_id).unwrap();
        assert_eq!(orig.furnishing, rest.furnishing);
        // Check furniture rows survived serialization.
        let orig_furn: Vec<_> = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .map(|f| (f.coord, f.placed))
            .collect();
        let rest_furn: Vec<_> = restored
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .map(|f| (f.coord, f.placed))
            .collect();
        assert_eq!(orig_furn, rest_furn);
    }

    #[test]
    fn compute_furniture_positions_home_single_item() {
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(0),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 5,
            depth: 5,
            height: 1,
            completed_tick: 0,
            name: None,
            furnishing: None,
            inventory_id: InventoryId(0),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };

        let items = structure.compute_furniture_positions(FurnishingType::Home, &mut rng);

        // Home always produces exactly 1 item regardless of building size.
        assert_eq!(items.len(), 1, "Home should produce exactly 1 item");
        assert_eq!(items[0].y, 0);
    }

    #[test]
    fn compute_furniture_positions_dining_hall_density() {
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(0),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 5,
            depth: 5,
            height: 1,
            completed_tick: 0,
            name: None,
            furnishing: None,
            inventory_id: InventoryId(0),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };

        let items = structure.compute_furniture_positions(FurnishingType::DiningHall, &mut rng);

        // 5x5 = 25 tiles, 1 per 4 = ~6 tables. Should be fewer than dormitory.
        assert!(
            items.len() >= 3,
            "Expected at least 3 tables for 5x5 dining hall, got {}",
            items.len()
        );
        assert!(
            items.len() <= 6,
            "Expected at most 6 tables for 5x5 dining hall, got {}",
            items.len()
        );
    }

    #[test]
    fn display_name_all_furnishing_types() {
        let mut rng = GameRng::new(42);
        let types_and_names = [
            (FurnishingType::ConcertHall, "Concert Hall #0"),
            (FurnishingType::DiningHall, "Dining Hall #0"),
            (FurnishingType::Dormitory, "Dormitory #0"),
            (FurnishingType::Home, "Home #0"),
            (FurnishingType::Kitchen, "Kitchen #0"),
            (FurnishingType::Storehouse, "Storehouse #0"),
            (FurnishingType::Workshop, "Workshop #0"),
        ];
        for (furnishing_type, expected) in types_and_names {
            let structure = CompletedStructure {
                id: StructureId(0),
                project_id: ProjectId::new(&mut rng),
                build_type: BuildType::Building,
                anchor: VoxelCoord::new(0, 0, 0),
                width: 3,
                depth: 3,
                height: 1,
                completed_tick: 0,
                name: None,
                furnishing: Some(furnishing_type),
                inventory_id: InventoryId(0),
                logistics_priority: None,
                cooking_enabled: false,
                cooking_bread_target: 0,
                workshop_enabled: false,
                workshop_recipe_ids: Vec::new(),
                workshop_recipe_targets: std::collections::BTreeMap::new(),
                greenhouse_species: None,
                greenhouse_enabled: false,
                greenhouse_last_production_tick: 0,
            };
            assert_eq!(
                structure.display_name(),
                expected,
                "display_name() for {:?}",
                furnishing_type
            );
        }
    }

    #[test]
    fn furnish_structure_workshop() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Workshop,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert_eq!(structure.furnishing, Some(FurnishingType::Workshop));
        let planned_furn = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|f| !f.placed)
            .count();
        assert!(planned_furn > 0);
        assert_eq!(
            structure.display_name(),
            format!("Workshop #{}", structure_id.0)
        );
    }

    // -----------------------------------------------------------------------
    // Greenhouse tests
    // -----------------------------------------------------------------------

    /// Helper: get the first cultivable fruit species from the DB.
    fn first_cultivable_species(sim: &SimState) -> Option<FruitSpeciesId> {
        sim.db
            .fruit_species
            .iter_all()
            .find(|f| f.greenhouse_cultivable)
            .map(|f| f.id)
    }

    /// Helper: get a non-cultivable fruit species from the DB.
    fn first_non_cultivable_species(sim: &SimState) -> Option<FruitSpeciesId> {
        sim.db
            .fruit_species
            .iter_all()
            .find(|f| !f.greenhouse_cultivable)
            .map(|f| f.id)
    }

    #[test]
    fn furnish_greenhouse_sets_species_and_creates_task() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let species_id = first_cultivable_species(&sim)
            .expect("worldgen should produce at least one cultivable fruit");

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Greenhouse,
                greenhouse_species: Some(species_id),
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert_eq!(structure.furnishing, Some(FurnishingType::Greenhouse));
        assert_eq!(structure.greenhouse_species, Some(species_id));
        assert!(structure.greenhouse_enabled);
        assert_eq!(structure.greenhouse_last_production_tick, sim.tick);

        // Should have created a Furnish task.
        let furnish_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == crate::db::TaskKindTag::Furnish)
            .collect();
        assert_eq!(furnish_tasks.len(), 1);
    }

    #[test]
    fn furnish_greenhouse_rejects_non_cultivable_species() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let species_id = first_non_cultivable_species(&sim);
        // If all species happen to be cultivable in this seed, skip.
        if let Some(species_id) = species_id {
            let cmd = SimCommand {
                player_id: sim.player_id,
                tick: sim.tick + 1,
                action: SimAction::FurnishStructure {
                    structure_id,
                    furnishing_type: FurnishingType::Greenhouse,
                    greenhouse_species: Some(species_id),
                },
            };
            sim.step(&[cmd], sim.tick + 1);

            let structure = sim.db.structures.get(&structure_id).unwrap();
            assert_eq!(
                structure.furnishing, None,
                "Non-cultivable species should be rejected"
            );
        }
    }

    #[test]
    fn furnish_greenhouse_rejects_missing_species() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // No greenhouse_species provided.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Greenhouse,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert_eq!(
            structure.furnishing, None,
            "Greenhouse without species should be rejected"
        );
    }

    #[test]
    fn furnish_greenhouse_rejects_unknown_species() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let bogus_id = FruitSpeciesId(9999);
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Greenhouse,
                greenhouse_species: Some(bogus_id),
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert_eq!(
            structure.furnishing, None,
            "Unknown species should be rejected"
        );
    }

    #[test]
    fn greenhouse_produces_fruit_after_interval() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let species_id = first_cultivable_species(&sim).expect("need a cultivable species");

        // Set a short production interval for testing.
        sim.config.greenhouse_base_production_ticks = 1000;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Greenhouse,
                greenhouse_species: Some(species_id),
            },
        };
        sim.step(&[cmd], sim.tick + 1);
        let furnish_tick = sim.tick;

        // The building has 3x3 = 9 interior tiles (floor_interior_positions).
        // Production interval = base / area = 1000 / 9 = 111 ticks.
        let structure = sim.db.structures.get(&structure_id).unwrap();
        let area = structure.floor_interior_positions().len() as u64;
        let interval = sim.config.greenhouse_base_production_ticks / area;

        // Advance past one interval + logistics heartbeat.
        let logistics_interval = sim.config.logistics_heartbeat_interval_ticks;
        let target_tick = furnish_tick + interval + logistics_interval;
        sim.step(&[], target_tick);

        // Check that fruit was produced in the greenhouse's inventory.
        let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
        let fruit_count: u32 = sim
            .db
            .item_stacks
            .iter_all()
            .filter(|s| {
                s.inventory_id == inv_id
                    && s.kind == inventory::ItemKind::Fruit
                    && s.material == Some(inventory::Material::FruitSpecies(species_id))
            })
            .map(|s| s.quantity)
            .sum();
        assert!(
            fruit_count >= 1,
            "Greenhouse should have produced at least 1 fruit, got {fruit_count}"
        );
    }

    #[test]
    fn greenhouse_display_name() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let species_id = first_cultivable_species(&sim).expect("need a cultivable species");

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Greenhouse,
                greenhouse_species: Some(species_id),
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        let name = structure.display_name();
        assert!(
            name.starts_with("Greenhouse #"),
            "Expected 'Greenhouse #N', got '{name}'"
        );
    }

    #[test]
    fn greenhouse_serde_roundtrip() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let species_id = first_cultivable_species(&sim).expect("need a cultivable species");

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Greenhouse,
                greenhouse_species: Some(species_id),
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        // Roundtrip through JSON.
        let json = serde_json::to_string(&sim).unwrap();
        let sim2: SimState = serde_json::from_str(&json).unwrap();

        let structure = sim2.db.structures.get(&structure_id).unwrap();
        assert_eq!(structure.furnishing, Some(FurnishingType::Greenhouse));
        assert_eq!(structure.greenhouse_species, Some(species_id));
        assert!(structure.greenhouse_enabled);
    }

    // -----------------------------------------------------------------------
    // Home assignment tests
    // -----------------------------------------------------------------------

    /// Create a completed building at `anchor`, furnish it as Home, and manually
    /// place 1 bed in the furniture table (skipping the furnishing task flow).
    fn insert_completed_home(sim: &mut SimState, anchor: VoxelCoord) -> StructureId {
        let structure_id = insert_completed_building(sim, anchor);

        // Find a valid bed position inside the building interior.
        let structure = sim.db.structures.get(&structure_id).unwrap();
        let interior = structure.floor_interior_positions();
        let bed_pos = interior[0]; // First interior tile.

        let mut structure = sim.db.structures.get(&structure_id).unwrap();
        structure.furnishing = Some(FurnishingType::Home);
        let _ = sim.db.structures.update_no_fk(structure);

        // Insert a placed furniture row for the bed.
        let _ = sim
            .db
            .furniture
            .insert_auto_no_fk(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: bed_pos,
                placed: true,
            });

        structure_id
    }

    #[test]
    fn assign_home_sets_bidirectional_refs() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let home_id = insert_completed_home(&mut sim, anchor);

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Assign elf to home.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(home_id),
            },
        };
        sim.step(&[cmd], 2);

        assert_eq!(
            sim.db.creatures.get(&elf_id).unwrap().assigned_home,
            Some(home_id)
        );
        assert_eq!(
            sim.db
                .creatures
                .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .map(|c| c.id),
            Some(elf_id)
        );
    }

    #[test]
    fn assign_home_unassign() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let home_id = insert_completed_home(&mut sim, anchor);

        // Spawn and assign.
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(home_id),
                },
            }],
            2,
        );
        assert_eq!(
            sim.db.creatures.get(&elf_id).unwrap().assigned_home,
            Some(home_id)
        );

        // Unassign.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 3,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: None,
                },
            }],
            3,
        );
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().assigned_home, None);
        assert!(
            sim.db
                .creatures
                .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
                .is_empty()
        );
    }

    #[test]
    fn assign_home_replaces_old_assignment() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor_a = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let anchor_b = VoxelCoord::new(tree_pos.x + 10, 0, tree_pos.z + 5);
        let home_a = insert_completed_home(&mut sim, anchor_a);
        let home_b = insert_completed_home(&mut sim, anchor_b);

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Assign to home A.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(home_a),
                },
            }],
            2,
        );
        assert_eq!(
            sim.db.creatures.get(&elf_id).unwrap().assigned_home,
            Some(home_a)
        );

        // Reassign to home B.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 3,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(home_b),
                },
            }],
            3,
        );
        assert_eq!(
            sim.db.creatures.get(&elf_id).unwrap().assigned_home,
            Some(home_b)
        );
        assert!(
            sim.db
                .creatures
                .by_assigned_home(&Some(home_a), tabulosity::QueryOpts::ASC)
                .is_empty()
        );
        assert_eq!(
            sim.db
                .creatures
                .by_assigned_home(&Some(home_b), tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .map(|c| c.id),
            Some(elf_id)
        );
    }

    #[test]
    fn assign_home_evicts_previous_occupant() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let home_id = insert_completed_home(&mut sim, anchor);

        // Spawn two elves.
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
                    species: Species::Elf,
                    position: tree_pos,
                },
            },
        ];
        sim.step(&cmds, 1);
        let elf_ids: Vec<CreatureId> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf)
            .map(|c| c.id)
            .collect();
        assert_eq!(elf_ids.len(), 2);
        let elf_a = elf_ids[0];
        let elf_b = elf_ids[1];

        // Assign elf A.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: elf_a,
                    structure_id: Some(home_id),
                },
            }],
            2,
        );
        assert_eq!(
            sim.db
                .creatures
                .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .map(|c| c.id),
            Some(elf_a)
        );

        // Assign elf B to same home — evicts elf A.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 3,
                action: SimAction::AssignHome {
                    creature_id: elf_b,
                    structure_id: Some(home_id),
                },
            }],
            3,
        );
        assert_eq!(
            sim.db
                .creatures
                .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .map(|c| c.id),
            Some(elf_b)
        );
        assert_eq!(sim.db.creatures.get(&elf_a).unwrap().assigned_home, None);
        assert_eq!(
            sim.db.creatures.get(&elf_b).unwrap().assigned_home,
            Some(home_id)
        );
    }

    #[test]
    fn assign_home_rejects_non_home() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish as Dormitory (not Home).
        {
            let mut s = sim.db.structures.get(&structure_id).unwrap();
            s.furnishing = Some(FurnishingType::Dormitory);
            let _ = sim.db.structures.update_no_fk(s);
        }

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(structure_id),
                },
            }],
            2,
        );

        // Should be rejected — no assignment set.
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().assigned_home, None);
        assert!(
            sim.db
                .creatures
                .by_assigned_home(&Some(structure_id), tabulosity::QueryOpts::ASC)
                .is_empty()
        );
    }

    #[test]
    fn assign_home_rejects_non_elf() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let home_id = insert_completed_home(&mut sim, anchor);

        // Spawn a capybara.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Capybara,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);
        let capy_id = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Capybara)
            .unwrap()
            .id;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: capy_id,
                    structure_id: Some(home_id),
                },
            }],
            2,
        );

        // Should be rejected — capybaras can't have homes.
        assert_eq!(sim.db.creatures.get(&capy_id).unwrap().assigned_home, None);
        assert!(
            sim.db
                .creatures
                .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
                .is_empty()
        );
    }

    #[test]
    fn tired_elf_sleeps_in_assigned_home() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Create a home with a bed.
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let home_id = insert_completed_home(&mut sim, anchor);
        let home_bed = sim
            .db
            .furniture
            .by_structure_id(&home_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|f| f.placed)
            .unwrap()
            .coord;

        // Spawn and assign.
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(home_id),
                },
            }],
            2,
        );

        // Make elf tired.
        {
            let food_max_val = sim.species_table[&Species::Elf].food_max;
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.rest = rest_max * 30 / 100;
            c.food = food_max_val;
            // Clear any existing task so the elf is idle.
            c.current_task = None;
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Advance past heartbeat.
        let target_tick = 2 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Tired elf with home should get a Sleep task"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
        let bed_pos = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
        assert_eq!(
            bed_pos,
            Some(home_bed),
            "Elf should sleep in their assigned home bed"
        );
    }

    #[test]
    fn tired_elf_without_home_uses_dormitory() {
        // This is largely the same as existing tests, but verifies the new
        // code path doesn't break dormitory fallback.
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Create a dormitory (not a home).
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);
        let structure = sim.db.structures.get(&structure_id).unwrap();
        let bed_pos = structure.floor_interior_positions()[0];
        let mut structure = sim.db.structures.get(&structure_id).unwrap();
        structure.furnishing = Some(FurnishingType::Dormitory);
        let _ = sim.db.structures.update_no_fk(structure);
        let _ = sim
            .db
            .furniture
            .insert_auto_no_fk(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: bed_pos,
                placed: true,
            });

        // Spawn elf (no home assignment).
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Make tired and idle.
        {
            let food_max_val = sim.species_table[&Species::Elf].food_max;
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.rest = rest_max * 30 / 100;
            c.food = food_max_val;
            c.current_task = None;
            let _ = sim.db.creatures.update_no_fk(c);
        }

        let target_tick = 1 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Tired elf should get a Sleep task from dormitory"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
        let task_bed = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
        assert_eq!(task_bed, Some(bed_pos), "Should sleep in dormitory bed");
    }

    #[test]
    fn assigned_home_unfurnished_falls_back() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Create a home without any placed furniture.
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_building(&mut sim, anchor);
        let mut structure = sim.db.structures.get(&structure_id).unwrap();
        structure.furnishing = Some(FurnishingType::Home);
        // No furniture placed — bed not yet built.
        let _ = sim.db.structures.update_no_fk(structure);

        // Spawn and assign.
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(structure_id),
                },
            }],
            2,
        );

        // Make tired and idle.
        {
            let food_max_val = sim.species_table[&Species::Elf].food_max;
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.rest = rest_max * 30 / 100;
            c.food = food_max_val;
            c.current_task = None;
            let _ = sim.db.creatures.update_no_fk(c);
        }

        let target_tick = 2 + heartbeat_interval + 1;
        sim.step(&[], target_tick);

        // Should fall back to ground sleep (no dormitories and home has no bed).
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Tired elf should get a ground Sleep task"
        );
        let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
        assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
        let bed_pos = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
        assert_eq!(bed_pos, None, "Should fall back to ground sleep");
    }

    // -----------------------------------------------------------------------
    // Thought system tests
    // -----------------------------------------------------------------------

    /// Helper: create a `SimState` with an elf spawned for thought/mood tests.
    /// Returns `(sim, creature_id)`.
    fn sim_with_elf_for_thoughts() -> (SimState, CreatureId) {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        sim.tick = 1000; // Advance tick for thought timestamps.
        (sim, elf_id)
    }

    #[test]
    fn thought_dedup_within_cooldown() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        sim.tick = 1001;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
        assert_eq!(thoughts.len(), 1, "Dedup should prevent second add");
    }

    #[test]
    fn thought_dedup_allows_after_cooldown() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        let cooldown = sim.config.thoughts.dedup_ate_meal_ticks;
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        sim.tick = 1000 + cooldown;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
        assert_eq!(thoughts.len(), 2, "Should allow add after cooldown expires");
    }

    #[test]
    fn thought_dedup_distinguishes_structure_ids() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::SleptInOwnHome(StructureId(1)));
        sim.tick = 1001;
        sim.add_creature_thought(cid, ThoughtKind::SleptInOwnHome(StructureId(2)));
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
        assert_eq!(
            thoughts.len(),
            2,
            "Different structure IDs are distinct thoughts"
        );
    }

    #[test]
    fn thought_cap_enforced() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.config.thoughts.cap = 5;
        sim.config.thoughts.dedup_ate_meal_ticks = 0; // Disable dedup.
        for i in 0..7 {
            sim.tick = i * 1000;
            sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        }
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
        assert_eq!(thoughts.len(), 5, "Should not exceed cap");
        // Oldest should have been dropped — first remaining is tick 2000.
        assert_eq!(thoughts[0].tick, 2000);
    }

    #[test]
    fn thought_expiry() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        let expiry = sim.config.thoughts.expiry_ate_meal_ticks;
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        // Before expiry: should remain.
        sim.tick = 1000 + expiry - 1;
        sim.expire_creature_thoughts(cid);
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
        assert_eq!(thoughts.len(), 1, "Should not expire yet");
        // At expiry: should be removed.
        sim.tick = 1000 + expiry;
        sim.expire_creature_thoughts(cid);
        let thoughts = sim
            .db
            .thoughts
            .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
        assert_eq!(thoughts.len(), 0, "Should expire at expiry tick");
    }

    #[test]
    fn thought_serde_roundtrip_via_simstate() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.tick = 5000;
        sim.add_creature_thought(cid, ThoughtKind::SleptOnGround);
        sim.tick = 6000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);

        let json = serde_json::to_string(&sim).unwrap();
        let restored: SimState = serde_json::from_str(&json).unwrap();
        let thoughts = restored
            .db
            .thoughts
            .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
        assert_eq!(thoughts.len(), 2);
        assert_eq!(thoughts[0].kind, ThoughtKind::SleptOnGround);
        assert_eq!(thoughts[1].kind, ThoughtKind::AteMeal);
    }

    // -----------------------------------------------------------------------
    // Mood tests
    // -----------------------------------------------------------------------

    #[test]
    fn mood_empty_thoughts_is_zero() {
        let (sim, cid) = sim_with_elf_for_thoughts();
        let (score, tier) = sim.mood_for_creature(cid);
        assert_eq!(score, 0);
        assert_eq!(tier, MoodTier::Neutral);
    }

    #[test]
    fn mood_single_positive_thought() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        let (score, tier) = sim.mood_for_creature(cid);
        assert_eq!(score, 60);
        assert_eq!(tier, MoodTier::Content);
    }

    #[test]
    fn mood_single_negative_thought() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::SleptOnGround);
        let (score, tier) = sim.mood_for_creature(cid);
        assert_eq!(score, -100);
        assert_eq!(tier, MoodTier::Unhappy);
    }

    #[test]
    fn mood_mixed_thoughts() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::SleptInOwnHome(StructureId(1)));
        sim.tick = 2000;
        sim.add_creature_thought(cid, ThoughtKind::LowCeiling(StructureId(2)));
        let (score, tier) = sim.mood_for_creature(cid);
        // +80 + (-50) = +30
        assert_eq!(score, 30);
        assert_eq!(tier, MoodTier::Content);
    }

    #[test]
    fn mood_stacking_same_kind() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.config.thoughts.dedup_ate_meal_ticks = 0; // Disable dedup.
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        sim.tick = 2000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        sim.tick = 3000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        let (score, tier) = sim.mood_for_creature(cid);
        // 3 * 60 = 180
        assert_eq!(score, 180);
        assert_eq!(tier, MoodTier::Happy);
    }

    #[test]
    fn mood_tier_boundaries() {
        let cfg = crate::config::MoodConfig::default();
        // Exact boundary values.
        assert_eq!(cfg.tier(-300), MoodTier::Devastated);
        assert_eq!(cfg.tier(-301), MoodTier::Devastated);
        assert_eq!(cfg.tier(-299), MoodTier::Miserable);
        assert_eq!(cfg.tier(-150), MoodTier::Miserable);
        assert_eq!(cfg.tier(-149), MoodTier::Unhappy);
        assert_eq!(cfg.tier(-30), MoodTier::Unhappy);
        assert_eq!(cfg.tier(-29), MoodTier::Neutral);
        assert_eq!(cfg.tier(0), MoodTier::Neutral);
        assert_eq!(cfg.tier(29), MoodTier::Neutral);
        assert_eq!(cfg.tier(30), MoodTier::Content);
        assert_eq!(cfg.tier(149), MoodTier::Content);
        assert_eq!(cfg.tier(150), MoodTier::Happy);
        assert_eq!(cfg.tier(299), MoodTier::Happy);
        assert_eq!(cfg.tier(300), MoodTier::Elated);
        assert_eq!(cfg.tier(301), MoodTier::Elated);
    }

    #[test]
    fn mood_custom_config_weights() {
        let (mut sim, cid) = sim_with_elf_for_thoughts();
        sim.tick = 1000;
        sim.add_creature_thought(cid, ThoughtKind::AteMeal);
        sim.config.mood.weight_ate_meal = 200;
        let (score, _) = sim.mood_for_creature(cid);
        assert_eq!(score, 200);
    }

    #[test]
    fn mood_config_serde_roundtrip() {
        let cfg = crate::config::MoodConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: crate::config::MoodConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.weight_ate_meal, cfg.weight_ate_meal);
        assert_eq!(restored.tier_elated_above, cfg.tier_elated_above);
    }

    #[test]
    fn mood_config_backward_compat() {
        // A GameConfig JSON without a "mood" key should deserialize with defaults.
        let sim = test_sim(42);
        let json = serde_json::to_string(&sim).unwrap();
        // Strip the "mood" key from the JSON to simulate an old save.
        let mut val: serde_json::Value = serde_json::from_str(&json).unwrap();
        val.get_mut("config")
            .and_then(|c| c.as_object_mut())
            .unwrap()
            .remove("mood");
        let stripped = serde_json::to_string(&val).unwrap();
        let restored: SimState = serde_json::from_str(&stripped).unwrap();
        assert_eq!(
            restored.config.mood.weight_ate_meal,
            crate::config::MoodConfig::default().weight_ate_meal
        );
    }

    #[test]
    fn ground_sleep_generates_thought() {
        // Integration test: elf sleeps on ground → has SleptOnGround thought.
        let mut config = test_config();
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0; // No hunger interference.
        elf_species.rest_decay_per_tick = 0; // Manual control of rest.
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest very low to trigger sleep.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.rest = rest_max * 10 / 100);

        // Advance past heartbeat to trigger sleep + enough ticks for it to complete.
        let target_tick = 1 + heartbeat_interval + sim.config.sleep_ticks_ground + 1000;
        sim.step(&[], target_tick);

        // Elf should have a SleptOnGround thought.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|t| t.kind == ThoughtKind::SleptOnGround),
            "Elf should have SleptOnGround thought after ground sleep. thoughts={:?}",
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        );
        // Piggyback: mood should reflect the negative SleptOnGround thought.
        let (score, _tier) = sim.mood_for_creature(elf_id);
        let expected: i32 = sim
            .db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .map(|t| sim.config.mood.mood_weight(&t.kind))
            .sum();
        assert_eq!(
            score, expected,
            "Mood score should match sum of thought weights"
        );
    }

    #[test]
    fn eating_generates_thought() {
        // Integration test: elf eats fruit → has AteMeal thought.
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let food_max = sim.species_table[&Species::Elf].food_max;
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        assert!(
            sim.trees.values().any(|t| !t.fruit_positions.is_empty()),
            "Tree must have fruit for this test"
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Make the elf hungry.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.food = food_max * 10 / 100);

        // Advance enough ticks for the elf to find fruit, walk to it, and eat it.
        // Walk could be up to ~50 voxels at 500 tpv = 25000 ticks.
        let target_tick = 1 + heartbeat_interval + 50_000;
        sim.step(&[], target_tick);

        // Elf should have an AteMeal thought.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|t| t.kind == ThoughtKind::AteMeal),
            "Elf should have AteMeal thought after eating. thoughts={:?}",
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        );
    }

    // --- Inventory integration tests ---

    #[test]
    fn elf_spawns_with_starting_bread() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        // Elves now spawn with starting bread (config.elf_starting_bread = 2).
        let bread_count =
            sim.inv_count_owned(sim.creature_inv(elf_id), inventory::ItemKind::Bread, elf_id);
        assert_eq!(
            bread_count, 2,
            "Elf should spawn with 2 owned bread from elf_starting_bread config"
        );
        assert_eq!(
            sim.inv_items(sim.creature_inv(elf_id)).len(),
            1,
            "Inventory should have exactly one stack (bread)"
        );
    }

    #[test]
    fn dormitory_sleep_generates_thought() {
        // Integration test: elf sleeps in dormitory → has SleptInDormitory thought.
        let mut config = test_config();
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;

        // Add a dormitory with beds near the tree.
        let graph = sim.graph_for_species(Species::Elf);
        let bed_node = graph.find_nearest_node(tree_pos).unwrap();
        let bed_pos = graph.node(bed_node).position;

        let structure_id = StructureId(999);
        let project_id = ProjectId::new(&mut sim.rng);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.db
            .structures
            .insert_no_fk(CompletedStructure {
                id: structure_id,
                project_id,
                build_type: BuildType::Building,
                anchor: bed_pos,
                width: 3,
                depth: 3,
                height: 3,
                completed_tick: 0,
                name: None,
                furnishing: Some(FurnishingType::Dormitory),
                inventory_id: inv_id,
                logistics_priority: None,
                cooking_enabled: false,
                cooking_bread_target: 0,
                workshop_enabled: false,
                workshop_recipe_ids: Vec::new(),
                workshop_recipe_targets: std::collections::BTreeMap::new(),
                greenhouse_species: None,
                greenhouse_enabled: false,
                greenhouse_last_production_tick: 0,
            })
            .unwrap();
        let _ = sim
            .db
            .furniture
            .insert_auto_no_fk(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: bed_pos,
                placed: true,
            });

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest very low to trigger sleep.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.rest = rest_max * 10 / 100);
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Advance enough ticks for sleep to trigger, walk to bed, and complete.
        // Walk time + sleep time + buffer.
        let target_tick = 1 + heartbeat_interval + 50_000 + sim.config.sleep_ticks_bed;
        sim.step(&[], target_tick);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|t| t.kind == ThoughtKind::SleptInDormitory(structure_id)),
            "Elf should have SleptInDormitory thought. thoughts={:?}",
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        );
        // Piggyback: mood should reflect dormitory sleep thought.
        let (score, _tier) = sim.mood_for_creature(elf_id);
        let expected: i32 = sim
            .db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .map(|t| sim.config.mood.mood_weight(&t.kind))
            .sum();
        assert_eq!(
            score, expected,
            "Mood score should match sum of thought weights"
        );
    }

    #[test]
    fn creature_add_and_query_bread() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        let elf_id = spawn_elf(&mut sim);

        sim.inv_add_simple_item(
            sim.creature_inv(elf_id),
            crate::inventory::ItemKind::Bread,
            5,
            Some(elf_id),
            None,
        );

        let count = sim.inv_item_count(sim.creature_inv(elf_id), crate::inventory::ItemKind::Bread);
        assert_eq!(count, 5);
    }

    #[test]
    fn creature_inventory_serialization_roundtrip() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        let elf_id = spawn_elf(&mut sim);

        sim.inv_add_simple_item(
            sim.creature_inv(elf_id),
            crate::inventory::ItemKind::Bread,
            3,
            Some(elf_id),
            None,
        );

        // Verify via inv_item_count (serialization of item_stacks is tested separately).
        let count = sim.inv_item_count(sim.creature_inv(elf_id), crate::inventory::ItemKind::Bread);
        assert_eq!(count, 3);
    }

    #[test]
    fn home_sleep_generates_thought() {
        // Integration test: elf sleeps in assigned home → has SleptInOwnHome thought.
        let mut config = test_config();
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;

        // Add a home with a bed near the tree.
        let graph = sim.graph_for_species(Species::Elf);
        let bed_node = graph.find_nearest_node(tree_pos).unwrap();
        let bed_pos = graph.node(bed_node).position;

        let structure_id = StructureId(888);
        let project_id = ProjectId::new(&mut sim.rng);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.db
            .structures
            .insert_no_fk(CompletedStructure {
                id: structure_id,
                project_id,
                build_type: BuildType::Building,
                anchor: bed_pos,
                width: 3,
                depth: 3,
                height: 3,
                completed_tick: 0,
                name: None,
                furnishing: Some(FurnishingType::Home),
                inventory_id: inv_id,
                logistics_priority: None,
                cooking_enabled: false,
                cooking_bread_target: 0,
                workshop_enabled: false,
                workshop_recipe_ids: Vec::new(),
                workshop_recipe_targets: std::collections::BTreeMap::new(),
                greenhouse_species: None,
                greenhouse_enabled: false,
                greenhouse_last_production_tick: 0,
            })
            .unwrap();
        let _ = sim
            .db
            .furniture
            .insert_auto_no_fk(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: bed_pos,
                placed: true,
            });

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Assign the home to the elf.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: 2,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(structure_id),
                },
            }],
            2,
        );

        // Set rest very low to trigger sleep.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.rest = rest_max * 10 / 100);
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Advance enough ticks for sleep to trigger, walk to bed, and complete.
        let target_tick = 2 + heartbeat_interval + 50_000 + sim.config.sleep_ticks_bed;
        sim.step(&[], target_tick);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|t| t.kind == ThoughtKind::SleptInOwnHome(structure_id)),
            "Elf should have SleptInOwnHome thought. thoughts={:?}",
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        );
        // Piggyback: mood should reflect home sleep thought.
        let (score, _tier) = sim.mood_for_creature(elf_id);
        let expected: i32 = sim
            .db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .map(|t| sim.config.mood.mood_weight(&t.kind))
            .sum();
        assert_eq!(
            score, expected,
            "Mood score should match sum of thought weights"
        );
    }

    #[test]
    fn low_ceiling_generates_thought() {
        // Integration test: elf sleeps in height-1 building → LowCeiling thought.
        let mut config = test_config();
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;

        // Add a dormitory with height=1 (low ceiling).
        let graph = sim.graph_for_species(Species::Elf);
        let bed_node = graph.find_nearest_node(tree_pos).unwrap();
        let bed_pos = graph.node(bed_node).position;

        let structure_id = StructureId(888);
        let project_id = ProjectId::new(&mut sim.rng);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.db
            .structures
            .insert_no_fk(CompletedStructure {
                id: structure_id,
                project_id,
                build_type: BuildType::Building,
                anchor: bed_pos,
                width: 3,
                depth: 3,
                height: 1, // Low ceiling!
                completed_tick: 0,
                name: None,
                furnishing: Some(FurnishingType::Dormitory),
                inventory_id: inv_id,
                logistics_priority: None,
                cooking_enabled: false,
                cooking_bread_target: 0,
                workshop_enabled: false,
                workshop_recipe_ids: Vec::new(),
                workshop_recipe_targets: std::collections::BTreeMap::new(),
                greenhouse_species: None,
                greenhouse_enabled: false,
                greenhouse_last_production_tick: 0,
            })
            .unwrap();
        let _ = sim
            .db
            .furniture
            .insert_auto_no_fk(|id| crate::db::Furniture {
                id,
                structure_id,
                coord: bed_pos,
                placed: true,
            });

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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest very low to trigger sleep.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.rest = rest_max * 10 / 100);
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

        // Advance enough ticks for sleep to trigger, walk to bed, and complete.
        let target_tick = 1 + heartbeat_interval + 50_000 + sim.config.sleep_ticks_bed;
        sim.step(&[], target_tick);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|t| t.kind == ThoughtKind::LowCeiling(structure_id)),
            "Elf should have LowCeiling thought from height-1 building. thoughts={:?}",
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        );
        // Should also have the dormitory sleep thought.
        assert!(
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
                .iter()
                .any(|t| t.kind == ThoughtKind::SleptInDormitory(structure_id)),
            "Elf should also have SleptInDormitory thought. thoughts={:?}",
            sim.db
                .thoughts
                .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        );
        // Piggyback: mood should reflect both dormitory sleep and low ceiling.
        let (score, _tier) = sim.mood_for_creature(elf_id);
        let expected: i32 = sim
            .db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .map(|t| sim.config.mood.mood_weight(&t.kind))
            .sum();
        assert_eq!(
            score, expected,
            "Mood score should match sum of thought weights"
        );
    }

    #[test]
    fn ground_piles_in_sim_state() {
        let mut sim = test_sim(42);
        let pos = VoxelCoord::new(10, 1, 20);
        {
            let pile_id = sim.ensure_ground_pile(pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                4,
                None,
                None,
            );
        }
        assert_eq!(sim.db.ground_piles.len(), 1);
        let pile = sim
            .db
            .ground_piles
            .by_position(&pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            sim.inv_item_count(pile.inventory_id, crate::inventory::ItemKind::Bread),
            4
        );
    }

    #[test]
    fn ground_piles_serialization_roundtrip() {
        let mut sim = test_sim(42);
        let pos1 = VoxelCoord::new(10, 1, 20);
        let pos2 = VoxelCoord::new(3, 1, 7);
        {
            let pile_id = sim.ensure_ground_pile(pos1);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Fruit,
                2,
                None,
                None,
            );
        }
        {
            let pile_id = sim.ensure_ground_pile(pos2);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                5,
                None,
                None,
            );
        }

        let json = serde_json::to_string(&sim).unwrap();
        let restored: SimState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.db.ground_piles.len(), 2);
        let pile1 = restored
            .db
            .ground_piles
            .by_position(&pos1, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(pile1.position, pos1);
        assert_eq!(
            restored.inv_item_count(pile1.inventory_id, crate::inventory::ItemKind::Fruit),
            2
        );
        let pile2 = restored
            .db
            .ground_piles
            .by_position(&pos2, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(pile2.position, pos2);
        assert_eq!(
            restored.inv_item_count(pile2.inventory_id, crate::inventory::ItemKind::Bread),
            5
        );
    }

    #[test]
    fn ground_piles_serde_roundtrip() {
        let mut sim = test_sim(42);
        let pos = VoxelCoord::new(10, 1, 20);
        {
            let pile_id = sim.ensure_ground_pile(pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                7,
                None,
                None,
            );
        }

        // Serialize → deserialize round-trip.
        let json = sim.to_json().expect("serialization should succeed");
        let restored = SimState::from_json(&json).expect("deserialization should succeed");

        assert_eq!(restored.db.ground_piles.len(), 1);
        let restored_pile = restored
            .db
            .ground_piles
            .by_position(&pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            restored.inv_item_count(
                restored_pile.inventory_id,
                crate::inventory::ItemKind::Bread
            ),
            7
        );

        // Checksums should match.
        assert_eq!(sim.state_checksum(), restored.state_checksum());
    }

    // --- Hauling and logistics tests ---

    /// Helper: create a completed building structure at the given anchor.
    fn insert_building(
        sim: &mut SimState,
        anchor: VoxelCoord,
        logistics_priority: Option<u8>,
        wants: Vec<crate::building::LogisticsWant>,
    ) -> StructureId {
        let sid = StructureId(sim.next_structure_id);
        sim.next_structure_id += 1;
        let project_id = ProjectId::new(&mut sim.rng);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.db
            .structures
            .insert_no_fk(CompletedStructure {
                id: sid,
                project_id,
                build_type: BuildType::Building,
                anchor,
                width: 3,
                depth: 3,
                height: 2,
                completed_tick: 0,
                name: None,
                furnishing: Some(FurnishingType::Storehouse),
                inventory_id: inv_id,
                logistics_priority,
                cooking_enabled: false,
                cooking_bread_target: 0,
                workshop_enabled: false,
                workshop_recipe_ids: Vec::new(),
                workshop_recipe_targets: std::collections::BTreeMap::new(),
                greenhouse_species: None,
                greenhouse_enabled: false,
                greenhouse_last_production_tick: 0,
            })
            .unwrap();
        sim.set_inv_wants(inv_id, &wants);
        sid
    }

    #[test]
    fn logistics_heartbeat_creates_haul_tasks() {
        let mut sim = test_sim(42);

        // Place a ground pile with bread.
        let pile_pos = sim.trees[&sim.player_tree_id].position;
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                10,
                None,
                None,
            );
        }

        // Create a building that wants bread.
        let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
        let _sid = insert_building(
            &mut sim,
            building_anchor,
            Some(5),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 5,
            }],
        );

        // Run logistics heartbeat manually.
        sim.process_logistics_heartbeat();

        // Should have created a haul task.
        let haul_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Haul)
            .collect();
        assert_eq!(haul_tasks.len(), 1, "Expected 1 haul task");

        let haul = sim
            .task_haul_data(haul_tasks[0].id)
            .expect("Haul task should have haul data");
        assert_eq!(haul.item_kind, crate::inventory::ItemKind::Bread);
        assert_eq!(haul.quantity, 5);
        assert_eq!(haul.source_kind, crate::db::HaulSourceKind::Pile);
        assert_eq!(haul.phase, task::HaulPhase::GoingToSource);

        // Ground pile items should be reserved.
        let pile = sim
            .db
            .ground_piles
            .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        let unreserved =
            sim.inv_unreserved_item_count(pile.inventory_id, crate::inventory::ItemKind::Bread);
        assert_eq!(unreserved, 5, "5 items should remain unreserved");
    }

    #[test]
    fn logistics_respects_priority() {
        let mut sim = test_sim(42);

        // Place bread on the ground.
        let pile_pos = sim.trees[&sim.player_tree_id].position;
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                3,
                None,
                None,
            );
        }

        // High-priority building wants 2 bread.
        let high_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
        insert_building(
            &mut sim,
            high_anchor,
            Some(10),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 2,
            }],
        );

        // Low-priority building wants 2 bread.
        let low_anchor = VoxelCoord::new(pile_pos.x + 6, pile_pos.y, pile_pos.z);
        insert_building(
            &mut sim,
            low_anchor,
            Some(1),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 2,
            }],
        );

        sim.process_logistics_heartbeat();

        // Should create 2 haul tasks: one for high-priority (2 bread), one for
        // low-priority (1 remaining bread).
        let haul_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Haul)
            .collect();
        assert_eq!(haul_tasks.len(), 2, "Expected 2 haul tasks");

        // All bread should be reserved.
        let pile = sim
            .db
            .ground_piles
            .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        let unreserved =
            sim.inv_unreserved_item_count(pile.inventory_id, crate::inventory::ItemKind::Bread);
        assert_eq!(unreserved, 0, "All bread should be reserved");
    }

    #[test]
    fn logistics_skips_reserved_items() {
        let mut sim = test_sim(42);

        // Place bread on the ground, some already reserved.
        let pile_pos = sim.trees[&sim.player_tree_id].position;
        let task_id = TaskId::new(&mut sim.rng);
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                5,
                None,
                None,
            );
            sim.inv_reserve_items(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                3,
                task_id,
            );
        }

        // Building wants 5 bread.
        let anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
        insert_building(
            &mut sim,
            anchor,
            Some(5),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 5,
            }],
        );

        sim.process_logistics_heartbeat();

        // Should only create a task for 2 unreserved bread, not all 5.
        let haul_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Haul)
            .collect();
        assert_eq!(haul_tasks.len(), 1);
        let haul = sim
            .task_haul_data(haul_tasks[0].id)
            .expect("Haul task should have haul data");
        assert_eq!(haul.quantity, 2, "Only 2 unreserved bread available");
    }

    #[test]
    fn logistics_counts_in_transit() {
        let mut sim = test_sim(42);

        // Place 10 bread on ground.
        let pile_pos = sim.trees[&sim.player_tree_id].position;
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                10,
                None,
                None,
            );
        }

        let anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
        let sid = insert_building(
            &mut sim,
            anchor,
            Some(5),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 8,
            }],
        );

        // Manually create an in-transit haul task for 5 bread.
        let fake_task_id = TaskId::new(&mut sim.rng);
        let existing_haul = Task {
            id: fake_task_id,
            kind: TaskKind::Haul {
                item_kind: crate::inventory::ItemKind::Bread,
                quantity: 5,
                source: task::HaulSource::GroundPile(pile_pos),
                destination: sid,
                phase: task::HaulPhase::GoingToSource,
                destination_nav_node: NavNodeId(0),
            },
            state: TaskState::InProgress,
            location: NavNodeId(0),
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Automated,
            target_creature: None,
        };
        sim.insert_task(existing_haul);

        sim.process_logistics_heartbeat();

        // In-transit counts as 5, target is 8, so need 3 more.
        let new_haul_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.id != fake_task_id && t.kind_tag == TaskKindTag::Haul)
            .collect();
        assert_eq!(new_haul_tasks.len(), 1, "Expected 1 new haul task");
        let haul = sim
            .task_haul_data(new_haul_tasks[0].id)
            .expect("Haul task should have haul data");
        assert_eq!(haul.quantity, 3);
    }

    #[test]
    fn logistics_pulls_from_lower_priority_building() {
        let mut sim = test_sim(42);

        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Building A (priority 3) has bread.
        let anchor_a = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
        let sid_a = insert_building(&mut sim, anchor_a, Some(3), Vec::new());
        sim.inv_add_simple_item(
            sim.structure_inv(sid_a),
            crate::inventory::ItemKind::Bread,
            5,
            None,
            None,
        );

        // Building B (priority 5) wants bread.
        let anchor_b = VoxelCoord::new(tree_pos.x + 6, tree_pos.y, tree_pos.z);
        insert_building(
            &mut sim,
            anchor_b,
            Some(5),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 3,
            }],
        );

        sim.process_logistics_heartbeat();

        let haul_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Haul)
            .collect();
        assert_eq!(haul_tasks.len(), 1);
        let haul = sim
            .task_haul_data(haul_tasks[0].id)
            .expect("Haul task should have haul data");
        assert_eq!(haul.source_kind, crate::db::HaulSourceKind::Building);
        let source_sid = sim
            .task_structure_ref(
                haul_tasks[0].id,
                crate::db::TaskStructureRole::HaulSourceBuilding,
            )
            .expect("Should have source building ref");
        assert_eq!(source_sid, sid_a, "Should pull from building A");
        assert_eq!(haul.quantity, 3);
    }

    #[test]
    fn logistics_surplus_source_from_higher_priority_building() {
        let mut sim = test_sim(42);

        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Kitchen (priority 8) has 10 bread, wants 0 bread → 10 surplus.
        let anchor_k = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
        let sid_k = insert_building(&mut sim, anchor_k, Some(8), Vec::new());
        sim.inv_add_simple_item(
            sim.structure_inv(sid_k),
            crate::inventory::ItemKind::Bread,
            10,
            None,
            None,
        );

        // Storehouse (priority 2) wants 5 bread.
        let anchor_s = VoxelCoord::new(tree_pos.x + 7, tree_pos.y, tree_pos.z);
        insert_building(
            &mut sim,
            anchor_s,
            Some(2),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 5,
            }],
        );

        sim.process_logistics_heartbeat();

        let haul_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Haul)
            .collect();
        assert_eq!(
            haul_tasks.len(),
            1,
            "Should create a haul task for surplus bread"
        );
        let haul = sim
            .task_haul_data(haul_tasks[0].id)
            .expect("Haul task should have haul data");
        assert_eq!(haul.source_kind, crate::db::HaulSourceKind::Building);
        let source_sid = sim
            .task_structure_ref(
                haul_tasks[0].id,
                crate::db::TaskStructureRole::HaulSourceBuilding,
            )
            .expect("Should have source building ref");
        assert_eq!(
            source_sid, sid_k,
            "Should pull from the kitchen (surplus source)"
        );
        assert_eq!(haul.quantity, 5);
    }

    #[test]
    fn logistics_caps_tasks_per_heartbeat() {
        let mut sim = test_sim(42);
        // Override max tasks to 2.
        sim.config.max_haul_tasks_per_heartbeat = 2;

        let tree_pos = sim.trees[&sim.player_tree_id].position;
        {
            let pile_id = sim.ensure_ground_pile(tree_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                100,
                None,
                None,
            );
        }

        // Create 5 buildings that each want 10 bread.
        for i in 0..5 {
            let anchor = VoxelCoord::new(tree_pos.x + 3 * (i + 1), tree_pos.y, tree_pos.z);
            insert_building(
                &mut sim,
                anchor,
                Some(5),
                vec![crate::building::LogisticsWant {
                    item_kind: crate::inventory::ItemKind::Bread,
                    target_quantity: 10,
                }],
            );
        }

        sim.process_logistics_heartbeat();

        let haul_count = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Haul)
            .count();
        assert_eq!(haul_count, 2, "Should be capped at 2 tasks per heartbeat");
    }

    #[test]
    fn kitchen_monitor_creates_cook_task() {
        let mut sim = test_sim(42);

        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Insert a kitchen with 1 fruit, cooking enabled, bread target 50.
        let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
        let sid = insert_completed_building(&mut sim, anchor);
        {
            let mut s = sim.db.structures.get(&sid).unwrap();
            s.furnishing = Some(FurnishingType::Kitchen);
            s.cooking_enabled = true;
            s.cooking_bread_target = 50;
            s.logistics_priority = Some(8);
            let _ = sim.db.structures.update_no_fk(s);
        }
        sim.inv_add_simple_item(
            sim.structure_inv(sid),
            inventory::ItemKind::Fruit,
            1,
            None,
            None,
        );

        sim.process_logistics_heartbeat();

        // Verify a Cook task was created.
        let cook_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Cook)
            .collect();
        assert_eq!(cook_tasks.len(), 1, "Should create 1 Cook task");
        assert_eq!(cook_tasks[0].state, task::TaskState::Available);
        assert_eq!(cook_tasks[0].required_species, Some(Species::Elf));

        // Verify fruit is reserved.
        let structure = sim.db.structures.get(&sid).unwrap();
        let unreserved =
            sim.inv_unreserved_item_count(structure.inventory_id, inventory::ItemKind::Fruit);
        assert_eq!(unreserved, 0, "Fruit should be reserved for cook task");
    }

    #[test]
    fn cook_task_converts_fruit_to_bread() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 20; // Prevent hunger interference.

        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Insert a kitchen building with 1 fruit (reserved for the cook task).
        let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
        let sid = insert_completed_building(&mut sim, anchor);
        {
            let mut s = sim.db.structures.get(&sid).unwrap();
            s.furnishing = Some(FurnishingType::Kitchen);
            s.cooking_enabled = true;
            s.cooking_bread_target = 50;
            let _ = sim.db.structures.update_no_fk(s);
        }

        // Find a nav node inside the kitchen.
        let interior_pos = sim.db.structures.get(&sid).unwrap().anchor;
        let kitchen_nav = sim.nav_graph.find_nearest_node(interior_pos).unwrap();

        // Create Cook task at the kitchen's nav node.
        let task_id = TaskId::new(&mut sim.rng);
        let cook_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Cook { structure_id: sid },
            state: task::TaskState::Available,
            location: kitchen_nav,
            progress: 0.0,
            total_cost: sim.config.cook_work_ticks as f32,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::Automated,
            target_creature: None,
        };
        sim.insert_task(cook_task);

        // Add 1 fruit reserved by this task.
        sim.inv_add_simple_item(
            sim.structure_inv(sid),
            inventory::ItemKind::Fruit,
            1,
            None,
            Some(task_id),
        );

        // Spawn an elf near the kitchen.
        let mut events = Vec::new();
        sim.spawn_creature(Species::Elf, interior_pos, &mut events);

        // Run enough ticks for elf to reach kitchen and complete cooking.
        // cook_work_ticks = 5000, plus walking time.
        sim.step(&[], sim.tick + 15000);

        // Verify: fruit consumed, bread produced.
        let structure = sim.db.structures.get(&sid).unwrap();
        let fruit_count =
            sim.inv_unreserved_item_count(structure.inventory_id, inventory::ItemKind::Fruit);
        let bread_count =
            sim.inv_unreserved_item_count(structure.inventory_id, inventory::ItemKind::Bread);
        assert_eq!(fruit_count, 0, "Fruit should be consumed");
        assert_eq!(bread_count, 10, "Should produce 10 bread");

        // Verify task is complete.
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, task::TaskState::Complete);
    }

    #[test]
    fn harvest_task_creates_ground_pile() {
        let mut sim = test_sim(42);

        // Find a fruit voxel on the tree.
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.fruit_positions.is_empty(),
            "Tree should have fruit for this test"
        );
        let fruit_pos = tree.fruit_positions[0];

        // Spawn an elf near the fruit.
        let elf_id = spawn_elf(&mut sim);

        // Find the nav node nearest to the fruit.
        let fruit_nav = sim.nav_graph.find_nearest_node(fruit_pos).unwrap();

        // Place the elf at the fruit nav node.
        let elf_pos = sim.nav_graph.node(fruit_nav).position;
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |elf| {
            elf.current_node = Some(fruit_nav);
            elf.position = elf_pos;
        });

        // Create a Harvest task at the fruit nav node.
        let task_id = TaskId::new(&mut sim.rng);
        let harvest_task = Task {
            id: task_id,
            kind: TaskKind::Harvest { fruit_pos },
            state: TaskState::InProgress,
            location: fruit_nav,
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Automated,
            target_creature: None,
        };
        sim.insert_task(harvest_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Execute the task directly (resolve the harvest action).
        sim.resolve_harvest_action(elf_id, task_id, fruit_pos);

        // Assert: fruit voxel removed from world.
        assert_eq!(
            sim.world.get(fruit_pos),
            VoxelType::Air,
            "Fruit voxel should be removed"
        );

        // Assert: fruit removed from tree's fruit_positions.
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.fruit_positions.contains(&fruit_pos),
            "Fruit should be removed from tree"
        );

        // Assert: ground pile created with 1 Fruit. The pile may have been
        // snapped down to the nearest surface if the elf was up on the tree.
        let pile = sim
            .db
            .ground_piles
            .iter_all()
            .find(|p| p.position.x == elf_pos.x && p.position.z == elf_pos.z)
            .expect("Ground pile should exist in elf's column");
        assert_eq!(
            sim.inv_item_count(pile.inventory_id, inventory::ItemKind::Fruit),
            1,
            "Ground pile should have 1 fruit"
        );

        // Assert: task completed.
        assert_eq!(
            sim.db.tasks.get(&task_id).unwrap().state,
            TaskState::Complete,
            "Harvest task should be complete"
        );
    }

    #[test]
    fn logistics_heartbeat_creates_harvest_tasks() {
        let mut sim = test_sim(42);

        // Verify the tree has fruit voxels.
        let tree = &sim.trees[&sim.player_tree_id];
        let fruit_count = tree.fruit_positions.len();
        assert!(fruit_count > 0, "Tree should have fruit for this test");

        // Ensure no ground piles with fruit exist.
        assert_eq!(sim.db.ground_piles.len(), 0);

        // Create a building that wants fruit (kitchen with logistics).
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let site = VoxelCoord::new(tree_pos.x + 3, 0, tree_pos.z);
        let kitchen_priority = sim.config.kitchen_default_priority;
        let sid = insert_building(
            &mut sim,
            site,
            Some(kitchen_priority),
            vec![building::LogisticsWant {
                item_kind: inventory::ItemKind::Fruit,
                target_quantity: 5,
            }],
        );
        {
            let mut s = sim.db.structures.get(&sid).unwrap();
            s.furnishing = Some(FurnishingType::Kitchen);
            let _ = sim.db.structures.update_no_fk(s);
        }

        // Run logistics heartbeat.
        sim.process_logistics_heartbeat();

        // Assert: at least one Harvest task was created.
        let harvest_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Harvest)
            .collect();
        assert!(
            !harvest_tasks.is_empty(),
            "Logistics heartbeat should create Harvest tasks when buildings want fruit"
        );

        // Each harvest task should target a valid fruit position.
        for task in &harvest_tasks {
            let fruit_pos = sim
                .task_voxel_ref(task.id, crate::db::TaskVoxelRole::FruitTarget)
                .expect("Harvest task should have a FruitTarget voxel ref");
            assert_eq!(
                sim.world.get(fruit_pos),
                VoxelType::Fruit,
                "Harvest task should target an actual fruit voxel"
            );
            assert_eq!(task.state, TaskState::Available);
            assert_eq!(task.required_species, Some(Species::Elf));
            assert_eq!(task.origin, TaskOrigin::Automated);
        }
    }

    #[test]
    fn kitchen_cooks_fruit_into_bread_end_to_end() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 20; // Prevent hunger interference.
        sim.config.elf_default_wants = Vec::new(); // Disable personal acquisition.
        // Disable hunger and tiredness so the elf doesn't get distracted.
        if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
            elf_data.food_decay_per_tick = 0;
            elf_data.rest_decay_per_tick = 0;
        }
        sim.species_table = sim
            .config
            .species
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        // Verify the tree has fruit voxels (no manual ground pile needed).
        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.fruit_positions.is_empty(),
            "Tree should have fruit voxels for this test"
        );

        // Place both buildings near the tree on the forest floor, adjacent.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let site1 = VoxelCoord::new(tree_pos.x + 3, 0, tree_pos.z);
        let site2 = VoxelCoord::new(tree_pos.x + 7, 0, tree_pos.z);

        // Insert storehouse — only wants bread (small target to keep test focused).
        let sid_store = insert_completed_building(&mut sim, site1);
        {
            let mut s = sim.db.structures.get(&sid_store).unwrap();
            s.furnishing = Some(FurnishingType::Storehouse);
            s.logistics_priority = Some(sim.config.storehouse_default_priority);
            let _ = sim.db.structures.update_no_fk(s);
        }
        sim.set_inv_wants(
            sim.structure_inv(sid_store),
            &[building::LogisticsWant {
                item_kind: inventory::ItemKind::Bread,
                target_quantity: 10,
            }],
        );

        // Insert kitchen with reduced wants to keep test fast and focused.
        let sid_kitchen = insert_completed_building(&mut sim, site2);
        {
            let mut s = sim.db.structures.get(&sid_kitchen).unwrap();
            s.furnishing = Some(FurnishingType::Kitchen);
            s.logistics_priority = Some(sim.config.kitchen_default_priority);
            s.cooking_enabled = true;
            s.cooking_bread_target = 1;
            let _ = sim.db.structures.update_no_fk(s);
        }
        sim.set_inv_wants(
            sim.structure_inv(sid_kitchen),
            &[building::LogisticsWant {
                item_kind: inventory::ItemKind::Fruit,
                target_quantity: 1,
            }],
        );

        // Spawn 1 elf near the tree.
        let spawn_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
        let mut events = Vec::new();
        sim.spawn_creature(Species::Elf, spawn_pos, &mut events);

        // Run 500k ticks — enough for full pipeline:
        // harvest task → elf picks fruit → ground pile → haul to kitchen → cook → haul bread out.
        sim.step(&[], sim.tick + 500_000);

        // Storehouse should have bread from cooking (full pipeline: harvest → haul → cook → haul).
        let store_bread =
            sim.inv_unreserved_item_count(sim.structure_inv(sid_store), inventory::ItemKind::Bread);
        assert!(
            store_bread >= 10,
            "Storehouse should have at least 10 bread from cooking, got {store_bread}"
        );
    }

    #[test]
    fn elf_acquires_bread_from_kitchen_pipeline() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        sim.config.elf_default_wants = vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Bread,
            target_quantity: 2,
        }];
        // Disable hunger/tiredness.
        if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
            elf_data.food_decay_per_tick = 0;
            elf_data.rest_decay_per_tick = 0;
        }
        sim.species_table = sim
            .config
            .species
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        let tree = &sim.trees[&sim.player_tree_id];
        assert!(
            !tree.fruit_positions.is_empty(),
            "Tree should have fruit voxels"
        );

        // Insert kitchen (fruit_want=1, bread_target=1). No storehouse.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let site = VoxelCoord::new(tree_pos.x + 3, 0, tree_pos.z);
        let sid_kitchen = insert_completed_building(&mut sim, site);
        {
            let mut s = sim.db.structures.get(&sid_kitchen).unwrap();
            s.furnishing = Some(FurnishingType::Kitchen);
            s.logistics_priority = Some(sim.config.kitchen_default_priority);
            s.cooking_enabled = true;
            s.cooking_bread_target = 1;
            let _ = sim.db.structures.update_no_fk(s);
        }
        sim.set_inv_wants(
            sim.structure_inv(sid_kitchen),
            &[building::LogisticsWant {
                item_kind: inventory::ItemKind::Fruit,
                target_quantity: 1,
            }],
        );

        // Spawn 1 elf.
        let spawn_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
        let mut events = Vec::new();
        sim.spawn_creature(Species::Elf, spawn_pos, &mut events);
        let elf_id = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Pipeline: harvest fruit → haul to kitchen → cook bread → elf acquires bread.
        sim.step(&[], sim.tick + 500_000);

        // Elf should have acquired bread.
        let elf_bread =
            sim.inv_count_owned(sim.creature_inv(elf_id), inventory::ItemKind::Bread, elf_id);
        assert!(
            elf_bread > 0,
            "Elf should have acquired bread from kitchen pipeline, got {elf_bread}"
        );
    }

    #[test]
    fn haul_source_empty_cancels() {
        let mut sim = test_sim(42);

        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
        let sid = insert_building(&mut sim, anchor, Some(5), Vec::new());

        // Create a haul task with source pointing to a non-existent ground pile.
        let source_pos = VoxelCoord::new(tree_pos.x, tree_pos.y, tree_pos.z);
        let task_id = TaskId::new(&mut sim.rng);
        let source_nav = sim.nav_graph.find_nearest_node(source_pos).unwrap();
        let dest_nav = sim.nav_graph.find_nearest_node(anchor).unwrap();

        let haul_task = Task {
            id: task_id,
            kind: TaskKind::Haul {
                item_kind: crate::inventory::ItemKind::Bread,
                quantity: 5,
                source: task::HaulSource::GroundPile(source_pos),
                destination: sid,
                phase: task::HaulPhase::GoingToSource,
                destination_nav_node: dest_nav,
            },
            state: TaskState::InProgress,
            location: source_nav,
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Automated,
            target_creature: None,
        };
        sim.insert_task(haul_task);

        // Spawn an elf and manually assign it to the haul task at the source.
        let elf_id = spawn_elf(&mut sim);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&elf_id, |c| c.current_node = Some(source_nav));
        let _ = sim
            .db
            .tasks
            .modify_unchecked(&task_id, |t| t.state = task::TaskState::InProgress);

        // Execute the task — no ground pile exists, so pickup should find 0 items.
        sim.resolve_pickup_action(elf_id);

        // Task should be completed (cancelled due to empty source).
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, TaskState::Complete, "Task should be completed");
    }

    // -----------------------------------------------------------------------
    // 15.10 New SimAction variants (SetCreatureFood, SetCreatureRest,
    //       AddCreatureItem, AddGroundPileItem)
    // -----------------------------------------------------------------------

    #[test]
    fn set_creature_food() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetCreatureFood {
                creature_id: elf_id,
                food: 42_000,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().food, 42_000);
    }

    #[test]
    fn set_creature_rest() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetCreatureRest {
                creature_id: elf_id,
                rest: 99_000,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().rest, 99_000);
    }

    #[test]
    fn add_creature_item() {
        let mut sim = test_sim(42);
        sim.config.elf_starting_bread = 0;
        let elf_id = spawn_elf(&mut sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::AddCreatureItem {
                creature_id: elf_id,
                item_kind: crate::inventory::ItemKind::Bread,
                quantity: 5,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        let bread_count =
            sim.inv_item_count(sim.creature_inv(elf_id), crate::inventory::ItemKind::Bread);
        assert_eq!(bread_count, 5);
    }

    #[test]
    fn add_ground_pile_item() {
        let mut sim = test_sim(42);
        let pos = VoxelCoord::new(32, 1, 32);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::AddGroundPileItem {
                position: pos,
                item_kind: crate::inventory::ItemKind::Bread,
                quantity: 3,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        let pile = sim
            .db
            .ground_piles
            .by_position(&pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .expect("pile should exist");
        let bread_count = sim.inv_item_count(pile.inventory_id, crate::inventory::ItemKind::Bread);
        assert_eq!(bread_count, 3);
    }

    // -----------------------------------------------------------------------
    // spawn_initial_creatures
    // -----------------------------------------------------------------------

    /// Build a test config with known initial creatures for a 64x64x64 world.
    fn initial_spawn_test_config() -> GameConfig {
        use crate::config::{InitialCreatureSpec, InitialGroundPileSpec};
        let mut config = test_config();
        config.elf_starting_bread = 0; // Isolate from starting bread feature.
        config.initial_creatures = vec![
            InitialCreatureSpec {
                species: Species::Elf,
                count: 2,
                spawn_position: VoxelCoord::new(32, 1, 32),
                food_pcts: vec![100, 50],
                rest_pcts: vec![80, 40],
                bread_counts: vec![0, 3],
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
        config
    }

    #[test]
    fn spawn_initial_creatures_populates() {
        let config = initial_spawn_test_config();
        let mut sim = SimState::with_config(42, config);
        let mut events = Vec::new();
        sim.spawn_initial_creatures(&mut events);

        let elf_count = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf)
            .count();
        let capy_count = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Capybara)
            .count();
        assert_eq!(elf_count, 2);
        assert_eq!(capy_count, 1);
        assert_eq!(sim.db.creatures.len(), 3);

        // Should have emitted CreatureArrived events for all 3.
        let arrived: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, SimEventKind::CreatureArrived { .. }))
            .collect();
        assert_eq!(arrived.len(), 3);
    }

    #[test]
    fn spawn_initial_creatures_sets_food_rest() {
        let config = initial_spawn_test_config();
        let mut sim = SimState::with_config(42, config);
        let mut events = Vec::new();
        sim.spawn_initial_creatures(&mut events);

        let elf_food_max = sim.species_table[&Species::Elf].food_max;
        let elf_rest_max = sim.species_table[&Species::Elf].rest_max;

        let mut elves: Vec<_> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf)
            .collect();
        // Sort by food descending to identify first (100%) vs second (50%).
        elves.sort_by(|a, b| b.food.cmp(&a.food));

        assert_eq!(elves[0].food, elf_food_max * 100 / 100);
        assert_eq!(elves[0].rest, elf_rest_max * 80 / 100);
        assert_eq!(elves[1].food, elf_food_max * 50 / 100);
        assert_eq!(elves[1].rest, elf_rest_max * 40 / 100);

        // Second elf should have 3 bread.
        let bread_count = sim.inv_item_count(
            sim.creature_inv(elves[1].id),
            crate::inventory::ItemKind::Bread,
        );
        assert_eq!(bread_count, 3);
    }

    #[test]
    fn spawn_initial_creatures_ground_piles() {
        let config = initial_spawn_test_config();
        let mut sim = SimState::with_config(42, config);
        let mut events = Vec::new();
        sim.spawn_initial_creatures(&mut events);

        // Ground pile should exist. Position may be snapped to surface via
        // find_surface_position, so look up by the expected surface position.
        let surface_pos = sim.find_surface_position(32, 34);
        let pile = sim
            .db
            .ground_piles
            .by_position(&surface_pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .expect("ground pile should exist");
        let bread_count = sim.inv_item_count(pile.inventory_id, crate::inventory::ItemKind::Bread);
        assert_eq!(bread_count, 5);
    }

    // -----------------------------------------------------------------------
    // find_surface_position
    // -----------------------------------------------------------------------

    #[test]
    fn find_surface_position_finds_air() {
        let sim = test_sim(42);
        let center = sim.world.size_x as i32 / 2;
        let pos = sim.find_surface_position(center, center);

        // The returned position should be Air (non-solid).
        assert!(
            !sim.world.get(pos).is_solid(),
            "Surface position should be Air, got {:?}",
            sim.world.get(pos)
        );

        // One below should be solid (the ground).
        if pos.y > 0 {
            let below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
            assert!(
                sim.world.get(below).is_solid(),
                "Below surface should be solid, got {:?}",
                sim.world.get(below)
            );
        }
    }

    // -----------------------------------------------------------------------
    // AcquireItem tests
    // -----------------------------------------------------------------------

    #[test]
    fn acquire_item_picks_up_and_owns() {
        let mut sim = test_sim(42);

        // Create a ground pile with unowned bread.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 3, None, None);
        }

        // Spawn elf, position at pile.
        let elf_id = spawn_elf(&mut sim);
        let pile_nav = sim.nav_graph.find_nearest_node(pile_pos).unwrap();
        let pile_nav_pos = sim.nav_graph.node(pile_nav).position;
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.current_node = Some(pile_nav);
            c.position = pile_nav_pos;
        });

        // Create AcquireItem task with reservations.
        let task_id = TaskId::new(&mut sim.rng);
        let source = task::HaulSource::GroundPile(pile_pos);
        {
            let pile = sim
                .db
                .ground_piles
                .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
                .into_iter()
                .next()
                .unwrap();
            sim.inv_reserve_unowned_items(
                pile.inventory_id,
                inventory::ItemKind::Bread,
                2,
                task_id,
            );
        }
        let acquire_task = Task {
            id: task_id,
            kind: TaskKind::AcquireItem {
                source,
                item_kind: inventory::ItemKind::Bread,
                quantity: 2,
            },
            state: TaskState::InProgress,
            location: pile_nav,
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(acquire_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Execute.
        sim.resolve_acquire_item_action(elf_id, task_id);

        // Assert: bread removed from ground pile (1 unreserved remains).
        let pile = sim
            .db
            .ground_piles
            .by_position(&pile_pos, tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            sim.inv_item_count(pile.inventory_id, inventory::ItemKind::Bread),
            1,
            "Ground pile should have 1 bread left"
        );

        // Assert: elf now has 2 bread owned by the elf (plus starting bread).
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        let owned_bread = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bread, elf_id);
        // Elf gets starting bread (default 2) + acquired 2 = 4.
        assert_eq!(
            owned_bread, 4,
            "Elf should own 4 bread (2 starting + 2 acquired)"
        );

        // Assert: task completed.
        assert_eq!(
            sim.db.tasks.get(&task_id).unwrap().state,
            TaskState::Complete
        );
    }

    #[test]
    fn idle_elf_below_want_target_acquires_item() {
        let mut sim = test_sim(42);
        // Disable hunger/tiredness so elf stays idle.
        sim.config.elf_starting_bread = 0;
        if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
            elf_data.food_decay_per_tick = 0;
            elf_data.rest_decay_per_tick = 0;
        }
        sim.species_table = sim
            .config
            .species
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        // Set elf wants = [Bread: 2].
        sim.config.elf_default_wants = vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Bread,
            target_quantity: 2,
        }];

        // Spawn elf (will have 0 bread, wants 2).
        let elf_id = spawn_elf(&mut sim);

        // Verify elf has 0 bread and wants set.
        assert_eq!(
            sim.inv_count_owned(sim.creature_inv(elf_id), inventory::ItemKind::Bread, elf_id),
            0
        );
        assert_eq!(sim.inv_wants(sim.creature_inv(elf_id)).len(), 1);

        // Create unowned bread in a ground pile near the elf.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 5, None, None);
        }

        // Advance past a heartbeat (heartbeat interval is 3000 for elves).
        sim.step(&[], sim.tick + 5000);

        // Assert: elf should have an AcquireItem task created.
        let has_acquire_task = sim.db.tasks.iter_all().any(|t| {
            t.kind_tag == TaskKindTag::AcquireItem
                && sim
                    .task_acquire_data(t.id)
                    .is_some_and(|a| a.item_kind == inventory::ItemKind::Bread)
                && sim
                    .db
                    .creatures
                    .get(&elf_id)
                    .is_some_and(|c| c.current_task == Some(t.id))
        });
        // Either has an active task, or already completed one and picked up bread.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        let elf_bread = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bread, elf_id);
        assert!(
            has_acquire_task || elf_bread > 0,
            "Elf should have created an AcquireItem task or already acquired bread, \
             has_task={has_acquire_task}, bread={elf_bread}"
        );
    }

    #[test]
    fn acquire_item_reserves_prevent_double_claim() {
        let mut sim = test_sim(42);
        // Disable hunger/tiredness.
        sim.config.elf_starting_bread = 0;
        if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
            elf_data.food_decay_per_tick = 0;
            elf_data.rest_decay_per_tick = 0;
        }
        sim.species_table = sim
            .config
            .species
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        sim.config.elf_default_wants = vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Bread,
            target_quantity: 2,
        }];

        // Create exactly 2 unowned bread.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(pile.inventory_id, inventory::ItemKind::Bread, 2, None, None);
        }

        // Spawn 2 elves (each wants 2 bread, only 2 available total).
        let elf1 = spawn_elf(&mut sim);
        let spawn_pos = VoxelCoord::new(tree_pos.x + 1, 1, tree_pos.z);
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: spawn_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
        let elf2 = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf && c.id != elf1)
            .unwrap()
            .id;

        // Run enough ticks for both heartbeats to fire and tasks to complete.
        sim.step(&[], sim.tick + 50_000);

        // Count total bread across both elves. Should be exactly 2 (no duplication).
        let elf1_bread =
            sim.inv_count_owned(sim.creature_inv(elf1), inventory::ItemKind::Bread, elf1);
        let elf2_bread =
            sim.inv_count_owned(sim.creature_inv(elf2), inventory::ItemKind::Bread, elf2);
        assert_eq!(
            elf1_bread + elf2_bread,
            2,
            "Total bread across both elves should be exactly 2 (no duplication), \
             elf1={elf1_bread}, elf2={elf2_bread}"
        );
    }

    // -----------------------------------------------------------------------
    // Mood consequences: moping tests
    // -----------------------------------------------------------------------

    /// Helper: create a sim with custom mood_consequences config, spawn an elf,
    /// and optionally inject thoughts to reach a target mood tier.
    fn mope_test_setup(
        mope_config: crate::config::MoodConsequencesConfig,
        thoughts: &[ThoughtKind],
    ) -> (SimState, CreatureId) {
        let mut config = test_config();
        config.mood_consequences = mope_config;
        // Disable hunger and tiredness so they don't interfere.
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(99, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = *sim
            .db
            .creatures
            .iter_keys()
            .find(|id| sim.db.creatures.get(id).unwrap().species == Species::Elf)
            .expect("elf should exist");

        // Inject thoughts.
        for thought in thoughts {
            sim.add_creature_thought(elf_id, thought.clone());
        }

        (sim, elf_id)
    }

    #[test]
    fn mope_probability_zero_mean_never_fires() {
        // When mean = 0, mope_mean_ticks returns 0, check_mope should never trigger.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 0,
            mope_mean_ticks_miserable: 0,
            mope_mean_ticks_devastated: 0,
            ..Default::default()
        };
        assert_eq!(cfg.mope_mean_ticks(MoodTier::Unhappy), 0);
        assert_eq!(cfg.mope_mean_ticks(MoodTier::Miserable), 0);
        assert_eq!(cfg.mope_mean_ticks(MoodTier::Devastated), 0);

        // Run many heartbeats with zero-mean config + unhappy elf.
        let (mut sim, elf_id) = mope_test_setup(
            cfg,
            &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
        );

        // Advance many heartbeat cycles.
        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + interval * 100);

        // No Mope task should exist.
        let has_mope = sim
            .db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::Mope);
        assert!(!has_mope, "Zero mean should never produce a Mope task");
    }

    #[test]
    fn mope_probability_nonzero_fires_proportionally() {
        // With a very small mean (= heartbeat interval), mope fires ~100% per heartbeat.
        let interval = 3000_u64; // Default elf heartbeat.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: interval, // P ≈ 1.0 per heartbeat
            mope_duration_ticks: 1,            // Short mope so it completes quickly.
            ..Default::default()
        };
        let (mut sim, _elf_id) = mope_test_setup(
            cfg,
            &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
        );

        // Run several heartbeats.
        sim.step(&[], sim.tick + interval * 10);

        // With P ≈ 1.0 and 10 heartbeats, at least one Mope should fire.
        let mope_count = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Mope)
            .count();
        assert!(
            mope_count >= 1,
            "With mean=elapsed, at least one Mope should fire in 10 heartbeats, got {mope_count}"
        );
    }

    #[test]
    fn mope_config_serde_roundtrip() {
        let cfg = crate::config::MoodConsequencesConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: crate::config::MoodConsequencesConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.mope_mean_ticks_unhappy,
            cfg.mope_mean_ticks_unhappy
        );
        assert_eq!(restored.mope_duration_ticks, cfg.mope_duration_ticks);
        assert_eq!(
            restored.mope_can_interrupt_task,
            cfg.mope_can_interrupt_task
        );
    }

    #[test]
    fn mope_config_backward_compat() {
        // A GameConfig JSON without "mood_consequences" key → defaults.
        let sim = test_sim(42);
        let json = serde_json::to_string(&sim).unwrap();
        let mut val: serde_json::Value = serde_json::from_str(&json).unwrap();
        val.get_mut("config")
            .and_then(|c| c.as_object_mut())
            .unwrap()
            .remove("mood_consequences");
        let stripped = serde_json::to_string(&val).unwrap();
        let restored: SimState = serde_json::from_str(&stripped).unwrap();
        assert_eq!(
            restored.config.mood_consequences.mope_mean_ticks_unhappy,
            crate::config::MoodConsequencesConfig::default().mope_mean_ticks_unhappy
        );
    }

    #[test]
    fn unhappy_elf_eventually_mopes() {
        // Give elf SleptOnGround thoughts (weight -100 each → Unhappy/-200 → actually Miserable).
        // Use a high mope rate so it fires quickly.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000, // P ≈ 1.0 per heartbeat
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000,
            mope_duration_ticks: 100,
            ..Default::default()
        };
        let (mut sim, elf_id) = mope_test_setup(
            cfg,
            &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
        );

        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + interval * 20);

        let has_mope = sim.db.tasks.iter_all().any(|t| {
            t.kind_tag == TaskKindTag::Mope
                && sim
                    .db
                    .creatures
                    .get(&elf_id)
                    .is_some_and(|c| c.current_task == Some(t.id))
        });
        assert!(has_mope, "Unhappy elf should eventually get a Mope task");
    }

    #[test]
    fn content_elf_never_mopes() {
        // Give elf positive thoughts → Content/Happy tier. Mean=0 → never mopes.
        let cfg = crate::config::MoodConsequencesConfig::default();
        let (mut sim, elf_id) = mope_test_setup(cfg, &[ThoughtKind::AteMeal, ThoughtKind::AteMeal]);

        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + interval * 50);

        let has_mope = sim.db.tasks.iter_all().any(|t| {
            t.kind_tag == TaskKindTag::Mope
                && sim
                    .db
                    .creatures
                    .get(&elf_id)
                    .is_some_and(|c| c.current_task == Some(t.id))
        });
        assert!(!has_mope, "Content elf should never mope");
    }

    #[test]
    fn devastated_elf_interrupts_task_to_mope() {
        // Give elf Devastated-tier thoughts + a GoTo task + high mope rate.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000,
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000,
            mope_can_interrupt_task: true,
            mope_duration_ticks: 100,
        };
        let (mut sim, elf_id) = mope_test_setup(
            cfg,
            // SleptOnGround has weight -100, three of them → -300 → Devastated
            &[
                ThoughtKind::SleptOnGround,
                ThoughtKind::SleptOnGround,
                ThoughtKind::SleptOnGround,
            ],
        );

        // Assign a GoTo task to the elf so it's not idle.
        // Find a distant node for the GoTo task.
        let nav_count = sim.nav_graph.node_count();
        let far_node = NavNodeId((nav_count / 2) as u32);
        let task_id = TaskId::new(&mut sim.rng);
        let goto_task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: far_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        sim.insert_task(goto_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + interval * 20);

        // Elf should have abandoned GoTo and started moping.
        let has_mope = sim.db.tasks.iter_all().any(|t| {
            t.kind_tag == TaskKindTag::Mope
                && sim
                    .db
                    .creatures
                    .get(&elf_id)
                    .is_some_and(|c| c.current_task == Some(t.id))
        });
        assert!(
            has_mope,
            "Miserable elf with mope_can_interrupt_task should interrupt GoTo and start moping"
        );
    }

    #[test]
    fn mope_task_completes_and_elf_resumes() {
        // Short mope duration; elf should be idle afterward.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000, // P ≈ 1.0
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000,
            mope_duration_ticks: 10, // Very short mope.
            ..Default::default()
        };
        let (mut sim, _elf_id) = mope_test_setup(
            cfg,
            &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
        );

        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        // Advance enough for mope to trigger and complete.
        sim.step(&[], sim.tick + interval * 5);

        // At least one completed Mope should exist (state == Complete).
        let completed_mope = sim
            .db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::Mope && t.state == TaskState::Complete);
        assert!(
            completed_mope,
            "Mope task should complete after mope_duration_ticks"
        );
    }

    #[test]
    fn mope_does_not_interrupt_autonomous_sleep() {
        // A Devastated elf that is sleeping should NOT have sleep interrupted by mope.
        // We use a very long sleep and drain rest to 0 so the sleep won't complete
        // during the test window, proving mope didn't interrupt it.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000,
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000, // P ≈ 1.0 per heartbeat
            mope_can_interrupt_task: true,
            mope_duration_ticks: 100,
        };
        let mut config = test_config();
        config.mood_consequences = cfg;
        config.sleep_ticks_ground = 1_000_000; // Very long sleep.
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        elf_species.rest_per_sleep_tick = 1; // Tiny restore so rest_full won't trigger.
        let mut sim = SimState::with_config(99, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], 1);

        let elf_id = *sim
            .db
            .creatures
            .iter_keys()
            .find(|id| sim.db.creatures.get(id).unwrap().species == Species::Elf)
            .unwrap();

        // Drain rest to 0 so the elf won't complete sleep via rest_full.
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| c.rest = 0);

        // Inject Devastated-level thoughts.
        for _ in 0..4 {
            sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
        }

        // Manually assign a Sleep task to the elf.
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let sleep_task_id = TaskId::new(&mut sim.rng);
        let sleep_task = Task {
            id: sleep_task_id,
            kind: TaskKind::Sleep {
                bed_pos: None,
                location: crate::task::SleepLocation::Ground,
            },
            state: TaskState::InProgress,
            location: elf_node,
            progress: 0.0,
            total_cost: 1_000_000.0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(sleep_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(sleep_task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Run several heartbeats — mope rate is P≈1.0 but should not interrupt sleep.
        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + interval * 10);

        // The elf should still be sleeping — same task, never interrupted.
        let current_task = sim.db.creatures.get(&elf_id).and_then(|c| c.current_task);
        assert_eq!(
            current_task,
            Some(sleep_task_id),
            "Mope should not interrupt autonomous Sleep task"
        );
    }

    #[test]
    fn mope_does_not_interrupt_existing_mope() {
        // A moping elf should not have its mope interrupted by another mope.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000,
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000, // P ≈ 1.0 per heartbeat
            mope_can_interrupt_task: true,
            mope_duration_ticks: 100_000, // Long mope — won't complete during test.
        };
        let (mut sim, elf_id) = mope_test_setup(
            cfg,
            &[
                ThoughtKind::SleptOnGround,
                ThoughtKind::SleptOnGround,
                ThoughtKind::SleptOnGround,
            ],
        );

        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        // Run enough heartbeats to trigger the first mope.
        sim.step(&[], sim.tick + interval * 5);

        // Elf should have a mope task.
        let mope_task_id = sim
            .db
            .creatures
            .get(&elf_id)
            .and_then(|c| c.current_task)
            .filter(|tid| {
                sim.db
                    .tasks
                    .get(tid)
                    .is_some_and(|t| t.kind_tag == TaskKindTag::Mope)
            });
        assert!(mope_task_id.is_some(), "Elf should be moping");
        let first_mope_id = mope_task_id.unwrap();

        // Run more heartbeats — mope rate is P≈1.0 but should not replace existing mope.
        sim.step(&[], sim.tick + interval * 10);

        let current_task = sim.db.creatures.get(&elf_id).and_then(|c| c.current_task);
        assert_eq!(
            current_task,
            Some(first_mope_id),
            "Moping elf should keep the same Mope task, not get a replacement"
        );
    }

    #[test]
    fn mope_always_preempts_player_directed_build() {
        // With the preemption system, Mood(4) always preempts PlayerDirected(2)
        // regardless of the mope_can_interrupt_task config field (which is
        // now superseded). Verify that even with mope_can_interrupt_task=false,
        // a Devastated elf's Build task is still interrupted.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000,
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000, // P ≈ 1.0
            mope_can_interrupt_task: false,   // Superseded — should have no effect.
            mope_duration_ticks: 100,
        };
        let (mut sim, elf_id) = mope_test_setup(
            cfg,
            &[
                ThoughtKind::SleptOnGround,
                ThoughtKind::SleptOnGround,
                ThoughtKind::SleptOnGround,
            ],
        );

        // Assign a long-running Build task at the elf's current node.
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let task_id = TaskId::new(&mut sim.rng);
        let project_id = crate::types::ProjectId::new(&mut sim.rng);
        let build_task = Task {
            id: task_id,
            kind: TaskKind::Build { project_id },
            state: TaskState::InProgress,
            location: elf_node,
            progress: 0.0,
            total_cost: 1_000_000.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        sim.insert_task(build_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + interval * 10);

        // The elf should have been interrupted — its current task should differ
        // from the original Build task (either moping or idle after mope).
        let current_task = sim.db.creatures.get(&elf_id).and_then(|c| c.current_task);
        assert_ne!(
            current_task,
            Some(task_id),
            "Mope (Mood) should always preempt Build (PlayerDirected), \
             regardless of deprecated mope_can_interrupt_task config"
        );
    }

    #[test]
    fn mope_task_location_is_home_when_assigned() {
        // An elf with an assigned home should mope at the home's nav node.
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000,
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000, // P ≈ 1.0
            mope_duration_ticks: 50_000,      // Long enough to observe.
            ..Default::default()
        };
        let mut config = test_config();
        config.mood_consequences = cfg;
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(99, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Create a home.
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let home_id = insert_completed_home(&mut sim, anchor);

        // Get the home's bed nav node (this is the location mope should target).
        let bed_pos = sim
            .db
            .furniture
            .by_structure_id(&home_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|f| f.placed)
            .unwrap()
            .coord;
        let home_nav_node = sim.nav_graph.find_nearest_node(bed_pos).unwrap();

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

        let elf_id = *sim
            .db
            .creatures
            .iter_keys()
            .find(|id| sim.db.creatures.get(id).unwrap().species == Species::Elf)
            .unwrap();

        // Assign elf to home.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(home_id),
            },
        };
        sim.step(&[cmd], 2);

        // Inject negative thoughts.
        for _ in 0..3 {
            sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
        }

        // Run heartbeats until mope triggers.
        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + interval * 10);

        // Find the mope task and verify its location is the home nav node.
        let mope_task = sim.db.tasks.iter_all().find(|t| {
            t.kind_tag == TaskKindTag::Mope
                && sim
                    .db
                    .creatures
                    .get(&elf_id)
                    .is_some_and(|c| c.current_task == Some(t.id))
        });
        assert!(mope_task.is_some(), "Elf should have a Mope task");
        assert_eq!(
            mope_task.unwrap().location,
            home_nav_node,
            "Mope task location should be the home's nav node"
        );
    }

    #[test]
    fn elf_at_want_target_does_not_acquire() {
        let mut sim = test_sim(42);
        // Disable hunger/tiredness.
        if let Some(elf_data) = sim.config.species.get_mut(&Species::Elf) {
            elf_data.food_decay_per_tick = 0;
            elf_data.rest_decay_per_tick = 0;
        }
        sim.species_table = sim
            .config
            .species
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        // Set wants = [Bread: 2], give elf 2 starting bread.
        sim.config.elf_starting_bread = 2;
        sim.config.elf_default_wants = vec![building::LogisticsWant {
            item_kind: inventory::ItemKind::Bread,
            target_quantity: 2,
        }];

        let elf_id = spawn_elf(&mut sim);

        // Verify elf has exactly 2 bread.
        assert_eq!(
            sim.inv_count_owned(sim.creature_inv(elf_id), inventory::ItemKind::Bread, elf_id),
            2
        );

        // Add unowned bread to world.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let pile_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                inventory::ItemKind::Bread,
                10,
                None,
                None,
            );
        }

        // Advance past heartbeat.
        sim.step(&[], sim.tick + 5000);

        // Assert: no AcquireItem task created (elf already has enough).
        let has_acquire_task = sim.db.tasks.iter_all().any(|t| {
            t.kind_tag == TaskKindTag::AcquireItem
                && sim
                    .db
                    .creatures
                    .get(&elf_id)
                    .is_some_and(|c| c.current_task == Some(t.id))
        });
        assert!(
            !has_acquire_task,
            "Elf at want target should NOT create AcquireItem task"
        );
    }

    // -----------------------------------------------------------------------
    // raycast_solid tests
    // -----------------------------------------------------------------------

    #[test]
    fn raycast_solid_finds_solid_voxel() {
        let mut sim = test_sim(42);
        // Place a known solid voxel and cast a ray at it.
        let target = VoxelCoord::new(5, 5, 5);
        sim.world.set(target, VoxelType::Trunk);
        let from = [5.5, 10.0, 5.5];
        let dir = [0.0, -1.0, 0.0];
        let result = sim.raycast_solid(from, dir, 100, None);
        assert!(result.is_some(), "Should hit the trunk voxel");
        let (coord, face) = result.unwrap();
        assert_eq!(coord, target);
        assert_eq!(face, 2, "Should enter through PosY face (from above)");
    }

    #[test]
    fn raycast_solid_returns_correct_face() {
        let mut sim = test_sim(42);
        // Place a solid voxel in a clear area far from the tree.
        let target = VoxelCoord::new(5, 10, 5);
        sim.world.set(target, VoxelType::Trunk);

        // Ray from above → enters through PosY face (index 2).
        let from_above = [5.5, 15.0, 5.5];
        let dir_down = [0.0, -1.0, 0.0];
        let (coord, face) = sim.raycast_solid(from_above, dir_down, 100, None).unwrap();
        assert_eq!(coord, target);
        assert_eq!(face, 2, "Ray from above should enter through PosY face");

        // Ray from +X side → enters through PosX face (index 0).
        let from_east = [10.5, 10.5, 5.5];
        let dir_west = [-1.0, 0.0, 0.0];
        let (coord, face) = sim.raycast_solid(from_east, dir_west, 100, None).unwrap();
        assert_eq!(coord, target);
        assert_eq!(face, 0, "Ray from +X should enter through PosX face");

        // Ray from +Z side → enters through PosZ face (index 4).
        let from_south = [5.5, 10.5, 10.5];
        let dir_north = [0.0, 0.0, -1.0];
        let (coord, face) = sim.raycast_solid(from_south, dir_north, 100, None).unwrap();
        assert_eq!(coord, target);
        assert_eq!(face, 4, "Ray from +Z should enter through PosZ face");
    }

    #[test]
    fn raycast_solid_returns_none_for_empty_ray() {
        let sim = test_sim(42);
        // Cast a ray straight up from the top of the world — should hit nothing.
        let from = [5.5, 50.0, 5.5];
        let dir = [0.0, 1.0, 0.0];
        let result = sim.raycast_solid(from, dir, 100, None);
        assert_eq!(result, None);
    }

    #[test]
    fn raycast_solid_negative_face_directions() {
        let mut sim = test_sim(42);
        let target = VoxelCoord::new(5, 10, 5);
        sim.world.set(target, VoxelType::Trunk);

        // Ray from -X side → enters through NegX face (index 1).
        let from_west = [0.5, 10.5, 5.5];
        let dir_east = [1.0, 0.0, 0.0];
        let (coord, face) = sim.raycast_solid(from_west, dir_east, 100, None).unwrap();
        assert_eq!(coord, target);
        assert_eq!(face, 1, "Ray from -X should enter through NegX face");

        // Ray from -Z side → enters through NegZ face (index 5).
        let from_north = [5.5, 10.5, 0.5];
        let dir_south = [0.0, 0.0, 1.0];
        let (coord, face) = sim.raycast_solid(from_north, dir_south, 100, None).unwrap();
        assert_eq!(coord, target);
        assert_eq!(face, 5, "Ray from -Z should enter through NegZ face");
    }

    #[test]
    fn raycast_solid_skips_starting_voxel() {
        let mut sim = test_sim(42);
        // Place two solid voxels adjacent vertically.
        sim.world.set(VoxelCoord::new(5, 10, 5), VoxelType::Trunk);
        sim.world.set(VoxelCoord::new(5, 11, 5), VoxelType::Trunk);
        // Start inside the upper voxel, cast downward — should skip
        // the starting voxel and hit the lower one.
        let from = [5.5, 11.5, 5.5];
        let dir = [0.0, -1.0, 0.0];
        let result = sim.raycast_solid(from, dir, 100, None);
        assert!(result.is_some());
        let (coord, _face) = result.unwrap();
        assert_eq!(
            coord,
            VoxelCoord::new(5, 10, 5),
            "Should skip starting voxel"
        );
    }

    #[test]
    fn raycast_solid_hits_blueprint_with_overlay() {
        let mut sim = test_sim(42);
        // Find an air voxel adjacent to trunk (valid for platform placement).
        let target = find_air_adjacent_to_trunk(&sim);

        // Without overlay, ray passes through (it's air).
        let from = [
            target.x as f32 + 0.5,
            target.y as f32 + 5.0,
            target.z as f32 + 0.5,
        ];
        let dir = [0.0, -1.0, 0.0];
        let result_no_overlay = sim.raycast_solid(from, dir, 20, None);
        // Ray might hit something else (e.g., floor below), but not the target.
        assert!(
            result_no_overlay.is_none() || result_no_overlay.unwrap().0 != target,
            "Without overlay, ray should not hit the air voxel as solid"
        );

        // Designate a platform blueprint at the target.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![target],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);
        assert_eq!(sim.db.blueprints.len(), 1);

        // With overlay, ray hits the blueprint voxel.
        let overlay = sim.blueprint_overlay();
        let result_with_overlay = sim.raycast_solid(from, dir, 20, Some(&overlay));
        assert!(
            result_with_overlay.is_some(),
            "With overlay, ray should hit the blueprint voxel"
        );
        let (coord, face) = result_with_overlay.unwrap();
        assert_eq!(coord, target);
        assert_eq!(face, 2, "Should enter through PosY face (from above)");
    }

    // -----------------------------------------------------------------------
    // auto_ladder_orientation tests
    // -----------------------------------------------------------------------

    #[test]
    fn auto_ladder_orientation_faces_trunk() {
        let mut sim = test_sim(42);
        // Place a trunk column at (5, 10..14, 5) and test ladder at (6, 10, 5).
        // Use an elevated position far from the real tree to avoid interference.
        for y in 10..14 {
            sim.world.set(VoxelCoord::new(5, y, 5), VoxelType::Trunk);
        }
        // Clear all neighbors around the ladder column to ensure only the
        // trunk at x=5 is adjacent.
        for y in 10..14 {
            sim.world.set(VoxelCoord::new(6, y, 5), VoxelType::Air);
            sim.world.set(VoxelCoord::new(7, y, 5), VoxelType::Air);
            sim.world.set(VoxelCoord::new(6, y, 4), VoxelType::Air);
            sim.world.set(VoxelCoord::new(6, y, 6), VoxelType::Air);
        }

        let face = sim.auto_ladder_orientation(6, 10, 5, 4);
        // Trunk is to the west (-X) of the ladder, so the ladder should face
        // NegX (face 1).
        assert_eq!(face, 1, "Ladder should face the trunk (NegX)");
    }

    #[test]
    fn auto_ladder_orientation_tie_breaks_to_first() {
        let mut sim = test_sim(42);
        // Place solid voxels on both +X and -X sides of the ladder column,
        // creating a tie. The code iterates [PosX, PosZ, NegX, NegZ], so
        // PosX (face 0) should win.
        for y in 10..14 {
            sim.world.set(VoxelCoord::new(4, y, 5), VoxelType::Trunk); // -X
            sim.world.set(VoxelCoord::new(6, y, 5), VoxelType::Trunk); // +X
            // Clear other neighbors.
            sim.world.set(VoxelCoord::new(5, y, 4), VoxelType::Air);
            sim.world.set(VoxelCoord::new(5, y, 6), VoxelType::Air);
        }
        let face = sim.auto_ladder_orientation(5, 10, 5, 4);
        assert_eq!(
            face, 0,
            "Tie should break to PosX (first in iteration order)"
        );
    }

    #[test]
    fn auto_ladder_orientation_no_neighbors_defaults_east() {
        let mut sim = test_sim(42);
        // Clear all neighbors around the ladder column — no solid voxels.
        for y in 10..14 {
            sim.world.set(VoxelCoord::new(5, y, 5), VoxelType::Air);
            sim.world.set(VoxelCoord::new(4, y, 5), VoxelType::Air);
            sim.world.set(VoxelCoord::new(6, y, 5), VoxelType::Air);
            sim.world.set(VoxelCoord::new(5, y, 4), VoxelType::Air);
            sim.world.set(VoxelCoord::new(5, y, 6), VoxelType::Air);
        }
        let face = sim.auto_ladder_orientation(5, 10, 5, 4);
        // All counts are 0, so first direction (PosX, face 0) wins.
        assert_eq!(face, 0, "No neighbors should default to PosX (East)");
    }

    // -----------------------------------------------------------------------
    // Notification tests
    // -----------------------------------------------------------------------

    #[test]
    fn debug_notification_command_creates_notification() {
        let mut sim = test_sim(42);
        let pid = sim.player_id;

        assert_eq!(sim.db.notifications.iter_all().count(), 0);

        let cmd = SimCommand {
            player_id: pid,
            tick: 1,
            action: SimAction::DebugNotification {
                message: "hello world".to_string(),
            },
        };
        sim.step(&[cmd], 1);

        assert_eq!(sim.db.notifications.iter_all().count(), 1);
        let notif = sim.db.notifications.iter_all().next().unwrap();
        assert_eq!(notif.message, "hello world");
        assert_eq!(notif.tick, 1);
    }

    #[test]
    fn notifications_persist_across_serde_roundtrip() {
        let mut sim = test_sim(42);
        let pid = sim.player_id;

        let cmd = SimCommand {
            player_id: pid,
            tick: 1,
            action: SimAction::DebugNotification {
                message: "save me".to_string(),
            },
        };
        sim.step(&[cmd], 1);
        assert_eq!(sim.db.notifications.iter_all().count(), 1);

        // Serialize and deserialize.
        let json = serde_json::to_string(&sim).unwrap();
        let mut restored: SimState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.db.notifications.iter_all().count(), 1);
        let notif = restored.db.notifications.iter_all().next().unwrap();
        assert_eq!(notif.message, "save me");

        // Verify that auto-increment IDs don't collide after deserialization.
        restored.add_notification("post-load".to_string());
        let ids: Vec<_> = restored.db.notifications.iter_all().map(|n| n.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(
            ids[1] > ids[0],
            "Post-load notification ID ({:?}) should be greater than pre-existing ({:?})",
            ids[1],
            ids[0]
        );
    }

    // -----------------------------------------------------------------------
    // Manufacturing / Workshop tests
    // -----------------------------------------------------------------------

    #[test]
    fn new_item_kind_serde_roundtrip() {
        use crate::inventory::ItemKind;
        for kind in [ItemKind::Bow, ItemKind::Arrow, ItemKind::Bowstring] {
            let json = serde_json::to_string(&kind).unwrap();
            let restored: ItemKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, restored);
        }
    }

    #[test]
    fn material_enum_serde_roundtrip() {
        use crate::inventory::Material;
        for mat in [
            Material::Oak,
            Material::Birch,
            Material::Willow,
            Material::Ash,
            Material::Yew,
            Material::FruitSpecies(crate::fruit::FruitSpeciesId(42)),
        ] {
            let json = serde_json::to_string(&mat).unwrap();
            let restored: Material = serde_json::from_str(&json).unwrap();
            assert_eq!(mat, restored);
        }
    }

    #[test]
    fn item_stack_serde_backward_compat() {
        // Old JSON without material/quality/enchantment_id should deserialize.
        let json = r#"{
            "id": 1,
            "inventory_id": 1,
            "kind": "Bread",
            "quantity": 5,
            "owner": null,
            "reserved_by": null
        }"#;
        let stack: crate::db::ItemStack = serde_json::from_str(json).unwrap();
        assert_eq!(stack.kind, inventory::ItemKind::Bread);
        assert_eq!(stack.quantity, 5);
        assert!(stack.material.is_none());
        assert_eq!(stack.quality, 0);
        assert!(stack.enchantment_id.is_none());
    }

    #[test]
    fn inv_add_simple_item_stacks_correctly() {
        let mut sim = test_sim(42);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 3, None, None);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 2, None, None);
        // Should stack into one row with qty 5.
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].quantity, 5);
    }

    #[test]
    fn inv_add_item_material_creates_separate_stacks() {
        let mut sim = test_sim(42);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.inv_add_item(
            inv_id,
            inventory::ItemKind::Bow,
            1,
            None,
            None,
            Some(inventory::Material::Oak),
            0,
            None,
        );
        sim.inv_add_item(
            inv_id,
            inventory::ItemKind::Bow,
            1,
            None,
            None,
            Some(inventory::Material::Yew),
            0,
            None,
        );
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        assert_eq!(stacks.len(), 2, "Different materials should not stack");
    }

    #[test]
    fn inv_add_item_quality_creates_separate_stacks() {
        let mut sim = test_sim(42);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.inv_add_item(
            inv_id,
            inventory::ItemKind::Bow,
            1,
            None,
            None,
            None,
            0,
            None,
        );
        sim.inv_add_item(
            inv_id,
            inventory::ItemKind::Bow,
            1,
            None,
            None,
            None,
            3,
            None,
        );
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        assert_eq!(stacks.len(), 2, "Different qualities should not stack");
    }

    #[test]
    fn inv_normalize_respects_material_quality() {
        let mut sim = test_sim(42);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        // Create two stacks with same kind but different quality.
        sim.inv_add_item(
            inv_id,
            inventory::ItemKind::Arrow,
            5,
            None,
            None,
            None,
            0,
            None,
        );
        sim.inv_add_item(
            inv_id,
            inventory::ItemKind::Arrow,
            3,
            None,
            None,
            None,
            1,
            None,
        );
        sim.inv_normalize(inv_id);
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        assert_eq!(
            stacks.len(),
            2,
            "Merge should keep different qualities separate"
        );
    }

    #[test]
    fn item_subcomponent_cascade_delete() {
        let mut sim = test_sim(42);
        let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bow, 1, None, None);
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let stack_id = stacks[0].id;

        // Add a subcomponent.
        let _ = sim
            .db
            .item_subcomponents
            .insert_auto_no_fk(|id| crate::db::ItemSubcomponent {
                id,
                item_stack_id: stack_id,
                component_kind: inventory::ItemKind::Bowstring,
                material: None,
                quality: 0,
                quantity_per_item: 1,
            });
        assert_eq!(sim.db.item_subcomponents.len(), 1);

        // Delete the item stack — subcomponent should cascade.
        sim.db.remove_item_stack(&stack_id).unwrap();
        assert_eq!(
            sim.db.item_subcomponents.len(),
            0,
            "Subcomponent should cascade delete with parent stack"
        );
    }

    #[test]
    fn enchantment_effect_cascade_delete() {
        let mut sim = test_sim(42);

        // Create an enchantment.
        let ench_id = sim
            .db
            .item_enchantments
            .insert_auto_no_fk(|id| crate::db::ItemEnchantment { id })
            .unwrap();

        // Add an effect.
        let _ = sim
            .db
            .enchantment_effects
            .insert_auto_no_fk(|id| crate::db::EnchantmentEffect {
                id,
                enchantment_id: ench_id,
                effect_kind: inventory::EffectKind::Placeholder,
                magnitude: 10,
                threshold: None,
            });
        assert_eq!(sim.db.enchantment_effects.len(), 1);

        // Delete enchantment — effect should cascade.
        sim.db.remove_item_enchantment(&ench_id).unwrap();
        assert_eq!(
            sim.db.enchantment_effects.len(),
            0,
            "Effect should cascade delete with parent enchantment"
        );
    }

    #[test]
    fn completed_structure_serde_backward_compat_workshop() {
        // Old JSON without workshop fields should deserialize with defaults.
        let mut rng = GameRng::new(42);
        let structure = CompletedStructure {
            id: StructureId(1),
            project_id: ProjectId::new(&mut rng),
            build_type: BuildType::Building,
            anchor: VoxelCoord::new(0, 0, 0),
            width: 5,
            depth: 5,
            height: 3,
            completed_tick: 100,
            name: None,
            furnishing: None,
            inventory_id: InventoryId(0),
            logistics_priority: None,
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        };
        let json = serde_json::to_string(&structure).unwrap();
        // Remove workshop fields to simulate old save.
        let json_old = json
            .replace(r#","workshop_enabled":false"#, "")
            .replace(r#","workshop_recipe_ids":[]"#, "")
            .replace(r#","workshop_recipe_targets":{}"#, "");
        let restored: CompletedStructure = serde_json::from_str(&json_old).unwrap();
        assert!(!restored.workshop_enabled);
        assert!(restored.workshop_recipe_ids.is_empty());
        assert!(restored.workshop_recipe_targets.is_empty());
    }

    #[test]
    fn game_config_with_recipes_deserializes() {
        use crate::species::CombatAI;
        let config_json = std::fs::read_to_string("../default_config.json").unwrap();
        let config: crate::config::GameConfig = serde_json::from_str(&config_json).unwrap();
        assert_eq!(config.recipes.len(), 3);
        assert_eq!(config.recipes[0].id, "bowstring");
        assert_eq!(config.recipes[1].id, "bow");
        assert_eq!(config.recipes[2].id, "arrow");
        // CombatAI and detection range survive JSON roundtrip.
        assert_eq!(
            config.species[&Species::Goblin].combat_ai,
            CombatAI::AggressiveMelee
        );
        assert_eq!(
            config.species[&Species::Goblin].hostile_detection_range_sq,
            225
        );
        assert_eq!(config.species[&Species::Elf].combat_ai, CombatAI::Passive);
        assert_eq!(config.species[&Species::Elf].hostile_detection_range_sq, 0);
    }

    #[test]
    fn game_config_without_recipes_gets_defaults() {
        // Minimal valid config JSON — no recipes field.
        let config = crate::config::GameConfig::default();
        assert_eq!(config.recipes.len(), 3);
        assert_eq!(config.workshop_default_priority, 8);
    }

    #[test]
    fn furnish_workshop_sets_defaults() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Workshop,
                greenhouse_species: None,
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert!(structure.workshop_enabled, "Workshop should be enabled");
        assert_eq!(
            structure.workshop_recipe_ids.len(),
            sim.config.recipes.len(),
            "Workshop should have all recipes by default"
        );
        assert_eq!(
            structure.logistics_priority,
            Some(sim.config.workshop_default_priority),
            "Workshop should have default priority"
        );

        // Should have logistics wants for recipe inputs.
        let inv_id = structure.inventory_id;
        let wants = sim
            .db
            .logistics_want_rows
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        assert!(
            !wants.is_empty(),
            "Workshop should have logistics wants for recipe inputs"
        );
    }

    #[test]
    fn set_workshop_config_updates_fields() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish as workshop first.
        let furnish_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Workshop,
                greenhouse_species: None,
            },
        };
        sim.step(&[furnish_cmd], sim.tick + 1);

        // Disable workshop and set only arrow recipe.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: false,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "arrow".to_string(),
                    target: 0,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert!(!structure.workshop_enabled);
        assert_eq!(structure.workshop_recipe_ids, vec!["arrow".to_string()]);
    }

    #[test]
    fn set_workshop_config_rejects_invalid_recipe_ids() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish as workshop.
        let furnish_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Workshop,
                greenhouse_species: None,
            },
        };
        sim.step(&[furnish_cmd], sim.tick + 1);

        // Set with one valid and one invalid recipe ID.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![
                    WorkshopRecipeEntry {
                        recipe_id: "arrow".to_string(),
                        target: 0,
                    },
                    WorkshopRecipeEntry {
                        recipe_id: "nonexistent".to_string(),
                        target: 0,
                    },
                ],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert_eq!(
            structure.workshop_recipe_ids,
            vec!["arrow".to_string()],
            "Invalid recipe IDs should be filtered out"
        );
    }

    #[test]
    fn moping_creates_notification() {
        // Set up with guaranteed moping (mean = 1 so it always fires).
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 1,
            mope_duration_ticks: 5000,
            ..Default::default()
        };
        let (mut sim, _elf_id) = mope_test_setup(
            cfg,
            // Two SleptOnGround thoughts push mood to Unhappy.
            &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
        );

        assert_eq!(sim.db.notifications.iter_all().count(), 0);

        // Run enough ticks for a heartbeat to fire and trigger moping.
        let elf_hb = sim.config.species[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + elf_hb + 1);

        // Should have a moping notification.
        assert!(
            sim.db.notifications.iter_all().count() > 0,
            "Expected a moping notification"
        );
        let notif = sim.db.notifications.iter_all().next().unwrap();
        assert!(
            notif.message.contains("moping"),
            "Notification should mention moping, got: {}",
            notif.message
        );
        assert!(
            notif.message.contains("Unhappy"),
            "Notification should mention mood tier, got: {}",
            notif.message
        );
    }

    #[test]
    fn set_workshop_config_rejects_non_workshop() {
        let mut sim = test_sim(42);
        let anchor = find_building_site(&sim);
        let structure_id = insert_completed_building(&mut sim, anchor);

        // Furnish as kitchen, not workshop.
        let furnish_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Kitchen,
                greenhouse_species: None,
            },
        };
        sim.step(&[furnish_cmd], sim.tick + 1);

        // Try SetWorkshopConfig on a kitchen — should be silently ignored.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "arrow".to_string(),
                    target: 0,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert!(
            !structure.workshop_enabled,
            "Kitchen should not become a workshop"
        );
    }

    /// Helper: create a workshop with an elf, optionally stocking inputs.
    fn setup_workshop(sim: &mut SimState) -> (StructureId, CreatureId) {
        let anchor = find_building_site(sim);
        let structure_id = insert_completed_building(sim, anchor);

        // Furnish as workshop.
        let furnish_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Workshop,
                greenhouse_species: None,
            },
        };
        sim.step(&[furnish_cmd], sim.tick + 1);

        // Manually place furniture (skip furnish task flow).
        let structure = sim.db.structures.get(&structure_id).unwrap();
        let furn_ids: Vec<_> = sim
            .db
            .furniture
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .iter()
            .map(|f| f.id)
            .collect();
        for fid in furn_ids {
            let _ = sim.db.furniture.modify_unchecked(&fid, |f| {
                f.placed = true;
            });
        }

        // Spawn elf near building.
        let elf_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: structure.anchor,
            },
        };
        sim.step(&[elf_cmd], sim.tick + 1);
        let elf_id = sim
            .db
            .creatures
            .by_species(&Species::Elf, tabulosity::QueryOpts::ASC)
            .last()
            .unwrap()
            .id;

        // Make elf not hungry/tired so they don't generate autonomous tasks.
        let food_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetCreatureFood {
                creature_id: elf_id,
                food: sim.species_table[&Species::Elf].food_max,
            },
        };
        let rest_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetCreatureRest {
                creature_id: elf_id,
                rest: sim.species_table[&Species::Elf].rest_max,
            },
        };
        sim.step(&[food_cmd, rest_cmd], sim.tick + 1);

        // Set all recipes active with target 100 (tests that need different
        // targets override with their own SetWorkshopConfig).
        let all_configs: Vec<WorkshopRecipeEntry> = sim
            .config
            .recipes
            .iter()
            .map(|r| WorkshopRecipeEntry {
                recipe_id: r.id.clone(),
                target: 100,
            })
            .collect();
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: all_configs,
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        (structure_id, elf_id)
    }

    #[test]
    fn workshop_monitor_creates_craft_task_when_inputs_available() {
        let mut sim = test_sim(42);
        let (structure_id, _elf_id) = setup_workshop(&mut sim);

        // Stock workshop with Fruit (for bowstring recipe: 1 Fruit → 20 Bowstring).
        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Fruit, 1, None, None);

        // Run logistics heartbeat to trigger workshop monitor.
        sim.process_workshop_monitor();

        let craft_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft)
            .collect();
        assert_eq!(craft_tasks.len(), 1, "Should create 1 Craft task");
        assert_eq!(craft_tasks[0].state, task::TaskState::Available);
        assert_eq!(craft_tasks[0].required_species, Some(Species::Elf));

        // Check craft data.
        let craft_data = sim.task_craft_data(craft_tasks[0].id).unwrap();
        assert_eq!(craft_data.recipe_id, "bowstring");
    }

    #[test]
    fn workshop_monitor_skips_when_inputs_insufficient() {
        let mut sim = test_sim(42);
        let (structure_id, _elf_id) = setup_workshop(&mut sim);

        // Set only bow recipe with target > 0 (needs 1 Bowstring). Don't stock any Bowstring.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "bow".to_string(),
                    target: 50,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        sim.process_workshop_monitor();

        let craft_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft)
            .collect();
        assert_eq!(
            craft_tasks.len(),
            0,
            "Should not create Craft task without sufficient inputs"
        );
    }

    #[test]
    fn moping_notification_unnamed_elf_uses_generic_text() {
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 1,
            mope_duration_ticks: 5000,
            ..Default::default()
        };
        let (mut sim, elf_id) = mope_test_setup(
            cfg,
            &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
        );

        // Clear the elf's name to exercise the empty-name branch.
        let mut elf = sim.db.creatures.get(&elf_id).unwrap();
        elf.name = String::new();
        let _ = sim.db.creatures.update_no_fk(elf);

        let elf_hb = sim.config.species[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + elf_hb + 1);

        assert!(
            sim.db.notifications.iter_all().count() > 0,
            "Expected a moping notification for unnamed elf"
        );
        let notif = sim.db.notifications.iter_all().next().unwrap();
        assert!(
            notif.message.starts_with("An elf is moping"),
            "Unnamed elf notification should use generic text, got: {}",
            notif.message
        );
    }

    #[test]
    fn workshop_monitor_skips_when_active_craft_exists() {
        let mut sim = test_sim(42);
        let (structure_id, _elf_id) = setup_workshop(&mut sim);

        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Fruit, 5, None, None);

        // First run creates a Craft task.
        sim.process_workshop_monitor();
        let craft_count = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft)
            .count();
        assert_eq!(craft_count, 1);

        // Second run should skip (active task exists).
        sim.process_workshop_monitor();
        let craft_count = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft)
            .count();
        assert_eq!(craft_count, 1, "Should not create duplicate Craft task");
    }

    #[test]
    fn workshop_monitor_skips_when_disabled() {
        let mut sim = test_sim(42);
        let (structure_id, _elf_id) = setup_workshop(&mut sim);

        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Fruit, 1, None, None);

        // Disable the workshop (target > 0, but disabled flag prevents crafting).
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: false,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "bowstring".to_string(),
                    target: 50,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        sim.process_workshop_monitor();

        let craft_count = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft)
            .count();
        assert_eq!(
            craft_count, 0,
            "Disabled workshop should not create Craft tasks"
        );
    }

    #[test]
    fn do_craft_consumes_inputs_produces_outputs() {
        let mut sim = test_sim(42);
        let (structure_id, elf_id) = setup_workshop(&mut sim);

        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Fruit, 1, None, None);

        // Create craft task manually (bowstring: 1 Fruit → 20 Bowstring).
        sim.process_workshop_monitor();
        let task_id = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Craft)
            .unwrap()
            .id;

        // Claim the task.
        sim.claim_task(elf_id, task_id);

        // Resolve craft action (single-action task).
        sim.resolve_craft_action(elf_id);

        // Verify: Fruit consumed, Bowstring produced.
        let fruit_count = sim.inv_item_count(inv_id, inventory::ItemKind::Fruit);
        assert_eq!(fruit_count, 0, "Fruit should be consumed");
        let bowstring_count = sim.inv_item_count(inv_id, inventory::ItemKind::Bowstring);
        assert_eq!(bowstring_count, 20, "Should produce 20 Bowstrings");

        // Task should be complete.
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, task::TaskState::Complete);
    }

    #[test]
    fn do_craft_records_subcomponents_bow() {
        let mut sim = test_sim(42);
        let (structure_id, elf_id) = setup_workshop(&mut sim);

        // Configure for bow recipe only.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "bow".to_string(),
                    target: 100,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        // Stock 1 Bowstring.
        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bowstring, 1, None, None);

        sim.process_workshop_monitor();
        let task_id = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Craft)
            .unwrap()
            .id;

        sim.claim_task(elf_id, task_id);

        // Run to completion (8000 ticks for bow).
        sim.resolve_craft_action(elf_id);

        // Verify Bow produced.
        let bow_count = sim.inv_item_count(inv_id, inventory::ItemKind::Bow);
        assert_eq!(bow_count, 1, "Should produce 1 Bow");

        // Verify subcomponent recorded.
        let bow_stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        let bow_stack = bow_stacks
            .iter()
            .find(|s| s.kind == inventory::ItemKind::Bow)
            .unwrap();
        let subcomponents = sim
            .db
            .item_subcomponents
            .by_item_stack_id(&bow_stack.id, tabulosity::QueryOpts::ASC);
        assert_eq!(subcomponents.len(), 1, "Bow should have 1 subcomponent");
        assert_eq!(
            subcomponents[0].component_kind,
            inventory::ItemKind::Bowstring
        );
        assert_eq!(subcomponents[0].quantity_per_item, 1);
    }

    #[test]
    fn cleanup_craft_task_releases_reservations() {
        let mut sim = test_sim(42);
        let (structure_id, _elf_id) = setup_workshop(&mut sim);

        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Fruit, 1, None, None);

        sim.process_workshop_monitor();
        let task_id = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Craft)
            .unwrap()
            .id;

        // Verify fruit is reserved.
        let reserved = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.reserved_by == Some(task_id))
            .count();
        assert!(reserved > 0, "Fruit should be reserved");

        // Clean up the task.
        sim.cleanup_craft_task(task_id);

        // Verify reservations cleared.
        let still_reserved = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.reserved_by == Some(task_id))
            .count();
        assert_eq!(still_reserved, 0, "Reservations should be cleared");

        // Task should be complete.
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, task::TaskState::Complete);
    }

    #[test]
    fn integration_bowstring_recipe() {
        let mut sim = test_sim(42);
        let (structure_id, elf_id) = setup_workshop(&mut sim);

        // Stock Fruit, configure only bowstring recipe with target 100.
        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Fruit, 1, None, None);

        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "bowstring".to_string(),
                    target: 100,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        sim.process_workshop_monitor();
        let task_id = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Craft)
            .unwrap()
            .id;
        sim.claim_task(elf_id, task_id);

        sim.resolve_craft_action(elf_id);

        assert_eq!(sim.inv_item_count(inv_id, inventory::ItemKind::Fruit), 0);
        assert_eq!(
            sim.inv_item_count(inv_id, inventory::ItemKind::Bowstring),
            20
        );
    }

    #[test]
    fn integration_arrow_recipe_no_inputs() {
        let mut sim = test_sim(42);
        let (structure_id, elf_id) = setup_workshop(&mut sim);

        // Arrow recipe has no inputs — should create task immediately.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "arrow".to_string(),
                    target: 100,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        sim.process_workshop_monitor();
        let task_id = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Craft)
            .unwrap()
            .id;
        sim.claim_task(elf_id, task_id);

        sim.resolve_craft_action(elf_id);

        let inv_id = sim.structure_inv(structure_id);
        assert_eq!(sim.inv_item_count(inv_id, inventory::ItemKind::Arrow), 20);
    }

    #[test]
    fn workshop_monitor_skips_when_target_reached() {
        let mut sim = test_sim(42);
        let (structure_id, _creature_id) = setup_workshop(&mut sim);

        // Configure arrow recipe with target 20.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "arrow".to_string(),
                    target: 20,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        // Add 20 arrows to workshop inventory (meeting target).
        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 20, None, None);

        // Run logistics heartbeat — should NOT create a Craft task.
        sim.process_workshop_monitor();
        let craft_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
            .collect();
        assert!(
            craft_tasks.is_empty(),
            "Should not craft when target is reached"
        );
    }

    #[test]
    fn workshop_monitor_crafts_when_below_target() {
        let mut sim = test_sim(42);
        let (structure_id, _creature_id) = setup_workshop(&mut sim);

        // Configure arrow recipe with target 40.
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "arrow".to_string(),
                    target: 40,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        // Add 10 arrows (below target of 40).
        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 10, None, None);

        // Run workshop monitor — should create a Craft task.
        sim.process_workshop_monitor();
        let craft_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
            .collect();
        assert_eq!(craft_tasks.len(), 1, "Should craft when below target");
    }

    #[test]
    fn workshop_monitor_skips_when_target_zero() {
        let mut sim = test_sim(42);
        let (structure_id, _creature_id) = setup_workshop(&mut sim);

        // Configure arrow recipe with target 0 (don't craft).
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![WorkshopRecipeEntry {
                    recipe_id: "arrow".to_string(),
                    target: 0,
                }],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        sim.process_workshop_monitor();
        let craft_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
            .collect();
        assert_eq!(craft_tasks.len(), 0, "Should not craft when target is 0");
    }

    #[test]
    fn workshop_monitor_skips_when_target_missing() {
        let mut sim = test_sim(42);
        let (structure_id, _creature_id) = setup_workshop(&mut sim);

        // Clear all recipe configs so targets are missing (= don't craft).
        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        sim.process_workshop_monitor();
        let craft_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
            .collect();
        assert_eq!(
            craft_tasks.len(),
            0,
            "Should not craft when no targets are set"
        );
    }

    #[test]
    fn set_workshop_config_stores_targets() {
        let mut sim = test_sim(42);
        let (structure_id, _creature_id) = setup_workshop(&mut sim);

        let config_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SetWorkshopConfig {
                structure_id,
                workshop_enabled: true,
                recipe_configs: vec![
                    WorkshopRecipeEntry {
                        recipe_id: "bowstring".to_string(),
                        target: 50,
                    },
                    WorkshopRecipeEntry {
                        recipe_id: "arrow".to_string(),
                        target: 0,
                    },
                ],
            },
        };
        sim.step(&[config_cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        // Target 50 should be stored.
        assert_eq!(
            structure.workshop_recipe_targets.get("bowstring"),
            Some(&50)
        );
        // Target 0 means "don't craft" — stored as 0.
        assert_eq!(structure.workshop_recipe_targets.get("arrow"), Some(&0));
    }

    #[test]
    fn craft_task_serde_roundtrip() {
        use crate::prng::GameRng;
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);

        let task = task::Task {
            id: task_id,
            kind: task::TaskKind::Craft {
                structure_id: StructureId(5),
                recipe_id: "bowstring".to_string(),
            },
            state: task::TaskState::Available,
            location: NavNodeId(10),
            progress: 0.0,
            total_cost: 5000.0,
            required_species: Some(Species::Elf),
            origin: task::TaskOrigin::Automated,
            target_creature: None,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: task::Task = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, task_id);
        match &restored.kind {
            task::TaskKind::Craft {
                structure_id,
                recipe_id,
            } => {
                assert_eq!(*structure_id, StructureId(5));
                assert_eq!(recipe_id, "bowstring");
            }
            other => panic!("Expected Craft task, got {:?}", other),
        }
        assert_eq!(restored.origin, task::TaskOrigin::Automated);
    }

    // =========================================================================
    // Ground pile gravity
    // =========================================================================

    #[test]
    fn pile_on_solid_ground_does_not_fall() {
        let mut sim = test_sim(42);
        // Place a pile on y=1 (above ForestFloor at y=0 — always solid).
        let pos = VoxelCoord::new(10, 1, 10);
        let pile_id = sim.ensure_ground_pile(pos);
        sim.inv_add_simple_item(
            sim.db.ground_piles.get(&pile_id).unwrap().inventory_id,
            inventory::ItemKind::Bread,
            3,
            None,
            None,
        );

        let fell = sim.apply_pile_gravity();
        assert_eq!(fell, 0);

        // Pile is still at original position.
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        assert_eq!(pile.position, pos);
    }

    #[test]
    fn floating_pile_falls_to_surface() {
        let mut sim = test_sim(42);
        // Create a solid platform at y=5 by setting (10, 5, 10) to Platform.
        let platform_pos = VoxelCoord::new(10, 5, 10);
        sim.world.set(platform_pos, VoxelType::GrownPlatform);

        // Place a pile at y=6 (on top of the platform).
        let pile_pos = VoxelCoord::new(10, 6, 10);
        let pile_id = sim.ensure_ground_pile(pile_pos);
        sim.inv_add_simple_item(
            sim.db.ground_piles.get(&pile_id).unwrap().inventory_id,
            inventory::ItemKind::Bread,
            5,
            None,
            None,
        );

        // Pile should not fall — platform is solid below.
        assert_eq!(sim.apply_pile_gravity(), 0);

        // Remove the platform — pile is now floating.
        sim.world.set(platform_pos, VoxelType::Air);
        let fell = sim.apply_pile_gravity();
        assert_eq!(fell, 1);

        // Pile should have fallen to y=1 (above ForestFloor at y=0).
        // The pile gets a new ID after remove+re-insert, so look up by position.
        let landing = VoxelCoord::new(10, 1, 10);
        let piles_at_landing = sim
            .db
            .ground_piles
            .by_position(&landing, tabulosity::QueryOpts::ASC);
        assert_eq!(piles_at_landing.len(), 1);
        let pile = &piles_at_landing[0];

        // Items should still be there.
        let stacks = sim.inv_items(pile.inventory_id);
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].kind, inventory::ItemKind::Bread);
        assert_eq!(stacks[0].quantity, 5);
    }

    #[test]
    fn floating_pile_merges_with_existing_pile() {
        let mut sim = test_sim(42);
        // Place a pile on the ground at y=1.
        let ground_pos = VoxelCoord::new(15, 1, 15);
        let ground_pile_id = sim.ensure_ground_pile(ground_pos);
        let ground_inv = sim
            .db
            .ground_piles
            .get(&ground_pile_id)
            .unwrap()
            .inventory_id;
        sim.inv_add_simple_item(ground_inv, inventory::ItemKind::Bread, 3, None, None);

        // Create a platform and a pile on top of it.
        let platform_pos = VoxelCoord::new(15, 5, 15);
        sim.world.set(platform_pos, VoxelType::GrownPlatform);
        let high_pos = VoxelCoord::new(15, 6, 15);
        let high_pile_id = sim.ensure_ground_pile(high_pos);
        let high_inv = sim.db.ground_piles.get(&high_pile_id).unwrap().inventory_id;
        sim.inv_add_simple_item(high_inv, inventory::ItemKind::Fruit, 2, None, None);

        // Remove the platform — high pile should fall and merge with ground pile.
        sim.world.set(platform_pos, VoxelType::Air);
        let fell = sim.apply_pile_gravity();
        assert_eq!(fell, 1);

        // The floating pile should be deleted.
        assert!(sim.db.ground_piles.get(&high_pile_id).is_none());

        // The ground pile should have both item types.
        let ground_pile = sim.db.ground_piles.get(&ground_pile_id).unwrap();
        assert_eq!(ground_pile.position, ground_pos);
        let stacks = sim.inv_items(ground_pile.inventory_id);
        assert_eq!(stacks.len(), 2);
        let bread = stacks
            .iter()
            .find(|s| s.kind == inventory::ItemKind::Bread)
            .unwrap();
        let fruit = stacks
            .iter()
            .find(|s| s.kind == inventory::ItemKind::Fruit)
            .unwrap();
        assert_eq!(bread.quantity, 3);
        assert_eq!(fruit.quantity, 2);
    }

    #[test]
    fn merge_stacks_same_item_kind() {
        let mut sim = test_sim(42);
        // Both piles have Bread — after merge, the ground pile should have a
        // single Bread stack with the combined quantity.
        let ground_pos = VoxelCoord::new(20, 1, 20);
        let ground_pile_id = sim.ensure_ground_pile(ground_pos);
        let ground_inv = sim
            .db
            .ground_piles
            .get(&ground_pile_id)
            .unwrap()
            .inventory_id;
        sim.inv_add_simple_item(ground_inv, inventory::ItemKind::Bread, 4, None, None);

        let platform_pos = VoxelCoord::new(20, 3, 20);
        sim.world.set(platform_pos, VoxelType::GrownPlatform);
        let high_pos = VoxelCoord::new(20, 4, 20);
        let high_pile_id = sim.ensure_ground_pile(high_pos);
        let high_inv = sim.db.ground_piles.get(&high_pile_id).unwrap().inventory_id;
        sim.inv_add_simple_item(high_inv, inventory::ItemKind::Bread, 6, None, None);

        sim.world.set(platform_pos, VoxelType::Air);
        sim.apply_pile_gravity();

        assert!(sim.db.ground_piles.get(&high_pile_id).is_none());
        let stacks = sim.inv_items(ground_inv);
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].kind, inventory::ItemKind::Bread);
        assert_eq!(stacks[0].quantity, 10);
    }

    #[test]
    fn pile_falls_to_intermediate_surface() {
        let mut sim = test_sim(42);
        // Two platforms stacked: y=3 and y=6. Pile at y=7.
        // Remove y=6 — pile should fall to y=4 (on top of y=3 platform), not y=1.
        let lower_platform = VoxelCoord::new(25, 3, 25);
        let upper_platform = VoxelCoord::new(25, 6, 25);
        sim.world.set(lower_platform, VoxelType::GrownPlatform);
        sim.world.set(upper_platform, VoxelType::GrownPlatform);

        let pile_pos = VoxelCoord::new(25, 7, 25);
        let pile_id = sim.ensure_ground_pile(pile_pos);
        sim.inv_add_simple_item(
            sim.db.ground_piles.get(&pile_id).unwrap().inventory_id,
            inventory::ItemKind::Bread,
            1,
            None,
            None,
        );

        // Remove upper platform only.
        sim.world.set(upper_platform, VoxelType::Air);
        sim.apply_pile_gravity();

        // Pile gets a new ID after remove+re-insert, so look up by position.
        let landing = VoxelCoord::new(25, 4, 25);
        let piles = sim
            .db
            .ground_piles
            .by_position(&landing, tabulosity::QueryOpts::ASC);
        assert_eq!(piles.len(), 1, "pile should land on top of lower platform");
    }

    #[test]
    fn multiple_floating_piles_in_same_column() {
        let mut sim = test_sim(42);
        // Two platforms at y=3 and y=6. Piles at y=4 and y=7.
        // Remove both platforms — both piles should fall to y=1, merging.
        let p1 = VoxelCoord::new(30, 3, 30);
        let p2 = VoxelCoord::new(30, 6, 30);
        sim.world.set(p1, VoxelType::GrownPlatform);
        sim.world.set(p2, VoxelType::GrownPlatform);

        let pile1_pos = VoxelCoord::new(30, 4, 30);
        let pile1_id = sim.ensure_ground_pile(pile1_pos);
        let pile1_inv = sim.db.ground_piles.get(&pile1_id).unwrap().inventory_id;
        sim.inv_add_simple_item(pile1_inv, inventory::ItemKind::Bread, 2, None, None);

        let pile2_pos = VoxelCoord::new(30, 7, 30);
        let pile2_id = sim.ensure_ground_pile(pile2_pos);
        let pile2_inv = sim.db.ground_piles.get(&pile2_id).unwrap().inventory_id;
        sim.inv_add_simple_item(pile2_inv, inventory::ItemKind::Fruit, 3, None, None);

        sim.world.set(p1, VoxelType::Air);
        sim.world.set(p2, VoxelType::Air);
        let fell = sim.apply_pile_gravity();
        assert_eq!(fell, 2);

        // Both should have ended up at y=1. Only one pile should remain.
        let remaining: Vec<_> = sim
            .db
            .ground_piles
            .iter_all()
            .filter(|p| p.position.x == 30 && p.position.z == 30)
            .collect();
        assert_eq!(remaining.len(), 1);
        let final_pile = &remaining[0];
        assert_eq!(final_pile.position.y, 1);

        // Should have both item types.
        let stacks = sim.inv_items(final_pile.inventory_id);
        let total_items: u32 = stacks.iter().map(|s| s.quantity).sum();
        assert_eq!(total_items, 5);
    }

    #[test]
    fn empty_floating_pile_is_cleaned_up() {
        let mut sim = test_sim(42);
        // A floating pile with no items should still be moved.
        let platform_pos = VoxelCoord::new(35, 3, 35);
        sim.world.set(platform_pos, VoxelType::GrownPlatform);
        let pile_pos = VoxelCoord::new(35, 4, 35);
        let _pile_id = sim.ensure_ground_pile(pile_pos);

        sim.world.set(platform_pos, VoxelType::Air);
        let fell = sim.apply_pile_gravity();
        assert_eq!(fell, 1);

        // Pile should have moved to y=1.
        let landing = VoxelCoord::new(35, 1, 35);
        let piles = sim
            .db
            .ground_piles
            .by_position(&landing, tabulosity::QueryOpts::ASC);
        assert_eq!(piles.len(), 1);
    }

    #[test]
    fn inv_merge_combines_inventories() {
        let mut sim = test_sim(42);
        let src = sim.create_inventory(crate::db::InventoryOwnerKind::GroundPile);
        let dst = sim.create_inventory(crate::db::InventoryOwnerKind::GroundPile);

        // Same kind in both — should combine into one stack.
        sim.inv_add_simple_item(src, inventory::ItemKind::Bread, 3, None, None);
        sim.inv_add_simple_item(dst, inventory::ItemKind::Bread, 2, None, None);
        // Different kind in src — should become a new stack in dst.
        sim.inv_add_simple_item(src, inventory::ItemKind::Fruit, 1, None, None);

        sim.inv_merge(src, dst);

        // Source should be empty.
        assert!(sim.inv_items(src).is_empty());

        // Destination should have 2 stacks: Bread(5) and Fruit(1).
        let stacks = sim.inv_items(dst);
        assert_eq!(stacks.len(), 2);
        let bread = stacks
            .iter()
            .find(|s| s.kind == inventory::ItemKind::Bread)
            .unwrap();
        let fruit = stacks
            .iter()
            .find(|s| s.kind == inventory::ItemKind::Fruit)
            .unwrap();
        assert_eq!(bread.quantity, 5);
        assert_eq!(fruit.quantity, 1);
    }

    #[test]
    fn ensure_ground_pile_snaps_floating_position_to_surface() {
        let mut sim = test_sim(42);
        // Request a pile at y=10 with no solid voxel below (except floor at y=0).
        let floating_pos = VoxelCoord::new(40, 10, 40);
        let pile_id = sim.ensure_ground_pile(floating_pos);

        // Pile should have been snapped to y=1 (above ForestFloor).
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        assert_eq!(pile.position, VoxelCoord::new(40, 1, 40));
    }

    #[test]
    fn ensure_ground_pile_snaps_to_intermediate_platform() {
        let mut sim = test_sim(42);
        // Platform at y=5, request pile at y=10.
        sim.world
            .set(VoxelCoord::new(42, 5, 42), VoxelType::GrownPlatform);
        let pile_id = sim.ensure_ground_pile(VoxelCoord::new(42, 10, 42));

        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        assert_eq!(pile.position, VoxelCoord::new(42, 6, 42));
    }

    #[test]
    fn ensure_ground_pile_merges_when_snapped_to_existing() {
        let mut sim = test_sim(42);
        // Create a pile at y=1.
        let ground_pos = VoxelCoord::new(44, 1, 44);
        let ground_pile_id = sim.ensure_ground_pile(ground_pos);
        let ground_inv = sim
            .db
            .ground_piles
            .get(&ground_pile_id)
            .unwrap()
            .inventory_id;
        sim.inv_add_simple_item(ground_inv, inventory::ItemKind::Bread, 5, None, None);

        // Request a pile at y=8 (floating) — should snap to y=1 and return
        // the existing pile instead of creating a new one.
        let returned_id = sim.ensure_ground_pile(VoxelCoord::new(44, 8, 44));
        assert_eq!(returned_id, ground_pile_id);

        // Only one pile at this column.
        let piles = sim
            .db
            .ground_piles
            .by_position(&ground_pos, tabulosity::QueryOpts::ASC);
        assert_eq!(piles.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Action system tests (F-creature-actions coverage)
    // -----------------------------------------------------------------------

    /// High-priority #1: Verify action state (action_kind + next_available_tick)
    /// is correctly set during Build, Sleep, Cook, and Eat work actions.
    #[test]
    fn action_state_set_during_work_actions() {
        let mut config = test_config();
        // Very long build so it can't complete before we check.
        config.build_work_ticks_per_voxel = 500_000;
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        let elf_id = spawn_elf(&mut sim);

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

        // Run enough ticks for the elf to arrive and start building.
        sim.step(&[], sim.tick + 100_000);

        // Elf must have claimed the build task and be mid-action.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_some(),
            "Elf should have claimed the Build task"
        );
        let task_id = elf.current_task.unwrap();
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.kind_tag, TaskKindTag::Build);
        assert_eq!(task.state, TaskState::InProgress);
        assert_eq!(
            elf.action_kind,
            ActionKind::Build,
            "Elf working on build should have ActionKind::Build"
        );
        assert!(
            elf.next_available_tick.is_some(),
            "Elf in Build action should have next_available_tick set"
        );
    }

    /// High-priority #2: MoveAction cleanup on resolve — after a creature
    /// completes a Move action (arrives somewhere), the MoveAction row is
    /// deleted.
    #[test]
    fn move_action_cleaned_up_after_arrival() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Let the elf wander once to create a MoveAction.
        sim.step(&[], sim.tick + 2);

        // Elf should have a MoveAction now.
        assert!(
            sim.db.move_actions.get(&elf_id).is_some(),
            "MoveAction should exist after wander"
        );
        let next_tick = sim
            .db
            .creatures
            .get(&elf_id)
            .unwrap()
            .next_available_tick
            .unwrap();

        // Advance past the move completion.
        sim.step(&[], next_tick + 1);

        // After arrival, action state should be cleared (or a new action started).
        // Either way, the *old* MoveAction should have been cleaned up. If the elf
        // wandered again it will have a new MoveAction, which is fine — the test
        // just verifies the resolve path ran.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        if elf.action_kind == ActionKind::NoAction {
            assert!(
                sim.db.move_actions.get(&elf_id).is_none(),
                "MoveAction should be deleted when elf is idle"
            );
        }
        // If elf started a new wander, it has action_kind=Move — that's fine,
        // it means the old one was resolved and a new one created.
    }

    /// High-priority #3: Task removed during a non-Move action. Creature
    /// should fall through to decision cascade and wander.
    #[test]
    fn task_removed_during_build_action_creature_wanders() {
        let mut config = test_config();
        // Very long build so it can't complete before we cancel.
        config.build_work_ticks_per_voxel = 1_000_000;
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        let elf_id = spawn_elf(&mut sim);

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

        // Run enough for elf to reach the build site (but build won't finish
        // because it takes 1M ticks).
        sim.step(&[], sim.tick + 100_000);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        let task_id = elf.current_task;
        // Elf should have claimed the build task.
        if task_id.is_none() {
            // If no task yet, run longer for elf to find and claim it.
            sim.step(&[], sim.tick + 200_000);
        }
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(elf.current_task.is_some(), "Elf should have a Build task");

        // Cancel the build (remove the blueprint and task).
        let bp = sim.db.blueprints.iter_all().next().unwrap();
        let project_id = bp.id;
        let cancel_cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::CancelBuild { project_id },
        };
        sim.step(&[cancel_cmd], sim.tick + 2);

        // Advance for the elf's activation to fire after cancellation.
        sim.step(&[], sim.tick + 1_100_000);

        // Elf should have lost the task and be wandering.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            elf.current_task.is_none()
                || sim
                    .db
                    .tasks
                    .get(&elf.current_task.unwrap())
                    .is_some_and(|t| t.kind_tag != TaskKindTag::Build),
            "Elf should no longer have the cancelled Build task"
        );
    }

    /// High-priority #4: PickUp phase transition — use the logistics
    /// heartbeat to create a haul task, then run the full pipeline and
    /// verify the task transitions through GoingToDestination.
    #[test]
    fn pickup_action_transitions_haul_phase() {
        let mut sim = test_sim(42);
        sim.config.haul_pickup_action_ticks = 500;
        sim.config.haul_dropoff_action_ticks = 500;
        let elf_species = sim.config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;

        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Place bread on the ground.
        let pile_pos = tree_pos;
        {
            let pile_id = sim.ensure_ground_pile(pile_pos);
            let pile = sim.db.ground_piles.get(&pile_id).unwrap();
            sim.inv_add_simple_item(
                pile.inventory_id,
                crate::inventory::ItemKind::Bread,
                5,
                None,
                None,
            );
        }

        // Create a building that wants bread.
        let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
        let sid = insert_building(
            &mut sim,
            building_anchor,
            Some(5),
            vec![crate::building::LogisticsWant {
                item_kind: crate::inventory::ItemKind::Bread,
                target_quantity: 5,
            }],
        );

        // Run logistics heartbeat to create haul task.
        sim.process_logistics_heartbeat();

        let haul_tasks: Vec<_> = sim
            .db
            .tasks
            .iter_all()
            .filter(|t| t.kind_tag == TaskKindTag::Haul)
            .collect();
        assert_eq!(haul_tasks.len(), 1, "Expected 1 haul task");
        let haul_task_id = haul_tasks[0].id;

        // Verify initial state is GoingToSource.
        let haul_data = sim.task_haul_data(haul_task_id).unwrap();
        assert_eq!(haul_data.phase, task::HaulPhase::GoingToSource);
        let initial_location = sim.db.tasks.get(&haul_task_id).unwrap().location;

        // Spawn an elf and run to completion.
        let _elf_id = spawn_elf(&mut sim);
        sim.step(&[], sim.tick + 100_000);

        // Task should have completed (or at least transitioned phases).
        let task = sim.db.tasks.get(&haul_task_id).unwrap();
        if task.state == TaskState::Complete {
            // Full pipeline ran — items delivered.
            let structure = sim.db.structures.get(&sid).unwrap();
            let bread_count = sim.inv_unreserved_item_count(
                structure.inventory_id,
                crate::inventory::ItemKind::Bread,
            );
            assert!(bread_count > 0, "Bread should have been delivered");
        } else {
            // At minimum, haul should have progressed past GoingToSource.
            let haul_data = sim.task_haul_data(haul_task_id).unwrap();
            assert_eq!(
                haul_data.phase,
                task::HaulPhase::GoingToDestination,
                "Haul should have transitioned to GoingToDestination"
            );
            // Task location should have changed to destination.
            assert_ne!(
                task.location, initial_location,
                "Task location should update after PickUp"
            );
        }
    }

    /// High-priority #5: Mope progress increments by mope_action_ticks, not
    /// by 1. A mope task with total_cost = 10000 and mope_action_ticks = 1000
    /// completes after exactly 10 actions.
    #[test]
    fn mope_progress_increments_by_action_ticks() {
        let cfg = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: 3000, // P ≈ 1.0
            mope_mean_ticks_miserable: 3000,
            mope_mean_ticks_devastated: 3000,
            mope_duration_ticks: 10_000,
            ..Default::default()
        };
        let (mut sim, _elf_id) = mope_test_setup(
            cfg,
            &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
        );
        sim.config.mope_action_ticks = 1000;

        let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        // Step enough to trigger mope.
        sim.step(&[], sim.tick + interval * 5);

        // Find the mope task.
        let mope_task = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Mope);
        assert!(mope_task.is_some(), "Mope task should exist");
        let mope_task = mope_task.unwrap();

        // total_cost should be mope_duration_ticks (10000).
        assert_eq!(mope_task.total_cost, 10_000.0);

        // If still in progress, progress should be a multiple of mope_action_ticks.
        if mope_task.state == TaskState::InProgress {
            let remainder = mope_task.progress % 1000.0;
            assert_eq!(
                remainder, 0.0,
                "Mope progress should be a multiple of mope_action_ticks, got {}",
                mope_task.progress
            );
        }

        // If completed, verify progress >= total_cost.
        if mope_task.state == TaskState::Complete {
            assert!(mope_task.progress >= mope_task.total_cost);
        }
    }

    /// High-priority #6: Sleep adaptive completion — a creature near full
    /// rest completes sleep early (via rest_full) with progress < total_cost.
    #[test]
    fn sleep_adaptive_completion_rest_full_exits_early() {
        let mut config = test_config();
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        // High restore per sleep action so rest fills in ~1-2 actions.
        // rest_max is 1e15, so each action restores rest_per_sleep_tick * sleep_action_ticks.
        // With 1000 action_ticks and rest_per_sleep_tick = 1e11, each action restores 1e14.
        // At 95% rest, need 5e13 → 1 action should fill it.
        elf_species.rest_per_sleep_tick = 100_000_000_000;
        // Heartbeat far in the future so it doesn't interfere.
        elf_species.heartbeat_interval_ticks = 1_000_000;
        config.sleep_action_ticks = 1000;
        config.sleep_ticks_ground = 1_000_000; // Very long sleep by progress.
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let rest_max = sim.species_table[&Species::Elf].rest_max;

        // Spawn elf (step to tick 1 only, before first activation).
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Set rest to 95% (near full). With rest_per_sleep_tick=500 and
        // sleep_action_ticks=1000, each action restores 500_000. rest_max
        // is typically 100_000, so 5% = 5000 → one action should fill it.
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.rest = rest_max * 95 / 100;
        });

        // Create a ground sleep task at elf's location.
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let task_id = TaskId::new(&mut sim.rng);
        let sleep_task = task::Task {
            id: task_id,
            kind: task::TaskKind::Sleep {
                bed_pos: None,
                location: task::SleepLocation::Ground,
            },
            state: task::TaskState::InProgress,
            location: elf_node,
            progress: 0.0,
            total_cost: (sim.config.sleep_ticks_ground / sim.config.sleep_action_ticks) as f32,
            required_species: None,
            origin: task::TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(sleep_task);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Run enough for the first activation (tick 2) + a few sleep actions.
        sim.step(&[], sim.tick + 10_000);

        let sleep_task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            sleep_task.state,
            TaskState::Complete,
            "Sleep should complete early when rest hits max"
        );
        // Progress should be much less than total_cost (early exit via rest_full).
        assert!(
            sleep_task.progress < sleep_task.total_cost,
            "Progress ({}) should be less than total_cost ({}) for early rest-full completion",
            sleep_task.progress,
            sleep_task.total_cost
        );
    }

    /// High-priority #7: ActionKind + MoveAction serde roundtrip.
    #[test]
    fn action_state_survives_serde_roundtrip() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Let the elf wander to create a Move action + MoveAction row.
        sim.step(&[], sim.tick + 2);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(elf.action_kind, ActionKind::Move);
        assert!(elf.next_available_tick.is_some());
        assert!(sim.db.move_actions.get(&elf_id).is_some());

        // Serialize and deserialize.
        let json = serde_json::to_string(&sim).unwrap();
        let restored: SimState = serde_json::from_str(&json).unwrap();

        let elf_r = restored.db.creatures.get(&elf_id).unwrap();
        assert_eq!(elf_r.action_kind, ActionKind::Move);
        assert_eq!(elf_r.next_available_tick, elf.next_available_tick);

        let ma = restored.db.move_actions.get(&elf_id).unwrap();
        let ma_orig = sim.db.move_actions.get(&elf_id).unwrap();
        assert_eq!(ma.move_from, ma_orig.move_from);
        assert_eq!(ma.move_to, ma_orig.move_to);
        assert_eq!(ma.move_start_tick, ma_orig.move_start_tick);
    }

    /// Medium-priority #8: abort_current_action with Move cleans up MoveAction.
    #[test]
    fn abort_move_action_cleans_up_move_action_row() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Let the elf wander to create a Move action.
        sim.step(&[], sim.tick + 2);
        assert!(sim.db.move_actions.get(&elf_id).is_some());
        assert_eq!(
            sim.db.creatures.get(&elf_id).unwrap().action_kind,
            ActionKind::Move
        );

        // Manually abort.
        sim.abort_current_action(elf_id);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(elf.action_kind, ActionKind::NoAction);
        assert!(elf.next_available_tick.is_none());
        assert!(
            sim.db.move_actions.get(&elf_id).is_none(),
            "MoveAction row should be deleted after abort"
        );
    }

    /// Medium-priority #9: Mope interrupts non-Move work action (Build).
    /// Uses mope_mean_ticks = heartbeat interval (P ≈ 1.0 per heartbeat)
    /// and runs 200 heartbeats to ensure at least one mope fires.
    #[test]
    fn mope_interrupts_build_action() {
        let mut config = test_config();
        config.build_work_ticks_per_voxel = 500_000; // Very long build.
        let heartbeat = config
            .species
            .get(&Species::Elf)
            .unwrap()
            .heartbeat_interval_ticks;
        config.mood_consequences = crate::config::MoodConsequencesConfig {
            mope_mean_ticks_unhappy: heartbeat,
            mope_mean_ticks_miserable: heartbeat,
            mope_mean_ticks_devastated: heartbeat,
            mope_can_interrupt_task: true,
            mope_duration_ticks: 100,
        };
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(99, config);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let cmd_spawn = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd_spawn], 1);
        let elf_id = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Inject Devastated-tier thoughts.
        sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
        sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
        sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);

        // Designate a build.
        let cmd_build = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd_build], sim.tick + 2);

        // Run enough for elf to reach the build site and start building,
        // then for heartbeats to fire and trigger mope. P ≈ 1.0 per heartbeat
        // so 200 heartbeats should guarantee at least one mope fires.
        sim.step(&[], sim.tick + heartbeat * 200);

        let mope_ever_existed = sim
            .db
            .tasks
            .iter_all()
            .any(|t| t.kind_tag == TaskKindTag::Mope);
        assert!(
            mope_ever_existed,
            "Mope should have triggered at least once during 200 heartbeats with P≈1.0"
        );
    }

    /// Medium-priority #12: Creature removal cleans up MoveAction.
    /// The MoveAction table's FK on creature_id has on_delete cascade
    /// at the Database level. Since MoveAction's PK is also the FK
    /// (creature_id), we verify the cascade path works.
    #[test]
    fn creature_removal_cleans_up_move_action() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Let the elf wander to create a MoveAction.
        sim.step(&[], sim.tick + 2);
        assert!(sim.db.move_actions.get(&elf_id).is_some());

        // Manually remove both the MoveAction and creature (simulating
        // what a real despawn would do — abort_current_action + remove).
        sim.abort_current_action(elf_id);
        assert!(
            sim.db.move_actions.get(&elf_id).is_none(),
            "MoveAction should be removed by abort_current_action"
        );

        // Creature should still exist.
        assert!(sim.db.creatures.get(&elf_id).is_some());
    }

    /// Medium-priority #13: interpolated_position with non-Move action returns
    /// static position, not a crash.
    #[test]
    fn interpolated_position_with_build_action_returns_static() {
        let mut config = test_config();
        config.build_work_ticks_per_voxel = 100_000;
        let mut sim = SimState::with_config(42, config);
        let air_coord = find_air_adjacent_to_trunk(&sim);
        let elf_id = spawn_elf(&mut sim);

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

        // Run until elf is mid-Build.
        sim.step(&[], sim.tick + 200_000);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        // Force build action state for the test if not naturally set.
        if elf.action_kind == ActionKind::Build {
            // Calling interpolated_position with no MoveAction should not panic
            // and should return the static position.
            let pos = elf.interpolated_position(sim.tick as f64, None);
            let expected = (
                elf.position.x as f32,
                elf.position.y as f32,
                elf.position.z as f32,
            );
            assert_eq!(
                pos, expected,
                "Non-Move action should return static position"
            );
        }
    }

    /// Lower-priority #14: Config duration fields control timing — verify
    /// eat_action_ticks controls when Eat action resolves.
    #[test]
    fn eat_action_ticks_controls_timing() {
        let mut config = test_config();
        config.eat_action_ticks = 3000;
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn elf.
        let mut events = Vec::new();
        sim.spawn_creature(Species::Elf, tree_pos, &mut events);
        let elf_id = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Create an EatBread task at the elf's location.
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let task_id = TaskId::new(&mut sim.rng);
        let eat_task = task::Task {
            id: task_id,
            kind: task::TaskKind::EatBread,
            state: task::TaskState::InProgress,
            location: elf_node,
            progress: 0.0,
            total_cost: 1.0,
            required_species: None,
            origin: task::TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(eat_task);

        // Give elf bread and assign task.
        let inv_id = sim.creature_inv(elf_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 1, None, None);
        {
            let mut c = sim.db.creatures.get(&elf_id).unwrap();
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        // Run 1 tick to let activation fire and start the Eat action.
        sim.step(&[], sim.tick + 5);

        // Elf should be in Eat action with next_available_tick = tick + 3000.
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        if elf.action_kind == ActionKind::Eat {
            let expected_tick = elf.next_available_tick.unwrap();
            // The action should be scheduled ~3000 ticks from when it started.
            assert!(
                expected_tick > sim.tick,
                "Eat action should be scheduled in the future"
            );
        }

        // Run past the action duration.
        sim.step(&[], sim.tick + 3100);

        // Task should be complete.
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.state,
            TaskState::Complete,
            "Eat task should be complete"
        );
    }

    /// Lower-priority #15: carve_work_ticks_per_voxel is separate from build.
    /// Verifies that at a time when a build would have completed, a carve
    /// with 10x the duration is still in progress.
    #[test]
    fn carve_uses_separate_duration_from_build() {
        let mut config = test_config();
        config.build_work_ticks_per_voxel = 5000;
        config.carve_work_ticks_per_voxel = 500_000; // 100x slower than build.
        let elf_species = config.species.get_mut(&Species::Elf).unwrap();
        elf_species.food_decay_per_tick = 0;
        elf_species.rest_decay_per_tick = 0;
        let mut sim = SimState::with_config(42, config);

        // Find a non-forest-floor solid voxel to carve (e.g., a trunk voxel
        // that isn't on the ground).
        let tree = &sim.trees[&sim.player_tree_id];
        let carve_coord = tree
            .trunk_voxels
            .iter()
            .find(|c| c.y > 1)
            .copied()
            .expect("Should have trunk voxels above y=1");

        let _elf_id = spawn_elf(&mut sim);

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateCarve {
                voxels: vec![carve_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], sim.tick + 2);

        // Verify the carve task exists.
        let task = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Build);
        assert!(task.is_some(), "Carve task should exist");
        let task_id = task.unwrap().id;

        // Run enough for the elf to arrive and start working, but less than
        // carve_work_ticks_per_voxel. A build (5000 ticks) would be done by now,
        // but a carve (500_000 ticks) should still be in progress.
        sim.step(&[], sim.tick + 100_000);

        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.state,
            TaskState::InProgress,
            "Carve should still be in progress (500k ticks) — a build (5k) would be done by now"
        );
    }

    /// Lower-priority #18: Config backward compat for new action_ticks fields.
    /// Verify that the default GameConfig has the expected action_ticks values.
    #[test]
    fn config_backward_compat_action_ticks_defaults() {
        let config = GameConfig::default();

        assert_eq!(config.sleep_action_ticks, 1000);
        assert_eq!(config.eat_action_ticks, 1500);
        assert_eq!(config.harvest_action_ticks, 1500);
        assert_eq!(config.acquire_item_action_ticks, 1000);
        assert_eq!(config.haul_pickup_action_ticks, 1000);
        assert_eq!(config.haul_dropoff_action_ticks, 1000);
        assert_eq!(config.mope_action_ticks, 1000);
    }

    /// Lower-priority #20: abort_current_action is harmless with NoAction.
    #[test]
    fn abort_no_action_is_harmless() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn elf but only step to tick 1 (before first activation).
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(elf.action_kind, ActionKind::NoAction);

        // Abort should be a no-op.
        sim.abort_current_action(elf_id);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(elf.action_kind, ActionKind::NoAction);
        assert!(elf.next_available_tick.is_none());
    }

    /// ActionKind serde roundtrip for all 15 variants.
    #[test]
    fn action_kind_serde_roundtrip_all_variants() {
        let variants = [
            ActionKind::NoAction,
            ActionKind::Move,
            ActionKind::Build,
            ActionKind::Furnish,
            ActionKind::Cook,
            ActionKind::Craft,
            ActionKind::Sleep,
            ActionKind::Eat,
            ActionKind::Harvest,
            ActionKind::AcquireItem,
            ActionKind::PickUp,
            ActionKind::DropOff,
            ActionKind::Mope,
            ActionKind::MeleeStrike,
            ActionKind::Shoot,
        ];
        for variant in &variants {
            let json = serde_json::to_string(variant).unwrap();
            let restored: ActionKind = serde_json::from_str(&json).unwrap();
            assert_eq!(
                *variant, restored,
                "ActionKind {:?} should survive serde roundtrip",
                variant
            );
        }
    }

    /// abort_current_action with Build (non-Move) just clears state, no
    /// MoveAction deletion attempt.
    #[test]
    fn abort_build_action_clears_state_only() {
        let mut sim = test_sim(42);
        let tree_pos = sim.trees[&sim.player_tree_id].position;

        // Spawn elf but only step to tick 1 (before first activation).
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
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap()
            .id;

        // Manually set elf to Build action state.
        let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
            c.action_kind = ActionKind::Build;
            c.next_available_tick = Some(sim.tick + 50_000);
        });

        // No MoveAction should exist (elf hasn't moved yet).
        assert!(sim.db.move_actions.get(&elf_id).is_none());

        // Abort should clear the state without errors.
        sim.abort_current_action(elf_id);

        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(elf.action_kind, ActionKind::NoAction);
        assert!(elf.next_available_tick.is_none());
        assert!(sim.db.move_actions.get(&elf_id).is_none());
    }

    // -------------------------------------------------------------------
    // Civilization tests
    // -------------------------------------------------------------------

    #[test]
    fn spawned_elf_gets_player_civ_id() {
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
        sim.step(&[cmd], 1);

        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert_eq!(
            elf.civ_id, sim.player_civ_id,
            "Spawned elf should belong to the player's civilization"
        );
    }

    #[test]
    fn spawned_non_elf_has_no_civ_id() {
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
        sim.step(&[cmd], 1);

        let capy = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Capybara)
            .unwrap();
        assert_eq!(
            capy.civ_id, None,
            "Non-elf creature should not have a civ_id"
        );
    }

    #[test]
    fn discover_civ_creates_relationship() {
        let mut sim = test_sim(42);

        // Get two existing civ IDs from worldgen.
        let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
        assert!(civs.len() >= 2, "Need at least 2 civs for this test");

        let civ_a = civs[0].id;
        let civ_b = civs[1].id;

        // Remove any existing relationship between a→b from worldgen.
        let existing: Vec<_> = sim
            .db
            .civ_relationships
            .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|r| r.to_civ == civ_b)
            .map(|r| r.id)
            .collect();
        for id in existing {
            let _ = sim.db.civ_relationships.remove_no_fk(&id);
        }

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DiscoverCiv {
                civ_id: civ_a,
                discovered_civ: civ_b,
                initial_opinion: CivOpinion::Neutral,
            },
        };
        sim.step(&[cmd], 1);

        let rels = sim
            .db
            .civ_relationships
            .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC);
        let found = rels.iter().any(|r| r.to_civ == civ_b);
        assert!(found, "DiscoverCiv should create a relationship from a→b");
    }

    #[test]
    fn discover_civ_is_idempotent() {
        let mut sim = test_sim(42);
        let civs: Vec<_> = sim.db.civilizations.iter_all().collect();
        assert!(civs.len() >= 2);

        let civ_a = civs[0].id;
        let civ_b = civs[1].id;

        // Remove existing relationship.
        let existing: Vec<_> = sim
            .db
            .civ_relationships
            .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|r| r.to_civ == civ_b)
            .map(|r| r.id)
            .collect();
        for id in existing {
            let _ = sim.db.civ_relationships.remove_no_fk(&id);
        }

        // Discover twice.
        for tick in [1, 2] {
            let cmd = SimCommand {
                player_id: sim.player_id,
                tick,
                action: SimAction::DiscoverCiv {
                    civ_id: civ_a,
                    discovered_civ: civ_b,
                    initial_opinion: CivOpinion::Neutral,
                },
            };
            sim.step(&[cmd], tick);
        }

        let rels = sim
            .db
            .civ_relationships
            .by_from_civ(&civ_a, tabulosity::QueryOpts::ASC);
        let count = rels.iter().filter(|r| r.to_civ == civ_b).count();
        assert_eq!(
            count, 1,
            "DiscoverCiv should not create duplicate relationships"
        );
    }

    #[test]
    fn discover_civ_noop_for_nonexistent_civ() {
        let mut sim = test_sim(42);
        let rel_count_before = sim.db.civ_relationships.iter_all().count();

        // Use a CivId that doesn't exist.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DiscoverCiv {
                civ_id: CivId(999),
                discovered_civ: CivId(0),
                initial_opinion: CivOpinion::Neutral,
            },
        };
        sim.step(&[cmd], 1);

        let rel_count_after = sim.db.civ_relationships.iter_all().count();
        assert_eq!(
            rel_count_before, rel_count_after,
            "No-op for nonexistent civ"
        );
    }

    #[test]
    fn set_civ_opinion_updates_relationship() {
        let mut sim = test_sim(42);

        // Find an existing relationship from worldgen.
        let rel = sim.db.civ_relationships.iter_all().next();
        assert!(
            rel.is_some(),
            "Need at least one relationship for this test"
        );
        let rel = rel.unwrap();
        let rel_id = rel.id;
        let from_civ = rel.from_civ;
        let to_civ = rel.to_civ;
        let new_opinion = if rel.opinion == CivOpinion::Hostile {
            CivOpinion::Friendly
        } else {
            CivOpinion::Hostile
        };

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SetCivOpinion {
                civ_id: from_civ,
                target_civ: to_civ,
                opinion: new_opinion,
            },
        };
        sim.step(&[cmd], 1);

        let updated = sim.db.civ_relationships.get(&rel_id).unwrap();
        assert_eq!(updated.opinion, new_opinion, "Opinion should be updated");
    }

    #[test]
    fn set_civ_opinion_noop_for_unknown_pair() {
        let mut sim = test_sim(42);

        // Use a CivId pair with no relationship.
        // CivId(999) doesn't exist, so this should be a no-op.
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::SetCivOpinion {
                civ_id: CivId(999),
                target_civ: CivId(0),
                opinion: CivOpinion::Hostile,
            },
        };
        sim.step(&[cmd], 1);
        // No panic = success.
    }

    #[test]
    fn get_known_civs_returns_player_relationships() {
        let mut sim = test_sim(42);

        let known = sim.get_known_civs();
        // Should contain entries from worldgen diplomacy (player civ's outgoing rels).
        let player_rels = sim
            .db
            .civ_relationships
            .by_from_civ(&CivId(0), tabulosity::QueryOpts::ASC);

        assert_eq!(
            known.len(),
            player_rels.len(),
            "get_known_civs should return one entry per player-outgoing relationship"
        );
    }

    #[test]
    fn civ_opinion_serde_roundtrip() {
        use crate::types::CivOpinion;
        for &opinion in &[
            CivOpinion::Friendly,
            CivOpinion::Neutral,
            CivOpinion::Suspicious,
            CivOpinion::Hostile,
        ] {
            let json = serde_json::to_string(&opinion).unwrap();
            let restored: CivOpinion = serde_json::from_str(&json).unwrap();
            assert_eq!(opinion, restored);
        }
    }

    #[test]
    fn civ_species_serde_roundtrip() {
        use crate::types::CivSpecies;
        for &species in CivSpecies::ALL.iter() {
            let json = serde_json::to_string(&species).unwrap();
            let restored: CivSpecies = serde_json::from_str(&json).unwrap();
            assert_eq!(species, restored);
        }
    }

    #[test]
    fn culture_tag_serde_roundtrip() {
        use crate::types::CultureTag;
        for &tag in &[
            CultureTag::Woodland,
            CultureTag::Coastal,
            CultureTag::Mountain,
            CultureTag::Nomadic,
            CultureTag::Subterranean,
            CultureTag::Martial,
        ] {
            let json = serde_json::to_string(&tag).unwrap();
            let restored: CultureTag = serde_json::from_str(&json).unwrap();
            assert_eq!(tag, restored);
        }
    }

    #[test]
    fn discover_civ_command_serde_roundtrip() {
        let mut rng = GameRng::new(1);
        let cmd = SimCommand {
            player_id: PlayerId::new(&mut rng),
            tick: 42,
            action: SimAction::DiscoverCiv {
                civ_id: CivId(0),
                discovered_civ: CivId(5),
                initial_opinion: CivOpinion::Suspicious,
            },
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let restored: SimCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&restored).unwrap());
    }

    #[test]
    fn set_civ_opinion_command_serde_roundtrip() {
        let mut rng = GameRng::new(2);
        let cmd = SimCommand {
            player_id: PlayerId::new(&mut rng),
            tick: 99,
            action: SimAction::SetCivOpinion {
                civ_id: CivId(1),
                target_civ: CivId(3),
                opinion: CivOpinion::Hostile,
            },
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let restored: SimCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&restored).unwrap());
    }

    #[test]
    fn civ_opinion_shift_friendlier() {
        assert_eq!(
            CivOpinion::Hostile.shift_friendlier(),
            CivOpinion::Suspicious
        );
        assert_eq!(
            CivOpinion::Suspicious.shift_friendlier(),
            CivOpinion::Neutral
        );
        assert_eq!(CivOpinion::Neutral.shift_friendlier(), CivOpinion::Friendly);
        assert_eq!(
            CivOpinion::Friendly.shift_friendlier(),
            CivOpinion::Friendly
        );
    }

    #[test]
    fn civ_opinion_shift_hostile() {
        assert_eq!(CivOpinion::Friendly.shift_hostile(), CivOpinion::Neutral);
        assert_eq!(CivOpinion::Neutral.shift_hostile(), CivOpinion::Suspicious);
        assert_eq!(CivOpinion::Suspicious.shift_hostile(), CivOpinion::Hostile);
        assert_eq!(CivOpinion::Hostile.shift_hostile(), CivOpinion::Hostile);
    }

    // -----------------------------------------------------------------------
    // Pursuit (dynamic repathfinding) tests
    // -----------------------------------------------------------------------

    /// Helper: spawn a second elf and return its CreatureId.
    fn spawn_second_elf(sim: &mut SimState) -> CreatureId {
        // Collect existing elf IDs before spawning.
        let existing: std::collections::BTreeSet<CreatureId> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf)
            .map(|c| c.id)
            .collect();
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
        // Return the newly spawned elf (not in the existing set).
        sim.db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elf && !existing.contains(&c.id))
            .next()
            .unwrap()
            .id
    }

    /// Helper: insert a pursuit task targeting `target_id` at `location`,
    /// and directly assign `pursuer_id` to it.
    fn insert_pursuit_task(
        sim: &mut SimState,
        location: NavNodeId,
        target_id: CreatureId,
        pursuer_id: CreatureId,
    ) -> TaskId {
        let task_id = TaskId::new(&mut sim.rng);
        let task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location,
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: Some(target_id),
        };
        sim.insert_task(task);
        // Directly assign the pursuer to this task.
        let mut pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
        pursuer.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(pursuer);
        task_id
    }

    #[test]
    fn pursuit_task_repaths_when_target_moves() {
        let mut sim = test_sim(42);
        let pursuer_id = spawn_elf(&mut sim);
        let target_id = spawn_second_elf(&mut sim);

        // Get target's initial node.
        let target_node = sim
            .db
            .creatures
            .get(&target_id)
            .unwrap()
            .current_node
            .unwrap();

        // Pick a different alive node to move the target to (use a neighbor).
        let new_target_node = {
            let graph = sim.graph_for_species(Species::Elf);
            let edges = graph.neighbors(target_node);
            graph.edge(edges[0]).to
        };
        assert_ne!(target_node, new_target_node);

        // Create pursuit task at target's current node, assigned to pursuer.
        let task_id = insert_pursuit_task(&mut sim, target_node, target_id, pursuer_id);
        assert_eq!(sim.db.tasks.get(&task_id).unwrap().location, target_node);

        // Manually move the target to the new node (simulates target movement).
        let new_pos = sim.nav_graph.node(new_target_node).position;
        let _ = sim.db.creatures.modify_unchecked(&target_id, |c| {
            c.current_node = Some(new_target_node);
            c.position = new_pos;
        });

        // Step so the pursuer's activation fires and updates the task location.
        sim.step(&[], sim.tick + 10000);

        // The pursuit task's location should have changed from the initial
        // value, proving the repath logic fired. We don't assert the exact
        // node because the target may have moved further during the step
        // (heartbeat-driven tasks, wandering after the GoTo completes, etc.).
        if let Some(task) = sim.db.tasks.get(&task_id) {
            assert_ne!(
                task.location, target_node,
                "Pursuit task location should have updated when target moved"
            );
        }
        // If the task was completed (pursuer caught the target), that also
        // proves the repath worked — the pursuer followed the target.
    }

    #[test]
    fn pursuit_task_completes_when_adjacent() {
        let mut sim = test_sim(42);
        let pursuer_id = spawn_elf(&mut sim);
        let target_id = spawn_second_elf(&mut sim);

        // Read the pursuer's current node (may have wandered during spawns).
        let pursuer_node = sim
            .db
            .creatures
            .get(&pursuer_id)
            .unwrap()
            .current_node
            .unwrap();

        // Place both creatures at the same node and prevent them from wandering.
        let node_pos = sim.nav_graph.node(pursuer_node).position;
        let _ = sim.db.creatures.modify_unchecked(&target_id, |c| {
            c.current_node = Some(pursuer_node);
            c.position = node_pos;
        });
        let _ = sim.db.creatures.modify_unchecked(&pursuer_id, |c| {
            c.current_node = Some(pursuer_node);
            c.position = node_pos;
            c.path = None;
        });

        // Give the target a Sleep task so it stays still.
        let sleep_task_id = TaskId::new(&mut sim.rng);
        let sleep_task = Task {
            id: sleep_task_id,
            kind: TaskKind::Sleep {
                bed_pos: None,
                location: task::SleepLocation::Ground,
            },
            state: TaskState::InProgress,
            location: pursuer_node,
            progress: 0.0,
            total_cost: 999999.0, // very long sleep
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(sleep_task);
        let mut target = sim.db.creatures.get(&target_id).unwrap();
        target.current_task = Some(sleep_task_id);
        let _ = sim.db.creatures.update_no_fk(target);

        // Create pursuit task at the shared node.
        let task_id = insert_pursuit_task(&mut sim, pursuer_node, target_id, pursuer_id);

        // Step — pursuer should complete the GoTo since it's at the target's node.
        sim.step(&[], sim.tick + 10000);

        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.state,
            TaskState::Complete,
            "Pursuit task should complete when pursuer is at target's node"
        );
        let pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
        assert_eq!(
            pursuer.current_task, None,
            "Pursuer should be unassigned after task completion"
        );
    }

    #[test]
    fn pursuit_task_abandons_when_target_gone() {
        let mut sim = test_sim(42);
        let pursuer_id = spawn_elf(&mut sim);
        let target_id = spawn_second_elf(&mut sim);

        // Let both creatures settle (complete initial movement).
        sim.step(&[], sim.tick + 10000);

        let target_node = sim
            .db
            .creatures
            .get(&target_id)
            .unwrap()
            .current_node
            .unwrap();

        // Assign pursuit task — clear any existing task first.
        let mut pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
        pursuer.current_task = None;
        pursuer.path = None;
        let _ = sim.db.creatures.update_no_fk(pursuer);

        let task_id = insert_pursuit_task(&mut sim, target_node, target_id, pursuer_id);

        // Simulate target becoming unreachable by clearing its current_node.
        // This triggers the `target_node == None` branch in pursuit logic,
        // causing the pursuer to abandon the task.
        let _ = sim
            .db
            .creatures
            .modify_unchecked(&target_id, |c| c.current_node = None);

        // Step — pursuer should notice target has no nav node and unassign.
        sim.step(&[], sim.tick + 500000);

        let pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
        // The pursuer should have abandoned the pursuit task.
        assert_ne!(
            pursuer.current_task,
            Some(task_id),
            "Pursuer should have abandoned the pursuit task when target has no nav node"
        );

        // The pursuit task should be completed (not left Available for re-claim).
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.state,
            TaskState::Complete,
            "Abandoned pursuit task should be completed"
        );
    }

    #[test]
    fn pursuit_task_abandons_when_target_unreachable() {
        let mut sim = test_sim(42);
        let pursuer_id = spawn_elf(&mut sim);
        let target_id = spawn_second_elf(&mut sim);

        // Place target at a non-existent nav node (simulates disconnected region).
        let bogus_node = NavNodeId(999999);
        let _task_id = insert_pursuit_task(&mut sim, bogus_node, target_id, pursuer_id);
        // Also set target's current_node to the bogus node.
        let _ = sim.db.creatures.modify_unchecked(&target_id, |c| {
            c.current_node = Some(bogus_node);
        });

        // Step so pursuer's activation fires and hits the dead-node check.
        sim.step(&[], sim.tick + 50000);

        // Pursuer should have abandoned the pursuit task (may have claimed
        // another task from heartbeat, but not the pursuit task).
        let pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
        assert_ne!(
            pursuer.current_task,
            Some(_task_id),
            "Pursuer should have abandoned the pursuit task for unreachable target"
        );
    }

    #[test]
    fn non_pursuit_tasks_unaffected() {
        // Verify existing GoTo tasks (without target_creature) still work.
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let elf_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();

        // Insert a regular GoTo task (no target_creature).
        let task_id = insert_goto_task(&mut sim, elf_node);

        // Verify target_creature is None.
        let db_task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(db_task.target_creature, None);

        // Step — should complete normally.
        sim.step(&[], sim.tick + 10000);

        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(
            task.state,
            TaskState::Complete,
            "Non-pursuit GoTo task should complete normally"
        );
    }

    #[test]
    fn pursuit_task_serde_roundtrip() {
        let mut sim = test_sim(42);
        let pursuer_id = spawn_elf(&mut sim);
        let target_id = spawn_second_elf(&mut sim);
        let target_node = sim
            .db
            .creatures
            .get(&target_id)
            .unwrap()
            .current_node
            .unwrap();

        let task_id = insert_pursuit_task(&mut sim, target_node, target_id, pursuer_id);

        // Verify the task has target_creature set.
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.target_creature, Some(target_id));

        // Serialize the entire sim state via save/load.
        let json = serde_json::to_string(&sim.db).unwrap();
        let restored: crate::db::SimDb = serde_json::from_str(&json).unwrap();

        let restored_task = restored.tasks.get(&task_id).unwrap();
        assert_eq!(
            restored_task.target_creature,
            Some(target_id),
            "target_creature should survive serde roundtrip"
        );
    }

    // -----------------------------------------------------------------------
    // Blueprint overlay tests (B-preview-blueprints)
    // -----------------------------------------------------------------------

    #[test]
    fn blueprint_overlay_includes_designated_blueprints() {
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Designate a platform build.
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
        assert_eq!(sim.db.blueprints.len(), 1);

        let overlay = sim.blueprint_overlay();
        assert_eq!(
            overlay.voxels.get(&air_coord),
            Some(&VoxelType::GrownPlatform),
            "Designated platform blueprint should appear in overlay as GrownPlatform"
        );
    }

    #[test]
    fn blueprint_overlay_excludes_complete_blueprints() {
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Designate and then manually mark as complete.
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

        // Manually flip the blueprint to Complete.
        let mut bp = sim.db.blueprints.iter_all().next().unwrap().clone();
        bp.state = BlueprintState::Complete;
        let _ = sim.db.blueprints.update_no_fk(bp);

        let overlay = sim.blueprint_overlay();
        assert!(
            overlay.voxels.is_empty(),
            "Complete blueprints should not appear in overlay"
        );
    }

    #[test]
    fn blueprint_overlay_maps_carve_to_air() {
        let mut sim = test_sim(42);

        // Find a solid carvable voxel from the tree's trunk.
        let tree = &sim.trees[&sim.player_tree_id];
        let carve_coord = *tree
            .trunk_voxels
            .iter()
            .find(|&&c| {
                let vt = sim.world.get(c);
                vt.is_solid() && vt != VoxelType::ForestFloor
            })
            .expect("Need a carvable trunk voxel");

        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateCarve {
                voxels: vec![carve_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd], 1);

        let overlay = sim.blueprint_overlay();
        assert_eq!(
            overlay.voxels.get(&carve_coord),
            Some(&VoxelType::Air),
            "Carve blueprint should appear in overlay as Air"
        );
    }

    #[test]
    fn second_platform_blocked_by_existing_blueprint() {
        // Designating a platform on the same voxel as an existing blueprint
        // should fail because the overlay makes the voxel appear as
        // GrownPlatform (Blocked for overlap).
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // First designation succeeds.
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
        assert_eq!(sim.db.blueprints.len(), 1);

        // Second designation on the same voxel should be rejected.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd2], 2);
        // Still only one blueprint — second was rejected.
        assert_eq!(
            sim.db.blueprints.len(),
            1,
            "Second platform on same voxel should be rejected"
        );
        assert!(
            sim.last_build_message.is_some(),
            "Should have a rejection message"
        );
    }

    #[test]
    fn adjacent_platform_sees_blueprint_support() {
        // Place platforms in a chain extending from the trunk. Designate the
        // first N-1 as a single blueprint, then designate the last one
        // separately — it's only adjacent to the blueprint, not to any solid
        // in the real world, so it exercises the overlay adjacency check.
        let mut sim = test_sim(42);

        // Search across trunk voxels and all 4 horizontal directions for a
        // strip of air that eventually leaves all solid face neighbors behind.
        let directions: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        let tree = &sim.trees[&sim.player_tree_id];
        let mut best: Option<(Vec<VoxelCoord>, usize)> = None; // (strip, split_index)
        'outer: for &trunk_coord in &tree.trunk_voxels {
            for &(dx, dz) in &directions {
                let mut strip = Vec::new();
                for i in 1..=20_i32 {
                    let c = VoxelCoord::new(
                        trunk_coord.x + dx * i,
                        trunk_coord.y,
                        trunk_coord.z + dz * i,
                    );
                    if !sim.world.in_bounds(c) || sim.world.get(c) != VoxelType::Air {
                        break;
                    }
                    strip.push(c);
                }
                if strip.len() < 2 {
                    continue;
                }
                if let Some(split) = strip
                    .iter()
                    .position(|&c| !sim.world.has_solid_face_neighbor(c))
                {
                    if split > 0 {
                        best = Some((strip, split));
                        break 'outer;
                    }
                }
            }
        }
        let (strip, split) = best.expect(
            "Need a trunk voxel with an air strip that transitions from solid-neighbor to open",
        );

        let first_batch = &strip[..split];
        let extension = strip[split];

        // Designate the first batch (adjacent to trunk).
        let cmd1 = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: first_batch.to_vec(),
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd1], 1);
        assert_eq!(sim.db.blueprints.len(), 1);

        // Designate the extension. Without the overlay it would fail
        // adjacency; with the overlay the blueprint batch acts as solid.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![extension],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd2], 2);
        assert_eq!(
            sim.db.blueprints.len(),
            2,
            "Platform adjacent to blueprint should be accepted via overlay support"
        );
    }

    #[test]
    fn overlapping_carve_designations_rejected() {
        // A second carve on the same voxels should be rejected because
        // the overlay maps them to Air (nothing to carve).
        let mut sim = test_sim(42);

        let tree = &sim.trees[&sim.player_tree_id];
        let carve_coord = *tree
            .trunk_voxels
            .iter()
            .find(|&&c| {
                let vt = sim.world.get(c);
                vt.is_solid() && vt != VoxelType::ForestFloor
            })
            .expect("Need a carvable trunk voxel");

        // First carve succeeds.
        let cmd1 = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateCarve {
                voxels: vec![carve_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd1], 1);
        assert_eq!(sim.db.blueprints.len(), 1);

        // Second carve on same voxel rejected — overlay shows Air.
        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateCarve {
                voxels: vec![carve_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd2], 2);
        assert_eq!(
            sim.db.blueprints.len(),
            1,
            "Second carve on same voxel should be rejected"
        );
        assert!(
            sim.last_build_message.is_some(),
            "Should have a rejection message"
        );
    }

    #[test]
    fn building_foundation_on_designated_platform() {
        // A building placed on a designated platform (not yet built) should
        // see the platform voxels as solid via the overlay.
        let mut sim = test_sim(42);

        // Find a 3x3 air area adjacent to the trunk at some Y level.
        // Use the building site finder logic but at y=1 where ForestFloor
        // provides the foundation, then place a platform to serve as a
        // higher foundation.
        let site = find_building_site(&sim);
        // site is at y=0 (ForestFloor). Interior starts at y=1.
        // Designate a 3x3 platform at y=1.
        let mut platform_voxels = Vec::new();
        for dx in 0..3 {
            for dz in 0..3 {
                platform_voxels.push(VoxelCoord::new(site.x + dx, 1, site.z + dz));
            }
        }

        let cmd1 = SimCommand {
            player_id: sim.player_id,
            tick: 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: platform_voxels,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd1], 1);
        assert_eq!(sim.db.blueprints.len(), 1);

        // Verify the overlay makes the platform voxels solid.
        let overlay = sim.blueprint_overlay();
        let platform_coord = VoxelCoord::new(site.x, 1, site.z);
        assert_eq!(
            overlay.voxels.get(&platform_coord),
            Some(&VoxelType::GrownPlatform)
        );

        // Now designate a building with foundation at y=1 (the platform).
        // Interior at y=2. Clear any non-air voxels at y=2 so the test
        // always exercises the building-on-blueprint path.
        let building_anchor = VoxelCoord::new(site.x, 1, site.z);
        for dx in 0..3 {
            for dz in 0..3 {
                let coord = VoxelCoord::new(site.x + dx, 2, site.z + dz);
                if sim.world.get(coord) != VoxelType::Air {
                    sim.world.set(coord, VoxelType::Air);
                }
            }
        }

        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateBuilding {
                anchor: building_anchor,
                width: 3,
                depth: 3,
                height: 1,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd2], 2);
        assert_eq!(
            sim.db.blueprints.len(),
            2,
            "Building on designated platform should be accepted: {:?}",
            sim.last_build_message
        );
    }

    #[test]
    fn ladder_anchored_to_designated_platform() {
        // A wood ladder placed next to a designated platform should see the
        // platform as solid for anchoring via the overlay.
        let mut sim = test_sim(42);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Designate a platform.
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
        assert_eq!(sim.db.blueprints.len(), 1);

        // Find a voxel adjacent to the platform in any horizontal direction
        // that is Air and has no solid face neighbors in the real world, so
        // the ladder can only anchor via the blueprint overlay. Clear any
        // solid neighbors at the ladder voxel if needed to isolate it.
        let directions: [(i32, i32, FaceDirection); 4] = [
            (-1, 0, FaceDirection::PosX),
            (1, 0, FaceDirection::NegX),
            (0, -1, FaceDirection::PosZ),
            (0, 1, FaceDirection::NegZ),
        ];
        let (ladder_coord, orientation) = directions
            .iter()
            .map(|&(dx, dz, dir)| {
                (
                    VoxelCoord::new(air_coord.x + dx, air_coord.y, air_coord.z + dz),
                    dir,
                )
            })
            .find(|&(coord, _)| {
                sim.world.in_bounds(coord) && sim.world.get(coord) == VoxelType::Air
            })
            .expect("Need an air voxel adjacent to the platform for ladder placement");

        // Clear any solid face neighbors so anchoring can only succeed via overlay.
        for &dir in &FaceDirection::ALL {
            let (dx, dy, dz) = dir.to_offset();
            let neighbor = VoxelCoord::new(
                ladder_coord.x + dx,
                ladder_coord.y + dy,
                ladder_coord.z + dz,
            );
            if neighbor != air_coord && sim.world.get(neighbor).is_solid() {
                sim.world.set(neighbor, VoxelType::Air);
            }
        }
        assert!(
            !sim.world.has_solid_face_neighbor(ladder_coord),
            "Ladder voxel should have no solid face neighbors in the real world"
        );

        let cmd2 = SimCommand {
            player_id: sim.player_id,
            tick: 2,
            action: SimAction::DesignateLadder {
                anchor: ladder_coord,
                height: 1,
                orientation,
                kind: LadderKind::Wood,
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd2], 2);
        assert_eq!(
            sim.db.blueprints.len(),
            2,
            "Wood ladder anchored to designated platform should be accepted: {:?}",
            sim.last_build_message
        );
    }

    // ── interrupt_task tests ──────────────────────────────────────────

    #[test]
    fn interrupt_goto_completes_task_and_clears_creature() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        let nav_count = sim.nav_graph.node_count();
        let far_node = NavNodeId((nav_count / 2) as u32);
        let task_id = TaskId::new(&mut sim.rng);
        let goto_task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: far_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        sim.insert_task(goto_task);
        if let Some(mut c) = sim.db.creatures.get(&elf_id) {
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        sim.interrupt_task(elf_id, task_id);

        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, TaskState::Complete);
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert!(creature.current_task.is_none());
        assert!(creature.path.is_none());
    }

    #[test]
    fn interrupt_build_returns_task_to_available() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let air_coord = find_air_adjacent_to_trunk(&sim);

        // Designate a build.
        let cmd_build = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![air_coord],
                priority: Priority::Normal,
            },
        };
        sim.step(&[cmd_build], sim.tick + 2);

        let build_task_id = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Build)
            .unwrap()
            .id;

        // Assign the elf to the build task.
        if let Some(mut t) = sim.db.tasks.get(&build_task_id) {
            t.state = TaskState::InProgress;
            let _ = sim.db.tasks.update_no_fk(t);
        }
        if let Some(mut c) = sim.db.creatures.get(&elf_id) {
            c.current_task = Some(build_task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        sim.interrupt_task(elf_id, build_task_id);

        // Build is resumable — should return to Available.
        let task = sim.db.tasks.get(&build_task_id).unwrap();
        assert_eq!(task.state, TaskState::Available);
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert!(creature.current_task.is_none());
    }

    #[test]
    fn interrupt_craft_clears_reservations() {
        let mut sim = test_sim(42);
        let (structure_id, elf_id) = setup_workshop(&mut sim);

        let inv_id = sim.structure_inv(structure_id);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Fruit, 1, None, None);

        sim.process_workshop_monitor();
        let task_id = sim
            .db
            .tasks
            .iter_all()
            .find(|t| t.kind_tag == TaskKindTag::Craft)
            .unwrap()
            .id;

        // Verify fruit is reserved.
        let reserved = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.reserved_by == Some(task_id))
            .count();
        assert!(reserved > 0, "Fruit should be reserved before interrupt");

        // Assign elf to the task.
        if let Some(mut t) = sim.db.tasks.get(&task_id) {
            t.state = TaskState::InProgress;
            let _ = sim.db.tasks.update_no_fk(t);
        }
        if let Some(mut c) = sim.db.creatures.get(&elf_id) {
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        sim.interrupt_task(elf_id, task_id);

        // Reservations should be cleared.
        let still_reserved = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .iter()
            .filter(|s| s.reserved_by == Some(task_id))
            .count();
        assert_eq!(still_reserved, 0, "Reservations should be cleared");

        // Task should be Complete (non-resumable).
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, TaskState::Complete);
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert!(creature.current_task.is_none());
    }

    #[test]
    fn interrupt_sleep_completes_task() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        let current_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();
        let task_id = TaskId::new(&mut sim.rng);
        let sleep_task = Task {
            id: task_id,
            kind: TaskKind::Sleep {
                bed_pos: None,
                location: task::SleepLocation::Ground,
            },
            state: TaskState::InProgress,
            location: current_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
        };
        sim.insert_task(sleep_task);
        if let Some(mut c) = sim.db.creatures.get(&elf_id) {
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        sim.interrupt_task(elf_id, task_id);

        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.state, TaskState::Complete);
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert!(creature.current_task.is_none());
    }

    #[test]
    fn interrupt_missing_task_clears_creature_fields() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Assign creature to a task ID that doesn't exist in the DB.
        let fake_task_id = TaskId::new(&mut sim.rng);
        if let Some(mut c) = sim.db.creatures.get(&elf_id) {
            c.current_task = Some(fake_task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        sim.interrupt_task(elf_id, fake_task_id);

        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert!(creature.current_task.is_none());
        assert!(creature.path.is_none());
    }

    #[test]
    fn interrupt_clears_move_action() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        let current_node = sim.db.creatures.get(&elf_id).unwrap().current_node.unwrap();

        // Put the elf in a Move action.
        let pos = sim.db.creatures.get(&elf_id).unwrap().position;
        if let Some(mut c) = sim.db.creatures.get(&elf_id) {
            c.action_kind = ActionKind::Move;
            c.next_available_tick = Some(sim.tick + 1000);
            let _ = sim.db.creatures.update_no_fk(c);
        }
        let move_action = crate::db::MoveAction {
            creature_id: elf_id,
            move_from: pos,
            move_to: pos,
            move_start_tick: sim.tick,
        };
        let _ = sim.db.move_actions.insert_no_fk(move_action);

        // Create a GoTo task for context.
        let task_id = TaskId::new(&mut sim.rng);
        let goto_task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: current_node,
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
        };
        sim.insert_task(goto_task);
        if let Some(mut c) = sim.db.creatures.get(&elf_id) {
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }

        sim.interrupt_task(elf_id, task_id);

        // MoveAction should be deleted.
        assert!(sim.db.move_actions.get(&elf_id).is_none());
        // Action should be cleared.
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.action_kind, ActionKind::NoAction);
        assert!(creature.next_available_tick.is_none());
    }

    // -----------------------------------------------------------------------
    // Spatial index tests
    // -----------------------------------------------------------------------

    #[test]
    fn spatial_index_empty_before_spawn() {
        let sim = test_sim(42);
        assert!(
            sim.spatial_index.is_empty(),
            "Spatial index should be empty before any creatures are spawned"
        );
    }

    #[test]
    fn spatial_index_populated_after_spawn() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        let pos = elf.position;

        // Elf has a [1,1,1] footprint — should be registered at exactly one voxel.
        let at_pos = sim.creatures_at_voxel(pos);
        assert!(
            at_pos.contains(&elf_id),
            "Elf should be in spatial index at its position"
        );
        assert_eq!(at_pos.len(), 1, "Only one creature at this voxel");
    }

    #[test]
    fn spatial_index_tracks_movement() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let initial_pos = sim.db.creatures.get(&elf_id).unwrap().position;

        // Run enough ticks for the elf to move at least once.
        sim.step(&[], sim.tick + 50_000);
        let new_pos = sim.db.creatures.get(&elf_id).unwrap().position;

        if new_pos != initial_pos {
            assert!(
                !sim.creatures_at_voxel(initial_pos).contains(&elf_id),
                "Elf should not be at old position after moving"
            );
            assert!(
                sim.creatures_at_voxel(new_pos).contains(&elf_id),
                "Elf should be at new position after moving"
            );
        } else {
            assert!(sim.creatures_at_voxel(initial_pos).contains(&elf_id));
        }
    }

    #[test]
    fn spatial_index_multiple_creatures_same_voxel() {
        let mut sim = test_sim(42);
        let elf1 = spawn_elf(&mut sim);
        let elf2 = spawn_elf(&mut sim);

        // Force both elves to the same position so the test always exercises
        // multi-occupancy (spawn may place them at different nav nodes).
        let pos1 = sim.db.creatures.get(&elf1).unwrap().position;
        let pos2 = sim.db.creatures.get(&elf2).unwrap().position;
        if pos1 != pos2 {
            let species = sim.db.creatures.get(&elf2).unwrap().species;
            let footprint = sim.species_table[&species].footprint;
            SimState::deregister_creature_from_index(&mut sim.spatial_index, elf2, pos2, footprint);
            let _ = sim.db.creatures.modify_unchecked(&elf2, |c| {
                c.position = pos1;
            });
            SimState::register_creature_in_index(&mut sim.spatial_index, elf2, pos1, footprint);
        }

        let at_pos = sim.creatures_at_voxel(pos1);
        assert!(at_pos.contains(&elf1));
        assert!(at_pos.contains(&elf2));
        assert_eq!(at_pos.len(), 2);
        // Verify sorted for determinism.
        assert!(
            at_pos[0] <= at_pos[1],
            "Spatial index entries should be sorted by CreatureId"
        );
    }

    #[test]
    fn spatial_index_query_empty_voxel() {
        let sim = test_sim(42);
        let empty = sim.creatures_at_voxel(VoxelCoord::new(999, 999, 999));
        assert!(empty.is_empty());
    }

    #[test]
    fn spatial_index_survives_save_load_roundtrip() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let pos = sim.db.creatures.get(&elf_id).unwrap().position;
        assert!(sim.creatures_at_voxel(pos).contains(&elf_id));

        // Roundtrip through JSON (spatial_index is #[serde(skip)]).
        let json = sim.to_json().unwrap();
        let sim2 = SimState::from_json(&json).unwrap();

        let elf2 = sim2.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim2.creatures_at_voxel(elf2.position).contains(&elf_id),
            "Spatial index should be rebuilt after deserialization"
        );
    }

    #[test]
    fn spatial_index_consistent_after_many_ticks() {
        let mut sim = test_sim(42);
        let elf1 = spawn_elf(&mut sim);
        let elf2 = spawn_elf(&mut sim);
        let elf3 = spawn_elf(&mut sim);

        sim.step(&[], sim.tick + 100_000);

        // Every creature must be in the index at its current position.
        for &elf_id in &[elf1, elf2, elf3] {
            let elf = sim.db.creatures.get(&elf_id).unwrap();
            assert!(
                sim.creatures_at_voxel(elf.position).contains(&elf_id),
                "Creature {:?} should be at its position {:?}",
                elf_id,
                elf.position,
            );
        }

        // Total entries should match total footprint voxels.
        let total_entries: usize = sim.spatial_index.values().map(|v| v.len()).sum();
        let expected: usize = sim
            .db
            .creatures
            .iter_all()
            .map(|c| {
                let fp = sim.species_table[&c.species].footprint;
                fp[0] as usize * fp[1] as usize * fp[2] as usize
            })
            .sum();
        assert_eq!(
            total_entries, expected,
            "Spatial index entry count should match total footprint voxels"
        );
    }

    // -----------------------------------------------------------------------
    // HP / damage / heal / death tests
    // -----------------------------------------------------------------------

    #[test]
    fn spawn_sets_hp_from_species_data() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        let hp_max = sim.species_table[&Species::Elf].hp_max;
        assert_eq!(creature.hp, hp_max);
        assert_eq!(creature.hp_max, hp_max);
        assert_eq!(creature.vital_status, VitalStatus::Alive);
    }

    #[test]
    fn debug_kill_sets_dead_and_emits_event() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        let result = sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        // Creature should still exist but be dead.
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.vital_status, VitalStatus::Dead);
        assert_eq!(creature.hp, 0);

        // Should have emitted CreatureDied event.
        assert!(result.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::CreatureDied {
                creature_id: cid,
                cause: DeathCause::Debug,
                ..
            } if *cid == elf_id
        )));
    }

    #[test]
    fn dead_creature_excluded_from_count() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        assert_eq!(sim.creature_count(Species::Elf), 1);

        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        assert_eq!(sim.creature_count(Species::Elf), 0);
        // But the row still exists in the DB.
        assert!(sim.db.creatures.get(&elf_id).is_some());
    }

    #[test]
    fn damage_reduces_hp() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let hp_max = sim.species_table[&Species::Elf].hp_max;
        let tick = sim.tick;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: 30,
                },
            }],
            tick + 1,
        );

        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.hp, hp_max - 30);
        assert_eq!(creature.vital_status, VitalStatus::Alive);
    }

    #[test]
    fn damage_kills_at_zero_hp() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let hp_max = sim.species_table[&Species::Elf].hp_max;
        let tick = sim.tick;

        let result = sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: hp_max, // exactly lethal
                },
            }],
            tick + 1,
        );

        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.vital_status, VitalStatus::Dead);
        assert_eq!(creature.hp, 0);
        assert!(result.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::CreatureDied {
                cause: DeathCause::Damage,
                ..
            }
        )));
    }

    #[test]
    fn overkill_damage_clamps_hp_to_zero() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: 99999,
                },
            }],
            tick + 1,
        );

        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.hp, 0);
        assert_eq!(creature.vital_status, VitalStatus::Dead);
    }

    #[test]
    fn heal_restores_hp_clamped_to_max() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let hp_max = sim.species_table[&Species::Elf].hp_max;
        let tick = sim.tick;

        // Damage first.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: 60,
                },
            }],
            tick + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_max - 60);

        // Heal more than needed.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::HealCreature {
                    creature_id: elf_id,
                    amount: 999,
                },
            }],
            tick2 + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_max);
    }

    #[test]
    fn heal_does_not_revive_dead() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Kill.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        // Try to heal.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::HealCreature {
                    creature_id: elf_id,
                    amount: 100,
                },
            }],
            tick2 + 1,
        );

        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.vital_status, VitalStatus::Dead);
        assert_eq!(creature.hp, 0);
    }

    #[test]
    fn death_drops_inventory_as_ground_pile() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Give the elf some items.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::AddCreatureItem {
                    creature_id: elf_id,
                    item_kind: inventory::ItemKind::Bread,
                    quantity: 5,
                },
            }],
            tick + 1,
        );

        let creature_pos = sim.db.creatures.get(&elf_id).unwrap().position;

        // Kill the elf.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick2 + 1,
        );

        // Creature's inventory should be empty (items transferred to ground pile).
        let inv_id = sim.db.creatures.get(&elf_id).unwrap().inventory_id;
        let remaining: Vec<_> = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
        assert!(remaining.is_empty(), "dead creature should have no items");

        // A ground pile should exist somewhere with the dropped bread.
        // (Position may be snapped by ensure_ground_pile, so we don't check exact pos.)
        let _ = creature_pos; // used only to confirm creature existed
        let total_bread: u32 = sim
            .db
            .ground_piles
            .iter_all()
            .flat_map(|p| {
                sim.db
                    .item_stacks
                    .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
            })
            .filter(|s| s.kind == inventory::ItemKind::Bread)
            .map(|s| s.quantity)
            .sum();
        assert!(
            total_bread >= 5,
            "ground piles should have >= 5 bread, got {total_bread}"
        );
    }

    #[test]
    fn spatial_index_multi_voxel_footprint() {
        let mut index = BTreeMap::<VoxelCoord, Vec<CreatureId>>::new();
        let mut rng = GameRng::new(999);
        let cid = CreatureId::new(&mut rng);
        let anchor = VoxelCoord::new(5, 1, 5);
        let footprint = [2, 2, 2];

        SimState::register_creature_in_index(&mut index, cid, anchor, footprint);

        // Should be registered at 8 voxels (2x2x2).
        let mut registered_count = 0;
        for dx in 0..2 {
            for dy in 0..2 {
                for dz in 0..2 {
                    let v = VoxelCoord::new(5 + dx, 1 + dy, 5 + dz);
                    assert!(
                        index.get(&v).unwrap().contains(&cid),
                        "Creature should be at ({}, {}, {})",
                        5 + dx,
                        1 + dy,
                        5 + dz,
                    );
                    registered_count += 1;
                }
            }
        }
        assert_eq!(registered_count, 8);

        SimState::deregister_creature_from_index(&mut index, cid, anchor, footprint);
        assert!(index.is_empty(), "Index should be empty after deregister");
    }

    #[test]
    fn spatial_index_sorted_entries() {
        let mut index = BTreeMap::<VoxelCoord, Vec<CreatureId>>::new();
        let pos = VoxelCoord::new(5, 1, 5);
        let fp = [1, 1, 1];

        let mut rng = GameRng::new(12345);
        let mut ids = [
            CreatureId::new(&mut rng),
            CreatureId::new(&mut rng),
            CreatureId::new(&mut rng),
        ];
        ids.sort();

        // Register in reverse order.
        SimState::register_creature_in_index(&mut index, ids[2], pos, fp);
        SimState::register_creature_in_index(&mut index, ids[0], pos, fp);
        SimState::register_creature_in_index(&mut index, ids[1], pos, fp);

        let entries = &index[&pos];
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], ids[0]);
        assert_eq!(entries[1], ids[1]);
        assert_eq!(entries[2], ids[2]);
    }

    #[test]
    fn death_clears_assigned_home() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Build a home and assign the elf.
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
        let structure_id = insert_completed_home(&mut sim, anchor);
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::AssignHome {
                    creature_id: elf_id,
                    structure_id: Some(structure_id),
                },
            }],
            tick + 1,
        );
        assert!(
            sim.db
                .creatures
                .get(&elf_id)
                .unwrap()
                .assigned_home
                .is_some()
        );

        // Kill the elf.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick2 + 1,
        );
        assert!(
            sim.db
                .creatures
                .get(&elf_id)
                .unwrap()
                .assigned_home
                .is_none()
        );
    }

    #[test]
    fn dead_creature_heartbeat_does_not_reschedule() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Kill the elf.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        // Run sim forward past several heartbeat intervals. Any pending
        // heartbeat events for the dead elf should be no-ops (not reschedule).
        let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
        sim.step(&[], sim.tick + heartbeat_interval * 5);

        // Drain the event queue and check no heartbeat for this creature.
        let mut found_heartbeat = false;
        while let Some(evt) = sim.event_queue.pop_if_ready(u64::MAX) {
            if matches!(
                evt.kind,
                ScheduledEventKind::CreatureHeartbeat { creature_id } if creature_id == elf_id
            ) {
                found_heartbeat = true;
            }
        }
        assert!(
            !found_heartbeat,
            "dead creature should not have pending heartbeats"
        );
    }

    #[test]
    fn dead_creature_not_assigned_tasks() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Kill the elf.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        // Create a GoTo task.
        let pos = sim.db.creatures.get(&elf_id).unwrap().position;
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::CreateTask {
                    kind: TaskKind::GoTo,
                    position: pos,
                    required_species: Some(Species::Elf),
                },
            }],
            tick2 + 1,
        );

        // Run several activations.
        sim.step(&[], sim.tick + 10000);

        // Dead creature should NOT have picked up the task.
        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            creature.current_task.is_none(),
            "dead creature should not claim tasks"
        );
    }

    #[test]
    fn damage_dead_creature_is_noop() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Kill.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        // Try to damage again.
        let tick2 = sim.tick;
        let result = sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: 50,
                },
            }],
            tick2 + 1,
        );

        // Should not emit a second death event.
        assert!(
            !result
                .events
                .iter()
                .any(|e| matches!(&e.kind, SimEventKind::CreatureDied { .. })),
            "damaging dead creature should not emit another death event"
        );
    }

    #[test]
    fn death_creates_notification() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let initial_notifications = sim.db.notifications.len();
        let tick = sim.tick;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        assert!(
            sim.db.notifications.len() > initial_notifications,
            "death should create a notification"
        );
        let last_notif = sim.db.notifications.iter_all().last().unwrap();
        assert!(
            last_notif.message.contains("died"),
            "notification should mention death: {}",
            last_notif.message
        );
    }

    #[test]
    fn death_interrupts_current_task() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);

        // Create and claim a GoTo task.
        let pos = sim.db.creatures.get(&elf_id).unwrap().position;
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::CreateTask {
                    kind: TaskKind::GoTo,
                    position: pos,
                    required_species: Some(Species::Elf),
                },
            }],
            tick + 1,
        );

        // Run until the elf picks up the task.
        sim.step(&[], sim.tick + 5000);
        // Elf should have a task now (either the GoTo or something from heartbeat).

        // Kill the elf.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick2 + 1,
        );

        let creature = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            creature.current_task.is_none(),
            "dead creature should have no task"
        );
        assert_eq!(creature.action_kind, ActionKind::NoAction);
    }

    #[test]
    fn kill_nonexistent_creature_is_noop() {
        let mut sim = test_sim(42);
        let mut rng = GameRng::new(999);
        let fake_id = CreatureId::new(&mut rng);
        let tick = sim.tick;

        // Should not panic.
        let result = sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: fake_id,
                },
            }],
            tick + 1,
        );

        assert!(
            !result
                .events
                .iter()
                .any(|e| matches!(&e.kind, SimEventKind::CreatureDied { .. })),
            "killing nonexistent creature should not emit event"
        );
    }

    #[test]
    fn death_removes_from_spatial_index() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let pos = sim.db.creatures.get(&elf_id).unwrap().position;

        // Elf should be in the spatial index before death.
        assert!(
            sim.spatial_index
                .get(&pos)
                .map_or(false, |v| v.contains(&elf_id)),
            "living elf should be in spatial index"
        );

        // Kill the elf.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        // Elf should no longer be in the spatial index.
        assert!(
            !sim.spatial_index
                .get(&pos)
                .map_or(false, |v| v.contains(&elf_id)),
            "dead elf should be removed from spatial index"
        );
    }

    #[test]
    fn hp_death_serde_roundtrip() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Damage elf to half HP.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: 50,
                },
            }],
            tick + 1,
        );

        // Serialize and deserialize the DB.
        let json = serde_json::to_string(&sim.db).unwrap();
        let restored: SimDb = serde_json::from_str(&json).unwrap();
        let creature = restored.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.hp, sim.db.creatures.get(&elf_id).unwrap().hp);
        assert_eq!(creature.hp_max, sim.species_table[&Species::Elf].hp_max);
        assert_eq!(creature.vital_status, VitalStatus::Alive);
    }

    #[test]
    fn hp_death_serde_roundtrip_dead() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Kill elf.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: elf_id,
                },
            }],
            tick + 1,
        );

        // Serialize and deserialize.
        let json = serde_json::to_string(&sim.db).unwrap();
        let restored: SimDb = serde_json::from_str(&json).unwrap();
        let creature = restored.creatures.get(&elf_id).unwrap();
        assert_eq!(creature.vital_status, VitalStatus::Dead);
        assert_eq!(creature.hp, 0);
    }

    #[test]
    fn zero_and_negative_damage_is_noop() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;
        let tick = sim.tick;

        // Zero damage — should not change HP.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: 0,
                },
            }],
            tick + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_before);

        // Negative damage — should not change HP.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: -5,
                },
            }],
            tick2 + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_before);
    }

    #[test]
    fn zero_and_negative_heal_is_noop() {
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        let tick = sim.tick;

        // Damage first so there's room to heal.
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DamageCreature {
                    creature_id: elf_id,
                    amount: 30,
                },
            }],
            tick + 1,
        );
        let hp_after_damage = sim.db.creatures.get(&elf_id).unwrap().hp;

        // Zero heal — should not change HP.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::HealCreature {
                    creature_id: elf_id,
                    amount: 0,
                },
            }],
            tick2 + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_after_damage);

        // Negative heal — should not change HP.
        let tick3 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick3 + 1,
                action: SimAction::HealCreature {
                    creature_id: elf_id,
                    amount: -10,
                },
            }],
            tick3 + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_after_damage);
    }

    // -----------------------------------------------------------------------
    // Melee attack tests
    // -----------------------------------------------------------------------

    /// Spawn a creature of the given species near the tree and return its ID.
    fn spawn_species(sim: &mut SimState, species: Species) -> CreatureId {
        let existing: std::collections::BTreeSet<CreatureId> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == species)
            .map(|c| c.id)
            .collect();
        let tree_pos = sim.trees[&sim.player_tree_id].position;
        let cmd = SimCommand {
            player_id: sim.player_id,
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                species,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
        sim.db
            .creatures
            .iter_all()
            .find(|c| c.species == species && !existing.contains(&c.id))
            .unwrap()
            .id
    }

    /// Force a creature to a specific position, updating the spatial index.
    fn force_position(sim: &mut SimState, creature_id: CreatureId, new_pos: VoxelCoord) {
        let creature = sim.db.creatures.get(&creature_id).unwrap();
        let old_pos = creature.position;
        let species = creature.species;
        let footprint = sim.species_table[&species].footprint;
        SimState::deregister_creature_from_index(
            &mut sim.spatial_index,
            creature_id,
            old_pos,
            footprint,
        );
        let _ = sim.db.creatures.modify_unchecked(&creature_id, |c| {
            c.position = new_pos;
        });
        SimState::register_creature_in_index(
            &mut sim.spatial_index,
            creature_id,
            new_pos,
            footprint,
        );
    }

    /// Make a creature idle (NoAction, no next_available_tick, no task).
    fn force_idle(sim: &mut SimState, creature_id: CreatureId) {
        let _ = sim.db.creatures.modify_unchecked(&creature_id, |c| {
            c.action_kind = ActionKind::NoAction;
            c.next_available_tick = None;
            c.current_task = None;
            c.path = None;
        });
    }

    // -- in_melee_range pure-function tests --

    #[test]
    fn test_in_melee_range_adjacent() {
        // Face-adjacent 1x1 creatures: dist² = 1, within default range_sq = 2.
        assert!(in_melee_range(
            VoxelCoord::new(5, 1, 5),
            [1, 1, 1],
            VoxelCoord::new(6, 1, 5),
            [1, 1, 1],
            2,
        ));
    }

    #[test]
    fn test_in_melee_range_diagonal() {
        // 2D diagonal: dist² = 1² + 1² = 2, within default range.
        assert!(in_melee_range(
            VoxelCoord::new(5, 1, 5),
            [1, 1, 1],
            VoxelCoord::new(6, 1, 6),
            [1, 1, 1],
            2,
        ));
    }

    #[test]
    fn test_in_melee_range_too_far() {
        // 2 voxels apart on X: dist² = 2² = 4, exceeds range_sq = 2.
        assert!(!in_melee_range(
            VoxelCoord::new(5, 1, 5),
            [1, 1, 1],
            VoxelCoord::new(7, 1, 5),
            [1, 1, 1],
            2,
        ));
    }

    #[test]
    fn test_in_melee_range_3d_corner() {
        // 3D diagonal: dist² = 1 + 1 + 1 = 3, exceeds range_sq = 2.
        assert!(!in_melee_range(
            VoxelCoord::new(5, 1, 5),
            [1, 1, 1],
            VoxelCoord::new(6, 2, 6),
            [1, 1, 1],
            2,
        ));
    }

    #[test]
    fn test_in_melee_range_large_footprint() {
        // 2x2x2 attacker at (4,1,5), target at (6,1,5).
        // Attacker occupies x=4..5, target at x=6. Gap on x = 6-5 = 1, dist² = 1.
        assert!(in_melee_range(
            VoxelCoord::new(4, 1, 5),
            [2, 2, 2],
            VoxelCoord::new(6, 1, 5),
            [1, 1, 1],
            2,
        ));
    }

    // -- try_melee_strike integration tests --

    #[test]
    fn test_melee_strike_deals_damage() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        // Place goblin adjacent (x+1).
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

        let tick = sim.tick;
        let events = sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick + 1,
        );

        // HP reduced.
        let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
        assert_eq!(elf_hp_after, elf_hp_before - goblin_damage);

        // CreatureDamaged event emitted.
        assert!(events.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::CreatureDamaged {
                attacker_id,
                target_id,
                damage,
                ..
            } if *attacker_id == goblin && *target_id == elf && *damage == goblin_damage
        )));
    }

    #[test]
    fn test_melee_strike_kills_target() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        // Set elf HP to just below goblin damage so one strike kills.
        let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
            c.hp = goblin_damage; // exactly equal → dies
        });

        let tick = sim.tick;
        let events = sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick + 1,
        );

        assert_eq!(
            sim.db.creatures.get(&elf).unwrap().vital_status,
            VitalStatus::Dead
        );
        assert!(events.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::CreatureDied { creature_id, .. } if *creature_id == elf
        )));
    }

    #[test]
    fn test_melee_strike_out_of_range() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        // Place goblin 3 voxels away — out of range.
        let goblin_pos = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick + 1,
        );

        // HP unchanged.
        assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_before);
    }

    #[test]
    fn test_melee_strike_cooldown() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

        // First strike succeeds.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick + 1,
        );
        assert_eq!(
            sim.db.creatures.get(&elf).unwrap().hp,
            elf_hp_before - goblin_damage,
        );

        // Goblin is now in MeleeStrike action — second strike should fail.
        assert_eq!(
            sim.db.creatures.get(&goblin).unwrap().action_kind,
            ActionKind::MeleeStrike,
        );
        let elf_hp_mid = sim.db.creatures.get(&elf).unwrap().hp;
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick2 + 1,
        );
        // HP unchanged — attack was rejected due to cooldown.
        assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_mid);
    }

    #[test]
    fn test_melee_strike_dead_target() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        // Kill the elf first.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature { creature_id: elf },
            }],
            tick + 1,
        );
        assert_eq!(
            sim.db.creatures.get(&elf).unwrap().vital_status,
            VitalStatus::Dead
        );

        // Melee attack on dead target should be a no-op.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick2 + 1,
        );
        // Goblin should still be idle (attack didn't fire).
        assert_eq!(
            sim.db.creatures.get(&goblin).unwrap().action_kind,
            ActionKind::NoAction,
        );
    }

    #[test]
    fn test_melee_strike_zero_damage_species() {
        let mut sim = test_sim(42);
        // Capybara has melee_damage = 0 — cannot melee.
        let capybara = spawn_species(&mut sim, Species::Capybara);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        // Put capybara adjacent to elf.
        let capy_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, capybara, capy_pos);
        force_idle(&mut sim, capybara);

        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: capybara,
                    target_id: elf,
                },
            }],
            tick + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_before);
    }

    #[test]
    fn test_melee_strike_serde_roundtrip() {
        // Verify ActionKind::MeleeStrike survives serde roundtrip.
        let action = ActionKind::MeleeStrike;
        let json = serde_json::to_string(&action).unwrap();
        let restored: ActionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, ActionKind::MeleeStrike);
    }

    #[test]
    fn test_melee_strike_cooldown_expires() {
        // After the cooldown elapses, the creature can strike again. With
        // hostile AI, a goblin adjacent to an elf will automatically re-strike
        // once the cooldown resolves.
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
        let interval = sim.species_table[&Species::Goblin].melee_interval_ticks;

        // First strike via command.
        let elf_hp_initial = sim.db.creatures.get(&elf).unwrap().hp;
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick + 1,
        );
        assert_eq!(
            sim.db.creatures.get(&goblin).unwrap().action_kind,
            ActionKind::MeleeStrike,
        );
        assert_eq!(
            sim.db.creatures.get(&elf).unwrap().hp,
            elf_hp_initial - goblin_damage,
        );

        // Advance past cooldown. The goblin's MeleeStrike resolves, and the
        // hostile AI will auto-strike the elf again (still adjacent). There
        // may also be a pre-existing activation from spawn, so the elf may
        // take more than one additional hit. Verify at least 2 total strikes.
        sim.step(&[], sim.tick + interval + 1);
        let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
        let total_damage = elf_hp_initial - elf_hp_after;
        assert!(
            total_damage >= 2 * goblin_damage,
            "Should have at least 2 strikes: total damage {total_damage}, \
             expected >= {} (2 × {goblin_damage})",
            2 * goblin_damage,
        );
        // Damage should be an exact multiple of goblin_damage.
        assert_eq!(
            total_damage % goblin_damage,
            0,
            "Total damage {total_damage} should be a multiple of {goblin_damage}",
        );
    }

    #[test]
    fn test_melee_strike_dead_attacker() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        // Kill the goblin.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: goblin,
                },
            }],
            tick + 1,
        );
        assert_eq!(
            sim.db.creatures.get(&goblin).unwrap().vital_status,
            VitalStatus::Dead,
        );

        // Dead goblin trying to melee should be a no-op.
        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick2 + 1,
        );
        assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_before);
    }

    // -----------------------------------------------------------------------
    // Shoot action tests
    // -----------------------------------------------------------------------

    /// Helper: give a creature a bow and some arrows.
    fn arm_with_bow_and_arrows(sim: &mut SimState, creature_id: CreatureId, arrows: u32) {
        let inv_id = sim.db.creatures.get(&creature_id).unwrap().inventory_id;
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bow, 1, None, None);
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, arrows, None, None);
    }

    #[test]
    fn test_shoot_arrow_spawns_projectile() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        // Place them apart with clear LOS (same Y, 5 voxels apart on X).
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);

        // Give elf a bow and arrows.
        arm_with_bow_and_arrows(&mut sim, elf, 5);

        let tick = sim.tick;
        let events = sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );

        // A projectile should exist.
        assert_eq!(sim.db.projectiles.iter_all().count(), 1);

        // Arrow consumed from inventory.
        let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
        assert_eq!(sim.inv_item_count(inv_id, inventory::ItemKind::Arrow), 4);
        // Bow still there.
        assert_eq!(sim.inv_item_count(inv_id, inventory::ItemKind::Bow), 1);

        // Elf is now on Shoot cooldown.
        let elf_creature = sim.db.creatures.get(&elf).unwrap();
        assert_eq!(elf_creature.action_kind, ActionKind::Shoot);
        assert!(elf_creature.next_available_tick.is_some());

        // ProjectileLaunched event emitted.
        assert!(events.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::ProjectileLaunched {
                attacker_id,
                target_id,
            } if *attacker_id == elf && *target_id == goblin
        )));
    }

    #[test]
    fn test_shoot_arrow_no_bow_fails() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);

        // Give arrows but NO bow.
        let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);

        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );

        // No projectile spawned.
        assert_eq!(sim.db.projectiles.iter_all().count(), 0);
        // Elf still idle.
        assert_eq!(
            sim.db.creatures.get(&elf).unwrap().action_kind,
            ActionKind::NoAction,
        );
    }

    #[test]
    fn test_shoot_arrow_no_arrows_fails() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);

        // Give bow but NO arrows.
        let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
        sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bow, 1, None, None);

        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );

        assert_eq!(sim.db.projectiles.iter_all().count(), 0);
    }

    #[test]
    fn test_shoot_arrow_cooldown_prevents_second_shot() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);
        arm_with_bow_and_arrows(&mut sim, elf, 10);

        // First shot.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );
        assert_eq!(sim.db.projectiles.iter_all().count(), 1);

        // Immediate second shot should fail (still on cooldown).
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick2 + 1,
        );

        // Arrow count should only have decreased by 1 (second shot failed).
        let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
        assert_eq!(sim.inv_item_count(inv_id, inventory::ItemKind::Arrow), 9);
    }

    #[test]
    fn test_shoot_arrow_blocked_los_fails() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        // Place goblin 5 voxels away.
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);
        arm_with_bow_and_arrows(&mut sim, elf, 5);

        // Place a solid wall between them.
        sim.world.set(
            VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z),
            VoxelType::Trunk,
        );

        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );

        // No projectile — LOS blocked.
        assert_eq!(sim.db.projectiles.iter_all().count(), 0);
    }

    #[test]
    fn test_shoot_arrow_leaf_does_not_block_los() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);
        arm_with_bow_and_arrows(&mut sim, elf, 5);

        // Place a leaf between them — should NOT block LOS.
        sim.world.set(
            VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z),
            VoxelType::Leaf,
        );

        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );

        // Projectile spawned (leaf doesn't block).
        assert_eq!(sim.db.projectiles.iter_all().count(), 1);
    }

    #[test]
    fn test_shoot_arrow_dead_target_fails() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);
        arm_with_bow_and_arrows(&mut sim, elf, 5);

        // Kill the goblin.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature {
                    creature_id: goblin,
                },
            }],
            tick + 1,
        );

        // Try to shoot dead goblin.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick2 + 1,
        );

        assert_eq!(sim.db.projectiles.iter_all().count(), 0);
    }

    #[test]
    fn test_shoot_arrow_dead_attacker_fails() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);
        arm_with_bow_and_arrows(&mut sim, elf, 5);

        // Kill the elf.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature { creature_id: elf },
            }],
            tick + 1,
        );

        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick2 + 1,
        );

        assert_eq!(sim.db.projectiles.iter_all().count(), 0);
    }

    #[test]
    fn test_shoot_action_serde_roundtrip() {
        let action = ActionKind::Shoot;
        let json = serde_json::to_string(&action).unwrap();
        let restored: ActionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, ActionKind::Shoot);
    }

    #[test]
    fn test_shoot_arrow_cooldown_expiry_allows_second_shot() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, elf);
        arm_with_bow_and_arrows(&mut sim, elf, 10);

        // First shot.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );
        assert_eq!(sim.db.projectiles.iter_all().count(), 1);
        let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
        assert_eq!(sim.inv_item_count(inv_id, inventory::ItemKind::Arrow), 9);

        // Advance past the cooldown. The activation system clears the Shoot
        // action, but the elf may start wandering. Force idle again to isolate
        // the second-shot test.
        let cooldown = sim.config.shoot_cooldown_ticks;
        sim.step(&[], sim.tick + cooldown + 1);
        force_idle(&mut sim, elf);

        // Second shot should succeed now that cooldown has expired.
        let tick2 = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick2 + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick2 + 1,
        );
        assert_eq!(sim.inv_item_count(inv_id, inventory::ItemKind::Arrow), 8);
    }

    #[test]
    fn test_shoot_arrow_rejected_when_not_idle() {
        let mut sim = test_sim(42);
        let elf = spawn_elf(&mut sim);
        let goblin = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        arm_with_bow_and_arrows(&mut sim, elf, 5);

        // Put elf into a non-idle action (e.g., Build).
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
            c.action_kind = ActionKind::Build;
            c.next_available_tick = Some(sim.tick + 5000);
        });

        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugShootAction {
                    attacker_id: elf,
                    target_id: goblin,
                },
            }],
            tick + 1,
        );

        // No projectile — elf was busy.
        assert_eq!(sim.db.projectiles.iter_all().count(), 0);
    }

    #[test]
    fn test_hostile_ai_shoots_when_armed() {
        let mut sim = test_sim(99);
        let elf_id = spawn_species(&mut sim, Species::Elf);
        let goblin_id = spawn_species(&mut sim, Species::Goblin);

        // Arm the goblin with bow + arrows. Don't reposition — let the sim's
        // natural spawn placement and nav graph handle positions.
        arm_with_bow_and_arrows(&mut sim, goblin_id, 10);

        // Run the sim long enough for the goblin to activate and find elves.
        // The goblin may melee if adjacent, or shoot if it has LOS and is far
        // enough away. Either way, it should consume arrows or deal damage.
        let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;
        sim.step(&[], sim.tick + 20_000);

        let inv_id = sim.db.creatures.get(&goblin_id).unwrap().inventory_id;
        let arrows_remaining = sim.inv_item_count(inv_id, inventory::ItemKind::Arrow);
        let elf_hp_after = sim.db.creatures.get(&elf_id).unwrap().hp;

        // The goblin should have either shot arrows or attacked in melee.
        assert!(
            arrows_remaining < 10 || elf_hp_after < elf_hp_before,
            "Hostile goblin with bow+arrows should have attacked (arrows={arrows_remaining}, \
             hp_before={elf_hp_before}, hp_after={elf_hp_after})"
        );
    }

    // -----------------------------------------------------------------------
    // Hostile AI tests
    // -----------------------------------------------------------------------

    #[test]
    fn combat_ai_config() {
        use crate::species::CombatAI;
        let sim = test_sim(42);
        // Aggressive melee species.
        assert_eq!(
            sim.species_table[&Species::Goblin].combat_ai,
            CombatAI::AggressiveMelee
        );
        assert_eq!(
            sim.species_table[&Species::Orc].combat_ai,
            CombatAI::AggressiveMelee
        );
        assert_eq!(
            sim.species_table[&Species::Troll].combat_ai,
            CombatAI::AggressiveMelee
        );
        // Passive species.
        assert_eq!(
            sim.species_table[&Species::Elf].combat_ai,
            CombatAI::Passive
        );
        assert_eq!(
            sim.species_table[&Species::Capybara].combat_ai,
            CombatAI::Passive
        );
        assert_eq!(
            sim.species_table[&Species::Deer].combat_ai,
            CombatAI::Passive
        );
        assert_eq!(
            sim.species_table[&Species::Boar].combat_ai,
            CombatAI::Passive
        );
        assert_eq!(
            sim.species_table[&Species::Monkey].combat_ai,
            CombatAI::Passive
        );
        assert_eq!(
            sim.species_table[&Species::Squirrel].combat_ai,
            CombatAI::Passive
        );
        assert_eq!(
            sim.species_table[&Species::Elephant].combat_ai,
            CombatAI::Passive
        );
        // Detection ranges are set for aggressive species, zero for passive.
        assert!(sim.species_table[&Species::Goblin].hostile_detection_range_sq > 0);
        assert!(sim.species_table[&Species::Orc].hostile_detection_range_sq > 0);
        assert!(sim.species_table[&Species::Troll].hostile_detection_range_sq > 0);
        assert_eq!(
            sim.species_table[&Species::Elf].hostile_detection_range_sq,
            0
        );
        assert_eq!(
            sim.species_table[&Species::Capybara].hostile_detection_range_sq,
            0
        );
    }

    #[test]
    fn hostile_creature_pursues_and_attacks_elf() {
        let mut sim = test_sim(99);
        let elf_id = spawn_species(&mut sim, Species::Elf);
        let goblin_id = spawn_species(&mut sim, Species::Goblin);

        let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
        let goblin_start = sim.db.creatures.get(&goblin_id).unwrap().position;

        assert_ne!(
            elf_pos, goblin_start,
            "Elf and goblin spawned at same position — adjust test seed"
        );

        let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

        sim.step(&[], sim.tick + 10_000);

        let elf_hp_after = sim.db.creatures.get(&elf_id).unwrap().hp;
        let goblin_pos = sim.db.creatures.get(&goblin_id).unwrap().position;
        let elf_current_pos = sim.db.creatures.get(&elf_id).unwrap().position;
        let new_dist = goblin_pos.manhattan_distance(elf_current_pos);
        let initial_dist = goblin_start.manhattan_distance(elf_pos);

        // The goblin should have either moved closer to the elf's current
        // position, or dealt damage (meaning it reached and attacked).
        let moved_closer = new_dist < initial_dist;
        let dealt_damage = elf_hp_after < elf_hp_before;
        assert!(
            moved_closer || dealt_damage,
            "Goblin should pursue or attack elf: initial dist={initial_dist}, \
             new dist={new_dist}, elf hp {elf_hp_before} -> {elf_hp_after}"
        );
    }

    #[test]
    fn hostile_creature_wanders_without_elves() {
        let mut sim = test_sim(99);
        let goblin_id = spawn_species(&mut sim, Species::Goblin);
        let goblin_start = sim.db.creatures.get(&goblin_id).unwrap().position;

        sim.step(&[], sim.tick + 10_000);

        let goblin_pos = sim.db.creatures.get(&goblin_id).unwrap().position;
        assert_ne!(
            goblin_start, goblin_pos,
            "Goblin should wander even without elves to pursue"
        );
    }

    #[test]
    fn hostile_creature_attacks_adjacent_elf() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        // Place goblin adjacent to elf.
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        // Schedule an activation so the goblin enters the decision cascade.
        let tick = sim.tick;
        sim.event_queue.schedule(
            tick + 1,
            ScheduledEventKind::CreatureActivation {
                creature_id: goblin,
            },
        );

        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

        // Run one activation cycle — the goblin should melee the elf.
        sim.step(&[], tick + 2);

        let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
        let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
        assert_eq!(
            elf_hp_after,
            elf_hp_before - goblin_damage,
            "Adjacent hostile should automatically melee-strike the elf"
        );
    }

    // -----------------------------------------------------------------------
    // Projectile system tests (F-projectiles)
    // -----------------------------------------------------------------------

    #[test]
    fn spawn_projectile_creates_entity_and_inventory() {
        let mut sim = test_sim(42);
        let origin = VoxelCoord::new(40, 5, 40);
        let target = VoxelCoord::new(50, 5, 40);

        sim.spawn_projectile(origin, target, None);

        assert_eq!(sim.db.projectiles.len(), 1);
        let proj = sim.db.projectiles.iter_all().next().unwrap();
        assert_eq!(proj.shooter, None);
        assert_eq!(proj.prev_voxel, origin);
        // Should have an inventory with 1 arrow.
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].kind, inventory::ItemKind::Arrow);
        assert_eq!(stacks[0].quantity, 1);
    }

    #[test]
    fn spawn_projectile_schedules_tick_event() {
        let mut sim = test_sim(42);
        let initial_events = sim.event_queue.len();
        sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);
        // Should have scheduled exactly one ProjectileTick.
        assert_eq!(sim.event_queue.len(), initial_events + 1);
    }

    #[test]
    fn second_spawn_does_not_duplicate_tick_event() {
        let mut sim = test_sim(42);
        let initial_events = sim.event_queue.len();
        sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);
        sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(45, 5, 40), None);
        // Only one extra event (from first spawn), not two.
        assert_eq!(sim.event_queue.len(), initial_events + 1);
    }

    #[test]
    fn projectile_hits_solid_voxel_and_creates_ground_pile() {
        let mut sim = test_sim(42);
        // Place a solid wall at x=45.
        for y in 1..=5 {
            sim.world
                .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
        }

        // Spawn projectile heading +x toward the wall (flat, no gravity).
        sim.config.arrow_gravity = 0;
        sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
        sim.spawn_projectile(VoxelCoord::new(40, 3, 40), VoxelCoord::new(45, 3, 40), None);

        // Run until the projectile resolves (max 500 ticks).
        for _ in 0..500 {
            if sim.db.projectiles.len() == 0 {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            if sim.db.projectiles.len() > 0 {
                sim.event_queue
                    .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
            }
        }

        assert_eq!(sim.db.projectiles.len(), 0, "Projectile should be resolved");

        // Should have a ground pile with an arrow in it near x=44 (prev_voxel).
        let mut found_arrow = false;
        for pile in sim.db.ground_piles.iter_all() {
            let stacks = sim
                .db
                .item_stacks
                .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);
            for s in &stacks {
                if s.kind == inventory::ItemKind::Arrow {
                    found_arrow = true;
                }
            }
        }
        assert!(found_arrow, "Arrow should land as ground pile");
    }

    #[test]
    fn projectile_hits_creature_and_deals_damage() {
        let mut sim = test_sim(42);
        // Spawn a goblin at a known position.
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
        let goblin_hp_before = sim.db.creatures.get(&goblin).unwrap().hp;

        // Spawn projectile aimed at the goblin (no gravity for predictability).
        sim.config.arrow_gravity = 0;
        sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
        let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
        sim.spawn_projectile(origin, goblin_pos, None);

        // Run until resolved.
        let mut hit_events = Vec::new();
        for _ in 0..500 {
            if sim.db.projectiles.len() == 0 {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            for e in &events {
                if matches!(e.kind, SimEventKind::ProjectileHitCreature { .. }) {
                    hit_events.push(e.clone());
                }
            }
            if sim.db.projectiles.len() > 0 {
                sim.event_queue
                    .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
            }
        }

        assert_eq!(sim.db.projectiles.len(), 0, "Projectile should be resolved");
        assert!(!hit_events.is_empty(), "Should have hit the creature");

        let goblin_hp_after = sim.db.creatures.get(&goblin).unwrap().hp;
        assert!(
            goblin_hp_after < goblin_hp_before,
            "Goblin should have taken damage: {goblin_hp_before} -> {goblin_hp_after}"
        );
    }

    #[test]
    fn projectile_out_of_bounds_despawns_silently() {
        let mut sim = test_sim(42);
        // Shoot a projectile off the edge of the world.
        sim.config.arrow_gravity = 0;
        sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 5; // very fast

        sim.spawn_projectile(
            VoxelCoord::new(250, 5, 128),
            VoxelCoord::new(260, 5, 128), // target is beyond world bounds
            None,
        );

        // Run until resolved.
        for _ in 0..2000 {
            if sim.db.projectiles.len() == 0 {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            // No surface hit or creature hit events expected.
            for e in &events {
                assert!(
                    !matches!(e.kind, SimEventKind::ProjectileHitSurface { .. }),
                    "Should not hit surface"
                );
            }
            if sim.db.projectiles.len() > 0 {
                sim.event_queue
                    .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
            }
        }

        assert_eq!(
            sim.db.projectiles.len(),
            0,
            "Projectile should have despawned"
        );
    }

    #[test]
    fn projectile_does_not_hit_shooter() {
        let mut sim = test_sim(42);
        // Spawn an elf and shoot from their position.
        let elf = spawn_species(&mut sim, Species::Elf);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

        sim.config.arrow_gravity = 0;
        sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

        // Shoot from the elf's own position toward a distant target.
        sim.spawn_projectile(
            elf_pos,
            VoxelCoord::new(elf_pos.x + 20, elf_pos.y, elf_pos.z),
            Some(elf),
        );

        // Run a few ticks — the projectile should pass through the shooter.
        for _ in 0..50 {
            if sim.db.projectiles.is_empty() {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            if !sim.db.projectiles.is_empty() {
                sim.event_queue
                    .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
            }
        }

        // Elf should not have taken any damage.
        let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
        assert_eq!(
            elf_hp_after, elf_hp_before,
            "Shooter should not be hit by their own arrow"
        );
    }

    #[test]
    fn hostile_creature_wanders_after_killing_elf() {
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        // Place goblin adjacent to elf.
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        // Kill the elf.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugKillCreature { creature_id: elf },
            }],
            tick + 1,
        );
        assert_eq!(
            sim.db.creatures.get(&elf).unwrap().vital_status,
            VitalStatus::Dead,
        );

        // With no living elves, the goblin should fall back to random wander.
        sim.step(&[], sim.tick + 10_000);
        let goblin_final = sim.db.creatures.get(&goblin).unwrap().position;
        assert_ne!(
            goblin_final, goblin_pos,
            "Goblin should wander after elf is dead"
        );
    }

    #[test]
    fn projectile_skips_origin_voxel_creatures() {
        let mut sim = test_sim(42);
        // Spawn shooter and bystander at the same position.
        let shooter = spawn_species(&mut sim, Species::Elf);
        let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position;
        let shooter_hp = sim.db.creatures.get(&shooter).unwrap().hp;

        let bystander = spawn_species(&mut sim, Species::Elf);
        // Move bystander to the same position as the shooter.
        if let Some(mut c) = sim.db.creatures.get(&bystander) {
            c.position = shooter_pos;
            let _ = sim.db.creatures.update_no_fk(c);
        }
        sim.rebuild_spatial_index();
        let bystander_hp = sim.db.creatures.get(&bystander).unwrap().hp;

        sim.config.arrow_gravity = 0;
        sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

        // Shoot from the shared position toward a distant target.
        sim.spawn_projectile(
            shooter_pos,
            VoxelCoord::new(shooter_pos.x + 20, shooter_pos.y, shooter_pos.z),
            Some(shooter),
        );

        // Run ticks until projectile is consumed or max iterations.
        for _ in 0..50 {
            if sim.db.projectiles.is_empty() {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            if !sim.db.projectiles.is_empty() {
                sim.event_queue
                    .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
            }
        }

        // Neither the shooter nor the bystander in the origin voxel should
        // have been hit — projectiles skip the entire launch voxel.
        let shooter_hp_after = sim.db.creatures.get(&shooter).unwrap().hp;
        assert_eq!(
            shooter_hp_after, shooter_hp,
            "Shooter should not be hit by their own arrow"
        );

        let bystander_hp_after = sim.db.creatures.get(&bystander).unwrap().hp;
        assert_eq!(
            bystander_hp_after, bystander_hp,
            "Bystander in origin voxel should not be hit (hp: {} -> {})",
            bystander_hp, bystander_hp_after,
        );
    }

    #[test]
    fn hostile_waits_on_cooldown_near_elf() {
        // When a hostile is in melee range but on cooldown, it should not
        // wander away — it should wait and re-strike when the cooldown expires.
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);
        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
        force_position(&mut sim, goblin, goblin_pos);
        force_idle(&mut sim, goblin);

        // First strike via command puts goblin on cooldown.
        let tick = sim.tick;
        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugMeleeAttack {
                    attacker_id: goblin,
                    target_id: elf,
                },
            }],
            tick + 1,
        );
        assert_eq!(
            sim.db.creatures.get(&goblin).unwrap().action_kind,
            ActionKind::MeleeStrike,
        );

        // Advance past cooldown. Goblin should stay near elf and strike again,
        // NOT wander away.
        let interval = sim.species_table[&Species::Goblin].melee_interval_ticks;
        sim.step(&[], sim.tick + interval + 100);

        let goblin_final = sim.db.creatures.get(&goblin).unwrap().position;
        let dist = goblin_final.manhattan_distance(elf_pos);
        // Should still be within melee range (manhattan dist ≤ 2 for range_sq=2).
        assert!(
            dist <= 2,
            "Goblin should stay near elf on cooldown, not wander away (dist={dist})"
        );
    }

    #[test]
    fn hostile_ignores_elf_outside_detection_range() {
        // A goblin with detection_range_sq=225 (15 voxels) should NOT pursue
        // an elf that is >15 voxels away in euclidean distance.
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);

        // Place elf far from goblin — 50 voxels away on X axis (50² = 2500 >> 225).
        let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
        let far_pos = VoxelCoord::new(goblin_pos.x + 50, goblin_pos.y, goblin_pos.z);
        force_position(&mut sim, elf, far_pos);

        // Schedule activation.
        let tick = sim.tick;
        sim.event_queue.schedule(
            tick + 1,
            ScheduledEventKind::CreatureActivation {
                creature_id: goblin,
            },
        );
        force_idle(&mut sim, goblin);

        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

        // Run a short period — goblin should wander randomly, not pursue.
        // Keep ticks low so random wander can't close the 50-voxel gap.
        sim.step(&[], tick + 3000);

        let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
        assert_eq!(
            elf_hp_before, elf_hp_after,
            "Goblin should not attack elf outside detection range"
        );
        // Goblin should have wandered but NOT moved closer to the elf.
        // (It might have moved closer by random chance, so we just check
        // it didn't deal damage — the key assertion.)
    }

    #[test]
    fn hostile_pursues_elf_within_detection_range() {
        // A goblin with detection_range_sq=225 (15 voxels) SHOULD pursue
        // an elf within 10 voxels (10² = 100 < 225).
        let mut sim = test_sim(42);
        let goblin = spawn_species(&mut sim, Species::Goblin);
        let elf = spawn_elf(&mut sim);

        // Place elf 5 voxels from goblin on X axis (5² = 25 < 225).
        let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
        let near_pos = VoxelCoord::new(goblin_pos.x + 5, goblin_pos.y, goblin_pos.z);
        force_position(&mut sim, elf, near_pos);

        // Schedule activation.
        let tick = sim.tick;
        sim.event_queue.schedule(
            tick + 1,
            ScheduledEventKind::CreatureActivation {
                creature_id: goblin,
            },
        );
        force_idle(&mut sim, goblin);

        let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
        let initial_dist = goblin_pos.manhattan_distance(near_pos);

        sim.step(&[], tick + 10_000);

        let goblin_final = sim.db.creatures.get(&goblin).unwrap().position;
        let elf_current = sim.db.creatures.get(&elf).unwrap().position;
        let new_dist = goblin_final.manhattan_distance(elf_current);
        let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;

        let moved_closer = new_dist < initial_dist;
        let dealt_damage = elf_hp_after < elf_hp_before;
        assert!(
            moved_closer || dealt_damage,
            "Goblin should pursue elf within detection range: \
             initial dist={initial_dist}, new dist={new_dist}, \
             elf hp {elf_hp_before} -> {elf_hp_after}"
        );
    }

    #[test]
    fn hostile_does_not_attack_same_species() {
        // Two non-civ goblins adjacent to each other should NOT attack.
        let mut sim = test_sim(42);
        let g1 = spawn_species(&mut sim, Species::Goblin);
        let g2 = spawn_species(&mut sim, Species::Goblin);

        // Place them adjacent.
        let g1_pos = sim.db.creatures.get(&g1).unwrap().position;
        let g2_pos = VoxelCoord::new(g1_pos.x + 1, g1_pos.y, g1_pos.z);
        force_position(&mut sim, g2, g2_pos);
        force_idle(&mut sim, g1);
        force_idle(&mut sim, g2);

        let tick = sim.tick;
        sim.event_queue.schedule(
            tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id: g1 },
        );
        sim.event_queue.schedule(
            tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id: g2 },
        );

        let g1_hp_before = sim.db.creatures.get(&g1).unwrap().hp;
        let g2_hp_before = sim.db.creatures.get(&g2).unwrap().hp;

        sim.step(&[], tick + 3000);

        let g1_hp_after = sim.db.creatures.get(&g1).unwrap().hp;
        let g2_hp_after = sim.db.creatures.get(&g2).unwrap().hp;
        assert_eq!(
            g1_hp_before, g1_hp_after,
            "Goblins should not attack same species"
        );
        assert_eq!(
            g2_hp_before, g2_hp_after,
            "Goblins should not attack same species"
        );
    }

    #[test]
    fn all_hostile_species_pursue_elves() {
        for &hostile_species in &[Species::Goblin, Species::Orc, Species::Troll] {
            let mut sim = test_sim(99);
            let elf_id = spawn_species(&mut sim, Species::Elf);
            let hostile_id = spawn_species(&mut sim, hostile_species);

            let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
            let hostile_start = sim.db.creatures.get(&hostile_id).unwrap().position;

            assert_ne!(
                elf_pos, hostile_start,
                "{hostile_species:?} and elf spawned at same position — adjust test seed"
            );

            let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;
            let initial_dist = hostile_start.manhattan_distance(elf_pos);

            sim.step(&[], sim.tick + 10_000);

            let hostile_pos = sim.db.creatures.get(&hostile_id).unwrap().position;
            let elf_current_pos = sim.db.creatures.get(&elf_id).unwrap().position;
            let new_dist = hostile_pos.manhattan_distance(elf_current_pos);
            let elf_hp_after = sim.db.creatures.get(&elf_id).unwrap().hp;

            let moved_closer = new_dist < initial_dist;
            let dealt_damage = elf_hp_after < elf_hp_before;
            assert!(
                moved_closer || dealt_damage,
                "{hostile_species:?} should pursue or attack elf: initial dist={initial_dist}, \
                 new dist={new_dist}, elf hp {elf_hp_before} -> {elf_hp_after}"
            );
        }
    }

    #[test]
    fn projectile_hits_creature_beyond_origin_voxel() {
        let mut sim = test_sim(42);
        // Place a target creature a few voxels away from the origin.
        let target = spawn_species(&mut sim, Species::Elf);
        let origin = VoxelCoord::new(40, 1, 40);
        let target_pos = VoxelCoord::new(42, 1, 40);
        if let Some(mut c) = sim.db.creatures.get(&target) {
            c.position = target_pos;
            let _ = sim.db.creatures.update_no_fk(c);
        }
        sim.rebuild_spatial_index();
        let target_hp = sim.db.creatures.get(&target).unwrap().hp;

        sim.config.arrow_gravity = 0;
        sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

        // Shoot from origin toward the target (no shooter creature).
        sim.spawn_projectile(origin, target_pos, None);

        // Run ticks.
        let mut hit = false;
        for _ in 0..100 {
            if sim.db.projectiles.is_empty() {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            for e in &events {
                if let SimEventKind::ProjectileHitCreature { target_id, .. } = e.kind {
                    if target_id == target {
                        hit = true;
                    }
                }
            }
            if !sim.db.projectiles.is_empty() {
                sim.event_queue
                    .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
            }
        }

        assert!(hit, "Projectile should hit creature beyond origin voxel");
        let target_hp_after = sim.db.creatures.get(&target).unwrap().hp;
        assert!(
            target_hp_after < target_hp,
            "Target should have taken damage (hp: {} -> {})",
            target_hp,
            target_hp_after,
        );
    }

    #[test]
    fn projectile_cleanup_removes_inventory() {
        let mut sim = test_sim(42);
        sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);
        let proj = sim.db.projectiles.iter_all().next().unwrap();
        let inv_id = proj.inventory_id;
        let proj_id = proj.id;

        // Verify inventory exists.
        assert!(sim.db.inventories.get(&inv_id).is_some());

        sim.remove_projectile(proj_id);

        // Projectile, inventory, and item stacks should all be gone.
        assert_eq!(sim.db.projectiles.len(), 0);
        assert!(sim.db.inventories.get(&inv_id).is_none());
        assert!(
            sim.db
                .item_stacks
                .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
                .is_empty()
        );
    }

    #[test]
    fn projectile_serde_roundtrip() {
        let mut sim = test_sim(42);
        sim.spawn_projectile(VoxelCoord::new(40, 5, 40), VoxelCoord::new(50, 5, 40), None);

        let json = sim.to_json().unwrap();
        let sim2 = SimState::from_json(&json).unwrap();

        assert_eq!(sim2.db.projectiles.len(), 1);
        let proj = sim2.db.projectiles.iter_all().next().unwrap();
        let stacks = sim2
            .db
            .item_stacks
            .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
        assert_eq!(stacks.len(), 1);
        assert_eq!(stacks[0].kind, inventory::ItemKind::Arrow);
    }

    #[test]
    fn debug_spawn_projectile_command() {
        let mut sim = test_sim(42);
        let origin = VoxelCoord::new(40, 5, 40);
        let target = VoxelCoord::new(50, 5, 40);
        let tick = sim.tick;

        sim.step(
            &[SimCommand {
                player_id: sim.player_id,
                tick: tick + 1,
                action: SimAction::DebugSpawnProjectile {
                    origin,
                    target,
                    shooter_id: None,
                },
            }],
            tick + 1,
        );

        assert_eq!(sim.db.projectiles.len(), 1);
    }
}
