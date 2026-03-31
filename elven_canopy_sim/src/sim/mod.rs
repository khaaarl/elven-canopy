// Core simulation state and tick loop.
//
// `SimState` is the single source of truth for the entire game world. Entity
// data (creatures, tasks, blueprints, structures, ground piles, trees) lives in
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
// ## Poll-based activation
//
// Creature behavior uses **poll-based activation**: each tick, all living
// creatures whose `next_available_tick <= current_tick` are activated in
// deterministic CreatureId order. Each activation performs one action (walk
// 1 nav edge or do 1 unit of task work) and sets `next_available_tick` for
// the next poll. The sim runs at **1000 ticks per simulated second**
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
//   below the target, first call `find_owned_item_source()` to reclaim
//   the creature's own items from other inventories, then fall back to
//   `find_unowned_item_source()` for unclaimed items. One task per heartbeat.
//
// The activation loop (`process_creature_activation`) runs this logic:
//
//   0. **Flee check** (F-flee): before the decision cascade, if the creature
//      should flee (based on `engagement_style` disengage threshold) and detects
//      a hostile within its effective detection range (base `hostile_detection_range_sq`
//      scaled by Perception via `effective_detection_range_sq()`), interrupt any current task
//      and perform a greedy retreat step (pick the nav neighbor that maximizes
//      squared distance from the nearest threat). Ties broken by `NavNodeId`.
//   1. If the creature has no task (`current_task == None`), check for an
//      available task to claim. If none found, **wander**: pick a random
//      adjacent nav node, move there, and schedule the next activation at
//      `now + ceil(edge.distance * ticks_per_voxel)`.
//   2. If the creature has a task, run its **behavior script** (see below).
//
// Wandering is intentionally local and aimless — no pathfinding, just 1 random
// neighbor per activation. This creates natural-looking milling about.
//
// ## Voxel exclusion (F-voxel-exclusion)
//
// Creatures cannot move into voxels occupied by hostile creatures. This is
// enforced at **move time** (not pathfind time) via `destination_blocked_by_hostile()`,
// which checks all voxels in the mover's destination footprint against the
// spatial index. Multi-voxel creatures (e.g. 2x2x2 elephants) check all
// destination footprint voxels. When blocked, the creature stays put and
// schedules a retry after `config.voxel_exclusion_retry_ticks`.
//
// Enforcement points: `ground_move_one_step()` (safety net for all callers),
// `ground_random_wander()` (pre-filters edges), `ground_flee_step()` (pre-filters with
// fallback if cornered), and the three `walk_toward_*()` inline movers.
//
// Pathfinding (`pathfinding.rs`) intentionally knows nothing about occupancy —
// creature positions are dynamic and would make paths stale instantly. The
// move-time check is simpler and handles transient blocking naturally.
//
// ## Task system
//
// Tasks are the core assignment mechanism. The sim's `db.tasks` table stores
// the base task data; variant-specific data is decomposed into extension tables
// (`task_haul_data`, `task_sleep_data`, `task_acquire_data`, `task_craft_data`,
// `task_attack_target_data`, `task_attack_move_data`) and relationship
// tables (`task_blueprint_refs`, `task_structure_refs`, `task_voxel_refs`).
// Query helpers on `SimState` (`task_project_id`, `task_structure_ref`,
// `task_voxel_ref`, `task_haul_data`, `task_sleep_data`, `task_acquire_data`,
// `task_craft_data`, `task_attack_target_data`, `task_attack_move_data`,
// `task_haul_source`, `task_acquire_source`, `task_sleep_location`) abstract
// the extension table lookups. Each creature stores an optional `current_task`.
//
// ### Task entity (`task.rs`)
//
// A `Task` has:
// - `kind: TaskKind` — determines behavior (`GoTo`, `Build`, `EatBread`,
//   `EatFruit`, `Sleep`, `Furnish`, `Haul`, `Cook`, `Harvest`, `AcquireItem`,
//   `Mope`, `Craft`, `AttackTarget`, `AttackMove`).
// - `state: TaskState` — lifecycle: `Available` → `InProgress` → `Complete`.
// - `location: NavNodeId` — where creatures go to work on the task.
// - Assignment tracked via `creature.current_task` FK.
// - `progress: i64` and `total_cost: i64` — for tasks that require work units
//   (0 total_cost means instant completion, e.g. GoTo).
// - `required_species: Option<Species>` — species restriction (if `Some`,
//   only that species can claim it).
//
// ### Task lifecycle
//
// 1. A `CreateTask` command (from the UI via `sim_bridge.rs`) creates a task
//    in `Available` state, snapped to the nearest nav node.
// 2. On its next activation, an idle creature whose species matches calls
//    `find_available_task`, which uses Dijkstra search on the nav graph to
//    find the nearest `Available` task by travel cost. The creature calls
//    `claim_task`, which sets the task to `InProgress`, sets the creature's
//    `current_task`, and computes an A* path to `task.location`.
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
// 1. `start_*_action()` — sets `action_kind` and `next_available_tick`
//    (poll-based activation fires when that tick is reached).
// 2. `resolve_*_action()` — applies the action's effects (place voxel,
//    restore food, etc.) and returns whether the task completed.
// 3. Decision cascade — if task not done, re-enter `execute_task_behavior`
//    to start the next action; if done, find a new task or wander.
//
// **Interruption:** `interrupt_task()` is the entry point for hard task
// interruption (nav invalidation, mope preemption, death, flee). It calls
// `abort_current_action()` to clear the in-progress action and reset
// activation state, then delegates to `cleanup_and_unassign_task()`
// for per-kind cleanup (release reservations, drop carried items) and task
// state transitions (resumable → Available, others → Complete).
//
// **Preemption:** `preempt_task()` is used by player commands (DirectedGoTo,
// AttackTarget, AttackMove) to swap the task without aborting the in-progress
// action. The current action completes naturally at `next_available_tick`,
// then the creature picks up the new task in the decision cascade. This
// prevents exploitable double-speed movement from command spamming.
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
// species). Among candidates it picks the nearest by Dijkstra nav-graph
// distance (actual travel cost), falling back to arbitrary order only for
// tasks at the same nav node.
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
// ## HP, incapacitation, and death
//
// Each creature has `hp` and `hp_max` (set from `SpeciesData` at spawn) and
// a `vital_status` field (Alive/Incapacitated/Dead). `DamageCreature`
// reduces HP; reaching 0 incapacitates (sets `Incapacitated`, aborts action,
// emits event). Incapacitated creatures bleed out at 1 HP per heartbeat;
// true death at `-hp_max`. Massive hits past `-hp_max` kill outright.
// `HealCreature` restores HP (clamped to `hp_max`, revives incapacitated
// creatures above 0 HP, no-op on dead). `DebugKillCreature` kills instantly
// (bypasses incapacitation). Starvation also bypasses incapacitation.
// Species with `ticks_per_hp_regen > 0` passively regenerate HP at heartbeat
// (see `species.rs` for the field definition).
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
// `SimState` derives `Serialize`/`Deserialize` via serde. The voxel world is
// serialized directly using a compact binary pack format (see `world.rs`).
// Several transient fields (`nav_graph`, `large_nav_graph`, `species_table`,
// `lexicon`, `face_data`, `ladder_orientations`, `structure_voxels`) are
// `#[serde(skip)]` and must be rebuilt after deserialization via
// `rebuild_transient_state()`. Convenience methods `to_json()` and
// `from_json()` handle the full serialize/deserialize + rebuild cycle.
//
// ## Sub-modules
//
// `SimState` methods are split across focused sub-modules, each containing
// `impl SimState` blocks for a specific domain. This file (mod.rs) retains
// the struct definition, constructors, tick loop, event dispatch, and
// serialization. The sub-modules are:
//
// - `activation.rs`:    Creature activation chain, task selection and claiming.
// - `combat.rs`:        Melee, ranged, projectiles, flee, hostile AI, diplomacy.
// - `construction.rs`:  Build/carve designation, materialization, furnishing, raycast.
// - `crafting.rs`:      Recipe execution, active recipe management, cooking.
// - `creature.rs`:      Spawning, surface placement, pile gravity, task interruption.
// - `greenhouse.rs`:    Fruit spawning and harvest monitoring.
// - `inventory_mgmt.rs`: Item stack operations, reservations, equipment, durability.
// - `logistics.rs`:     Hauling, harvesting, pickup/dropoff, logistics heartbeat.
// - `movement.rs`:      GoTo commands, unit spreading, step execution, wandering.
// - `needs.rs`:         Eating, sleeping, moping, personal item acquisition.
// - `task_helpers.rs`:  Task extension table accessors and `insert_task`.
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

use crate::blueprint::BlueprintState;
use crate::command::{SimAction, SimCommand};
use crate::config::GameConfig;
use crate::db::{ActionKind, SimDb};
use crate::event::{EventQueue, ScheduledEventKind, SimEvent};
use crate::inventory;
use crate::nav::{self, NavGraph};
use crate::prng::GameRng;
use crate::species::SpeciesData;
use crate::structural;
use crate::task;
use crate::types::*;
use crate::world::VoxelWorld;
use elven_canopy_lang::Lexicon;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

    // trees field removed — now in self.db.trees / self.db.great_tree_infos
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

    /// The player-controlled civilization's ID. `None` for pre-civilization saves.
    #[serde(default)]
    pub player_civ_id: Option<CivId>,

    /// The 3D voxel world grid. Serialized compactly as packed binary data.
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

    /// Positions where mana-wasted work actions occurred during the current
    /// step. Cleared at the start of each `step()` call. Read by the GDScript
    /// VFX layer via `sim_bridge.rs::get_mana_wasted_positions()` to spawn
    /// floating blue swirl sprites.
    #[serde(skip)]
    pub mana_wasted_positions: Vec<VoxelCoord>,

    /// Set of dirt voxel coordinates that are NOT grassy. By default, any
    /// exposed dirt voxel is considered grassy. This set stores the exceptions.
    /// Dirt becomes grassless when: (1) a creature grazes on it, or (2) a voxel
    /// change freshly exposes dirt. Grassless dirt regrows periodically via the
    /// `GrassRegrowth` scheduled event. Used by mesh generation to color
    /// grassless dirt brown instead of green. `BTreeSet` for deterministic
    /// iteration (regrowth sweep).
    #[serde(default)]
    pub grassless: BTreeSet<VoxelCoord>,
}

/// A creature's current path through the nav graph.
///
/// Stores positions (`VoxelCoord`) instead of `NavNodeId`s so that paths are
/// independent of nav-graph node ID assignment. At each step the sim resolves
/// the next position to a `NavNodeId` via the graph's spatial index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreaturePath {
    /// Remaining positions to visit (next position is index 0).
    pub remaining_positions: Vec<VoxelCoord>,
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

/// Compute the closest-point squared euclidean distance between two
/// footprint bounding boxes. Used by weapon selection to pick the best
/// melee weapon for the actual distance to target.
pub fn melee_distance_sq(
    attacker_pos: VoxelCoord,
    attacker_footprint: [u8; 3],
    target_pos: VoxelCoord,
    target_footprint: [u8; 3],
) -> i64 {
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
    dx * dx + dy * dy + dz * dz
}

mod activation;
use activation::NextCreatureActivation;
mod activity;
mod combat;
mod construction;
mod crafting;
mod creature;
mod grazing;
mod greenhouse;
mod inventory_mgmt;
mod logistics;
mod movement;
mod needs;
mod paths;
mod raid;
mod skills;
pub(crate) mod social;
mod taming;
mod task_helpers;

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

    /// Find a path from a creature's current position to `goal`.
    ///
    /// Dispatches to nav-graph A* (ground creatures) or flight A* (flying
    /// creatures) based on species. Uses stat-modified movement speeds.
    /// Returns `None` if the creature doesn't exist, the goal is unreachable,
    /// or all paths exceed `max_path_len`.
    ///
    /// For ground creatures, the returned `PathResult` has `nav_nodes` and
    /// `nav_edges` populated. For flyers, those fields are empty.
    ///
    /// Named `find_path` rather than `creature_path` to avoid collision with
    /// the elf life-path method in `paths.rs`.
    pub fn find_path(
        &self,
        creature_id: CreatureId,
        goal: VoxelCoord,
        max_path_len: u32,
    ) -> Option<crate::pathfinding::PathResult> {
        let creature = self.db.creatures.get(&creature_id)?;
        let species = creature.species;
        let species_data = &self.species_table[&species];
        let position = creature.position;

        if let Some(flight_tpv) = species_data.flight_ticks_per_voxel {
            // Flying creature: A* on voxel grid.
            let footprint = species_data.footprint;
            crate::pathfinding::astar_fly(
                &self.world,
                position,
                goal,
                flight_tpv,
                max_path_len,
                footprint,
            )
        } else {
            // Ground creature: A* on nav graph with stat-modified speeds.
            let graph = self.graph_for_species(species);
            let start_node = graph.node_at(position)?;
            let goal_node = graph.node_at(goal)?;
            let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
            let strength = self.trait_int(creature_id, TraitKind::Strength, 0);
            let move_speeds =
                crate::stats::CreatureMoveSpeeds::new(species_data, agility, strength);
            let nav_speeds = crate::pathfinding::NavGraphSpeeds::from_move_speeds(
                &move_speeds,
                species_data.allowed_edge_types.as_deref(),
            );
            crate::pathfinding::astar_navgraph(
                graph,
                start_node,
                goal_node,
                &nav_speeds,
                max_path_len,
            )
        }
    }

    /// Find the nearest reachable candidate from a creature's current position.
    ///
    /// Dispatches to nav-graph Dijkstra (ground creatures) or sequential
    /// flight A* (flying creatures) based on species. Uses stat-modified
    /// movement speeds and the species' edge-type filter.
    ///
    /// Returns the *index* into `candidates` of the nearest reachable one,
    /// or `None` if the creature doesn't exist or no candidate is reachable.
    pub fn find_nearest(
        &self,
        creature_id: CreatureId,
        candidates: &[VoxelCoord],
        max_path_len: u32,
    ) -> Option<usize> {
        if candidates.is_empty() {
            return None;
        }

        let creature = self.db.creatures.get(&creature_id)?;
        let species = creature.species;
        let species_data = &self.species_table[&species];
        let position = creature.position;

        if let Some(flight_tpv) = species_data.flight_ticks_per_voxel {
            // Flying creature: interleaved A* across candidates.
            let footprint = species_data.footprint;
            let nearest_coord = crate::pathfinding::nearest_fly(
                &self.world,
                position,
                candidates,
                flight_tpv,
                max_path_len,
                footprint,
            )?;
            candidates.iter().position(|&c| c == nearest_coord)
        } else {
            // Ground creature: interleaved A* on nav graph.
            let graph = self.graph_for_species(species);
            let start_node = graph.node_at(position)?;

            // Convert candidate VoxelCoords to NavNodeIds, tracking the mapping.
            // Candidates must be at nav node positions (use node_at). Callers
            // whose candidates aren't on nav nodes (e.g., fruit positions)
            // should resolve to nav nodes before calling this function.
            let mut target_nodes = Vec::with_capacity(candidates.len());
            let mut index_map = Vec::with_capacity(candidates.len());
            for (i, &coord) in candidates.iter().enumerate() {
                if let Some(nav_node) = graph.node_at(coord) {
                    target_nodes.push(nav_node);
                    index_map.push(i);
                }
            }
            if target_nodes.is_empty() {
                return None;
            }

            let agility = self.trait_int(creature_id, TraitKind::Agility, 0);
            let strength = self.trait_int(creature_id, TraitKind::Strength, 0);
            let move_speeds =
                crate::stats::CreatureMoveSpeeds::new(species_data, agility, strength);
            let nav_speeds = crate::pathfinding::NavGraphSpeeds::from_move_speeds(
                &move_speeds,
                species_data.allowed_edge_types.as_deref(),
            );

            let nearest_node = crate::pathfinding::nearest_navgraph(
                graph,
                start_node,
                &target_nodes,
                &nav_speeds,
            )?;

            // Map the NavNodeId back to the original candidate index.
            let target_idx = target_nodes.iter().position(|&n| n == nearest_node)?;
            Some(index_map[target_idx])
        }
    }

    /// Remove a fruit position from whichever tree owns it.
    pub(crate) fn remove_fruit_from_trees(&mut self, fruit_pos: VoxelCoord) {
        // Find the tree containing this fruit position and remove it.
        let tree_id = self
            .db
            .trees
            .iter_all()
            .find(|t| t.fruit_positions.contains(&fruit_pos))
            .map(|t| t.id);
        if let Some(tree_id) = tree_id
            && let Some(mut t) = self.db.trees.get(&tree_id)
        {
            t.fruit_positions.retain(|&p| p != fruit_pos);
            let _ = self.db.update_tree(t);
        }
    }

    /// Create a new simulation with the given seed and config.
    ///
    /// Delegates world creation to `worldgen::run_worldgen()`, which runs
    /// generators in a defined order (tree → fruits → civs → knowledge) using
    /// a dedicated worldgen PRNG. The runtime PRNG is derived from the worldgen
    /// PRNG's final state, ensuring deterministic separation.
    pub fn with_config(seed: u64, config: GameConfig) -> Self {
        Self::with_config_and_log(seed, config, &crate::worldgen::stderr_log())
    }

    /// Like `with_config`, but accepts a custom logging callback for worldgen
    /// timing output. Use this from GDExtension to route logs through
    /// `godot_print!`.
    pub fn with_config_and_log(
        seed: u64,
        config: GameConfig,
        log: &crate::worldgen::WgLog,
    ) -> Self {
        use crate::worldgen;

        let wg = worldgen::run_worldgen(seed, &config, log);

        let player_tree_id = wg.player_tree_id;

        // Build species table from config.
        let species_table = config.species.clone();

        let mut state = Self {
            tick: 0,
            rng: wg.runtime_rng,
            config,
            event_queue: EventQueue::new(),
            db: wg.db,
            placed_voxels: Vec::new(),
            carved_voxels: Vec::new(),
            face_data_list: Vec::new(),
            face_data: BTreeMap::new(),
            ladder_orientations_list: Vec::new(),
            ladder_orientations: BTreeMap::new(),
            next_structure_id: 0,
            player_tree_id,
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
            mana_wasted_positions: Vec::new(),
            grassless: BTreeSet::new(),
        };

        // The world rebuild above produces thousands of set() calls that
        // accumulate dirty_voxels entries. Clear them — the mesh cache will
        // do a full build_all() at init, so those entries aren't needed.
        state.world.clear_dirty_voxels();
        // Compact RLE column groups after bulk worldgen writes.
        state.world.repack_all();

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

        // Schedule the first grass regrowth sweep.
        let grass_interval = state.config.grass_regrowth_interval_ticks;
        state
            .event_queue
            .schedule(grass_interval, ScheduledEventKind::GrassRegrowth);

        state
    }

    /// Register a player by name, associating them with the player-controlled
    /// civilization. No-op if a player with that name already exists.
    pub fn register_player(&mut self, name: &str) {
        use crate::db::Player;
        let name = name.to_string();
        if self.db.players.get(&name).is_none() {
            let _ = self.db.insert_player(Player {
                name,
                civ_id: self.player_civ_id,
            });
        }
    }

    /// Set (overwrite) a numbered selection group for a player. If a group
    /// with this number already exists, its contents are replaced; otherwise
    /// a new row is inserted.
    fn set_selection_group(
        &mut self,
        player_name: &str,
        group_number: u8,
        creature_ids: Vec<CreatureId>,
        structure_ids: Vec<StructureId>,
    ) {
        if !(1..=9).contains(&group_number) {
            return;
        }
        // Find existing row for this player + group_number.
        let existing_id = self.find_selection_group_id(player_name, group_number);
        if let Some(id) = existing_id {
            if let Some(mut g) = self.db.selection_groups.get(&id) {
                g.creature_ids = creature_ids;
                g.structure_ids = structure_ids;
                let _ = self.db.update_selection_group(g);
            }
        } else {
            let pn = player_name.to_string();
            let _ = self
                .db
                .insert_selection_group_auto(|id| crate::db::SelectionGroup {
                    id,
                    player_name: pn,
                    group_number,
                    creature_ids,
                    structure_ids,
                });
        }
    }

    /// Add creatures and structures to a numbered selection group for a player.
    /// If the group doesn't exist, it is created. Duplicates are ignored.
    fn add_to_selection_group(
        &mut self,
        player_name: &str,
        group_number: u8,
        creature_ids: Vec<CreatureId>,
        structure_ids: Vec<StructureId>,
    ) {
        if !(1..=9).contains(&group_number) {
            return;
        }
        let existing_id = self.find_selection_group_id(player_name, group_number);
        if let Some(id) = existing_id {
            if let Some(mut g) = self.db.selection_groups.get(&id) {
                for cid in &creature_ids {
                    if !g.creature_ids.contains(cid) {
                        g.creature_ids.push(*cid);
                    }
                }
                for sid in &structure_ids {
                    if !g.structure_ids.contains(sid) {
                        g.structure_ids.push(*sid);
                    }
                }
                let _ = self.db.update_selection_group(g);
            }
        } else {
            let pn = player_name.to_string();
            let _ = self
                .db
                .insert_selection_group_auto(|id| crate::db::SelectionGroup {
                    id,
                    player_name: pn,
                    group_number,
                    creature_ids,
                    structure_ids,
                });
        }
    }

    /// Find the `SelectionGroupId` for a player + group_number, if it exists.
    fn find_selection_group_id(
        &self,
        player_name: &str,
        group_number: u8,
    ) -> Option<SelectionGroupId> {
        for group in self
            .db
            .selection_groups
            .by_player_name(&player_name.to_string(), tabulosity::QueryOpts::ASC)
        {
            if group.group_number == group_number {
                return Some(group.id);
            }
        }
        None
    }

    /// Return all selection groups for a given player name.
    pub fn get_selection_groups(
        &self,
        player_name: &str,
    ) -> Vec<(u8, Vec<CreatureId>, Vec<StructureId>)> {
        self.db
            .selection_groups
            .by_player_name(&player_name.to_string(), tabulosity::QueryOpts::ASC)
            .into_iter()
            .map(|g| {
                (
                    g.group_number,
                    g.creature_ids.clone(),
                    g.structure_ids.clone(),
                )
            })
            .collect()
    }

    /// Apply a batch of commands and advance the sim to the target tick,
    /// processing all scheduled events up to that point.
    ///
    /// Commands must be sorted by tick. Commands with tick > `target_tick`
    /// are ignored (caller error).
    pub fn step(&mut self, commands: &[SimCommand], target_tick: u64) -> StepResult {
        self.mana_wasted_positions.clear();
        let mut events = Vec::new();

        // Index into the sorted command slice.
        let mut cmd_idx = 0;

        while self.tick < target_tick {
            // Determine the next thing to process: the next scheduled event,
            // the next command, or the next creature activation — whichever
            // comes first.
            let next_event_tick = self.event_queue.peek_tick();
            let next_cmd_tick = commands
                .get(cmd_idx)
                .filter(|c| c.tick <= target_tick)
                .map(|c| c.tick);
            let next_activation_tick = match self.next_creature_activation_tick() {
                NextCreatureActivation::NoCreatures => None,
                NextCreatureActivation::Immediate => Some(self.tick),
                NextCreatureActivation::AtTick(t) => Some(t),
            };

            let next_tick = [next_event_tick, next_cmd_tick, next_activation_tick]
                .into_iter()
                .flatten()
                .min()
                .map_or(target_tick, |t| t.min(target_tick));

            self.tick = next_tick;

            // Apply commands at this tick.
            while cmd_idx < commands.len() && commands[cmd_idx].tick <= self.tick {
                let cmd = &commands[cmd_idx];
                cmd_idx += 1;
                self.apply_command(cmd, &mut events);
            }

            // Process scheduled events at this tick (heartbeats, projectiles,
            // etc.).
            while let Some(event) = self.event_queue.pop_if_ready(self.tick) {
                self.process_event(event.kind, &mut events);
            }

            // Poll-based creature activation: find all living creatures whose
            // next_available_tick <= current tick and activate them in
            // CreatureId order for deterministic intra-tick ordering.
            // process_creature_activation immediately advances each creature's
            // next_available_tick to tick+1 (or later), preventing re-polling
            // at the same tick.
            let ready = self.poll_ready_creatures(self.tick);
            for creature_id in ready {
                self.process_creature_activation(creature_id, &mut events);
            }
        }

        self.tick = target_tick;
        self.world.sim_tick = self.tick;
        StepResult { events }
    }

    /// Apply a single command to the simulation.
    ///
    /// This is `pub(crate)` so that `GameSession` can apply commands
    /// immediately on receipt, without waiting for the next `step()` call.
    pub(crate) fn apply_command(&mut self, cmd: &SimCommand, events: &mut Vec<SimEvent>) {
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
                if let Some(mut s) = self.db.structures.get(structure_id) {
                    s.name = name.clone();
                    let _ = self.db.update_structure(s);
                }
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
                if let Some(mut s) = self.db.structures.get(structure_id) {
                    s.logistics_priority = *priority;
                    let _ = self.db.update_structure(s);
                }
            }
            SimAction::SetLogisticsWants {
                structure_id,
                wants,
            } => {
                let inv_id = self.structure_inv(*structure_id);
                self.set_inv_wants(inv_id, wants);
            }
            SimAction::SetCreatureFood { creature_id, food } => {
                if let Some(mut creature) = self.db.creatures.get(creature_id) {
                    creature.food = *food;
                    let _ = self.db.update_creature(creature);
                }
            }
            SimAction::SetCreatureRest { creature_id, rest } => {
                if let Some(mut creature) = self.db.creatures.get(creature_id) {
                    creature.rest = *rest;
                    let _ = self.db.update_creature(creature);
                }
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
            SimAction::DebugNotification { message } => {
                self.add_notification(message.clone());
            }
            SimAction::SetCraftingEnabled {
                structure_id,
                enabled,
            } => {
                self.set_crafting_enabled(*structure_id, *enabled);
            }
            SimAction::AddActiveRecipe {
                structure_id,
                recipe,
                material,
            } => {
                self.add_active_recipe(*structure_id, *recipe, *material);
            }
            SimAction::RemoveActiveRecipe { active_recipe_id } => {
                self.remove_active_recipe(*active_recipe_id);
            }
            SimAction::SetRecipeOutputTarget {
                active_recipe_target_id,
                target_quantity,
            } => {
                self.set_recipe_output_target(*active_recipe_target_id, *target_quantity);
            }
            SimAction::SetRecipeAutoLogistics {
                active_recipe_id,
                auto_logistics,
                spare_iterations,
            } => {
                self.set_recipe_auto_logistics(
                    *active_recipe_id,
                    *auto_logistics,
                    *spare_iterations,
                );
            }
            SimAction::SetRecipeEnabled {
                active_recipe_id,
                enabled,
            } => {
                self.set_recipe_enabled(*active_recipe_id, *enabled);
            }
            SimAction::MoveActiveRecipeUp { active_recipe_id } => {
                self.move_active_recipe_up(*active_recipe_id);
            }
            SimAction::MoveActiveRecipeDown { active_recipe_id } => {
                self.move_active_recipe_down(*active_recipe_id);
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
            SimAction::AttackCreature {
                attacker_id,
                target_id,
                queue,
            } => {
                self.command_attack_creature(*attacker_id, *target_id, *queue, events);
            }
            SimAction::DirectedGoTo {
                creature_id,
                position,
                queue,
            } => {
                self.command_directed_goto(*creature_id, *position, *queue, events);
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
            SimAction::CreateMilitaryGroup { name } => {
                self.create_military_group(name.clone(), events);
            }
            SimAction::DeleteMilitaryGroup { group_id } => {
                self.delete_military_group(*group_id, events);
            }
            SimAction::ReassignMilitaryGroup {
                creature_id,
                group_id,
            } => {
                self.reassign_military_group(*creature_id, *group_id);
            }
            SimAction::RenameMilitaryGroup { group_id, name } => {
                self.rename_military_group(*group_id, name.clone());
            }
            SimAction::SetGroupEngagementStyle {
                group_id,
                engagement_style,
            } => {
                self.set_group_engagement_style(*group_id, *engagement_style);
            }
            SimAction::SetGroupEquipmentWants { group_id, wants } => {
                self.set_group_equipment_wants(*group_id, wants.clone());
            }
            SimAction::TriggerRaid => {
                self.trigger_raid(events);
            }
            SimAction::DebugSpawnProjectile {
                origin,
                target,
                shooter_id,
            } => {
                self.spawn_projectile(*origin, *target, *shooter_id);
            }
            SimAction::AttackMove {
                creature_id,
                destination,
                queue,
            } => {
                self.command_attack_move(*creature_id, *destination, *queue, events);
            }
            SimAction::GroupGoTo {
                creature_ids,
                position,
                queue,
            } => {
                self.command_group_goto(creature_ids, *position, *queue, events);
            }
            SimAction::GroupAttackMove {
                creature_ids,
                destination,
                queue,
            } => {
                self.command_group_attack_move(creature_ids, *destination, *queue, events);
            }
            SimAction::SetSelectionGroup {
                group_number,
                creature_ids,
                structure_ids,
            } => {
                self.set_selection_group(
                    &cmd.player_name,
                    *group_number,
                    creature_ids.clone(),
                    structure_ids.clone(),
                );
            }
            SimAction::AddToSelectionGroup {
                group_number,
                creature_ids,
                structure_ids,
            } => {
                self.add_to_selection_group(
                    &cmd.player_name,
                    *group_number,
                    creature_ids.clone(),
                    structure_ids.clone(),
                );
            }

            // --- Group activity commands ---
            SimAction::CreateActivity {
                kind,
                location,
                min_count,
                desired_count,
                origin,
            } => {
                self.handle_create_activity(
                    *kind,
                    *location,
                    *min_count,
                    *desired_count,
                    *origin,
                    events,
                );
            }
            SimAction::CancelActivity { activity_id } => {
                self.handle_cancel_activity(*activity_id, events);
            }
            SimAction::AssignToActivity {
                activity_id,
                creature_id,
            } => {
                self.handle_assign_to_activity(*activity_id, *creature_id, events);
            }
            SimAction::RemoveFromActivity {
                activity_id,
                creature_id,
            } => {
                self.handle_remove_from_activity(*activity_id, *creature_id, events);
            }

            SimAction::AssignPath {
                creature_id,
                path_id,
            } => {
                self.assign_path(*creature_id, *path_id, events);
            }
            SimAction::StartDebugDance => {
                self.handle_start_debug_dance(events);
            }
            SimAction::DesignateTame { target_id } => {
                self.handle_designate_tame(*target_id, events);
            }
            SimAction::CancelTameDesignation { target_id } => {
                self.handle_cancel_tame_designation(*target_id);
            }
        }
    }

    /// Process a single scheduled event.
    fn process_event(&mut self, kind: ScheduledEventKind, events: &mut Vec<SimEvent>) {
        match kind {
            ScheduledEventKind::CreatureHeartbeat { creature_id } => {
                // Check vital status: dead → no-op, incapacitated → bleed tick,
                // alive → normal heartbeat.
                let vital_status = match self.db.creatures.get(&creature_id) {
                    Some(c) => c.vital_status,
                    None => return,
                };
                match vital_status {
                    VitalStatus::Dead => return,
                    VitalStatus::Incapacitated => {
                        self.process_incapacitated_heartbeat(creature_id, events);
                        return;
                    }
                    VitalStatus::Alive => {} // continue with normal heartbeat
                }

                // Heartbeat is for periodic non-movement checks (mood, mana, etc.).
                // Movement is driven by poll-based activation, not heartbeats.

                // Phase 1: apply food and rest decay, read state for need checks.
                let (
                    should_seek_dining,
                    should_preempt_for_dining,
                    should_seek_food,
                    should_seek_sleep,
                    starved,
                ) = if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                    let species = creature.species;
                    let species_data = &self.species_table[&species];
                    let interval = species_data.heartbeat_interval_ticks;

                    // Food decay.
                    let food_decay = species_data.food_decay_per_tick * interval as i64;
                    creature.food = (creature.food - food_decay).max(0);

                    // Rest decay.
                    let rest_decay = species_data.rest_decay_per_tick * interval as i64;
                    creature.rest = (creature.rest - rest_decay).max(0);

                    // HP regeneration: batch-apply over the heartbeat interval.
                    // ticks_per_hp_regen=0 means no regen; otherwise regen
                    // = interval / ticks_per_hp_regen HP (integer division).
                    if species_data.ticks_per_hp_regen > 0 {
                        let hp_regen = (interval / species_data.ticks_per_hp_regen) as i64;
                        creature.hp = (creature.hp + hp_regen).min(creature.hp_max);
                    }

                    // Mana generation: batch-apply mana_per_tick over the
                    // heartbeat interval, scaled by avg(WIL, INT). Excess
                    // beyond mp_max overflows to the bonded tree.
                    let mana_gen = if species_data.mana_per_tick > 0 {
                        let wil = self.trait_int(creature_id, TraitKind::Willpower, 0);
                        let int = self.trait_int(creature_id, TraitKind::Intelligence, 0);
                        let avg_wil_int = (wil + int) / 2;
                        let scaled_mpt = crate::stats::apply_stat_multiplier(
                            species_data.mana_per_tick,
                            avg_wil_int,
                        );
                        scaled_mpt * interval as i64
                    } else {
                        0
                    };
                    let mp_before = creature.mp;
                    let mp_after = (mp_before + mana_gen).min(creature.mp_max);
                    let mana_overflow = (mp_before + mana_gen) - mp_after;
                    creature.mp = mp_after;
                    let civ_for_overflow = creature.civ_id;

                    let is_starving = creature.food == 0;

                    let dining_threshold =
                        species_data.food_max * species_data.food_dining_threshold_pct / 100;
                    let emergency_threshold =
                        species_data.food_max * species_data.food_hunger_threshold_pct / 100;
                    // Only civilized creatures (those with a civ) use dining
                    // halls. Wild animals forage instead.
                    let wants_dining = creature.civ_id.is_some()
                        && creature.food < dining_threshold
                        && creature.food >= emergency_threshold;
                    let is_hungry = creature.food < emergency_threshold;

                    let rest_threshold =
                        species_data.rest_max * species_data.rest_tired_threshold_pct / 100;
                    let is_tired = creature.rest < rest_threshold;

                    let is_idle = creature.current_task.is_none();
                    let current_task_id = creature.current_task;

                    // Write back mutated fields.
                    let _ = self.db.update_creature(creature);

                    // Overflow mana to the bonded tree (the tree owned by
                    // the creature's civilization). Wild creatures (no civ)
                    // lose their excess.
                    if mana_overflow > 0
                        && mana_gen > 0
                        && let Some(civ_id) = civ_for_overflow
                    {
                        // Convert: overflow fraction × base_generation_rate
                        // gives tree-scale mana (millimana). When fully
                        // overflowing (overflow == mana_gen), the tree gains
                        // exactly mana_base_generation_rate_mm per heartbeat.
                        // Clamp overflow to mana_gen in case mp > mp_max.
                        let clamped_overflow = mana_overflow.min(mana_gen);
                        let tree_gain =
                            clamped_overflow * self.config.mana_base_generation_rate_mm / mana_gen;
                        // Find the tree owned by this civ and add mana to its
                        // GreatTreeInfo row.
                        if let Some(tree) = self
                            .db
                            .trees
                            .by_owner(&Some(civ_id), tabulosity::QueryOpts::ASC)
                            .into_iter()
                            .next()
                        {
                            let tree_id = tree.id;
                            if let Some(mut info) = self.db.great_tree_infos.get(&tree_id) {
                                info.mana_stored =
                                    (info.mana_stored + tree_gain).min(info.mana_capacity);
                                let _ = self.db.update_great_tree_info(info);
                            }
                        }
                    }

                    // Expire old thoughts.
                    self.expire_creature_thoughts(creature_id);

                    // Social opinion decay: probabilistic per-heartbeat roll.
                    let decay_ppm = self.config.social.opinion_decay_chance_ppm;
                    if decay_ppm > 0 {
                        let roll = self.rng.next_u64() % 1_000_000;
                        if roll < decay_ppm as u64 {
                            self.decay_opinions(creature_id);
                        }
                    }

                    // Casual social interaction: probabilistic per-heartbeat
                    // roll triggers a quick impression exchange with a nearby
                    // same-civ creature (F-casual-social).
                    let social_ppm = self.config.social.casual_social_chance_ppm;
                    if social_ppm > 0 {
                        let roll = self.rng.next_u64() % 1_000_000;
                        if roll < social_ppm as u64 {
                            self.try_casual_social(creature_id);
                        }
                    }

                    // Reschedule the next heartbeat.
                    let next_tick = self.tick + interval;
                    self.event_queue.schedule(
                        next_tick,
                        ScheduledEventKind::CreatureHeartbeat { creature_id },
                    );

                    // Hunger takes priority over tiredness. Dining hunger
                    // also takes priority over tiredness (elf prefers dining
                    // hall meal over sleeping when both apply).
                    //
                    // Dining can preempt lower-priority tasks (Autonomous)
                    // because it's Survival-level. Without this, elves busy
                    // hauling would skip the entire dining window and fall
                    // through to emergency eating.
                    let can_preempt_for_dining = wants_dining
                        && !is_idle
                        && current_task_id.is_some_and(|tid| {
                            self.db.tasks.get(&tid).is_some_and(|t| {
                                crate::preemption::preemption_level(t.kind_tag, t.origin).level()
                                    < crate::preemption::PreemptionLevel::Survival.level()
                            })
                        });
                    let seek_dining = wants_dining && (is_idle || can_preempt_for_dining);
                    let seek_food = is_hungry && is_idle;
                    let seek_sleep = is_tired && is_idle && !is_hungry && !wants_dining;

                    (
                        seek_dining,
                        can_preempt_for_dining,
                        seek_food,
                        seek_sleep,
                        is_starving,
                    )
                } else {
                    (false, false, false, false, false)
                };

                // Starvation death: food reached zero.
                if starved {
                    self.handle_creature_death(creature_id, DeathCause::Starvation, events);
                    return;
                }

                // Phase 2a-dining: if moderately hungry, seek a dining hall
                // with a free seat and stocked food. Preempts lower-priority
                // tasks (e.g., hauling) since dining is Survival-level.
                if should_seek_dining
                    && let Some((table_coord, _nav_node, structure_id)) =
                        self.find_nearest_dining_hall(creature_id)
                {
                    // Verify unreserved food exists before creating any DB rows.
                    // find_nearest_dining_hall already checks this, but we
                    // confirm here so we never insert a task we'd have to
                    // roll back.
                    let has_food = self.db.structures.get(&structure_id).is_some_and(|s| {
                        inventory::ItemKind::EDIBLE_KINDS.iter().any(|kind| {
                            self.inv_unreserved_item_count(
                                s.inventory_id,
                                *kind,
                                inventory::MaterialFilter::Any,
                            ) > 0
                        })
                    });
                    if has_food {
                        // Insert the task first so the FK on
                        // item_stacks.reserved_by is satisfied.
                        let task_id = TaskId::new(&mut self.rng);
                        let new_task = task::Task {
                            id: task_id,
                            kind: task::TaskKind::DineAtHall { structure_id },
                            state: task::TaskState::InProgress,
                            location: table_coord,
                            progress: 0,
                            total_cost: 0,
                            required_species: None,
                            origin: task::TaskOrigin::Autonomous,
                            target_creature: None,
                            restrict_to_creature_id: None,
                            prerequisite_task_id: None,
                            required_civ_id: None,
                        };
                        self.insert_task(new_task);

                        // Reserve one edible food item in the dining hall.
                        let mut food_reserved = false;
                        if let Some(structure) = self.db.structures.get(&structure_id) {
                            let inv_id = structure.inventory_id;
                            for kind in inventory::ItemKind::EDIBLE_KINDS {
                                let reserved = self.inv_reserve_unowned_items(
                                    inv_id,
                                    *kind,
                                    inventory::MaterialFilter::Any,
                                    1,
                                    task_id,
                                );
                                if reserved > 0 {
                                    food_reserved = true;
                                    break;
                                }
                            }
                        }
                        if food_reserved {
                            // Preemption must happen AFTER confirming
                            // reservation — otherwise a failed reservation
                            // leaves the elf taskless with the old task gone.
                            if should_preempt_for_dining
                                && let Some(tid) = self
                                    .db
                                    .creatures
                                    .get(&creature_id)
                                    .and_then(|c| c.current_task)
                            {
                                self.preempt_task(creature_id, tid);
                            }
                            // Insert DiningSeat voxel ref for seat reservation.
                            let seq = self.db.task_voxel_refs.next_seq();
                            let _ = self.db.insert_task_voxel_ref(crate::db::TaskVoxelRef {
                                seq,
                                task_id,
                                coord: table_coord,
                                role: crate::db::TaskVoxelRole::DiningSeat,
                            });
                            if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                                creature.current_task = Some(task_id);
                                let _ = self.db.update_creature(creature);
                            }
                        } else {
                            // Reservation failed despite the pre-check (should
                            // not happen in single-threaded sim, but defend
                            // against it). Remove the task entirely.
                            let _ = self.db.remove_task(&task_id);
                        }
                    }
                }
                // If no dining hall available, elf remains idle until
                // next heartbeat or until food drops to emergency threshold.

                // Phase 2a-emergency: if emergency-hungry and idle, eat bread
                // from inventory (instant, no travel) or fall back to seeking fruit.
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
                        && let Some(creature_pos) =
                            self.db.creatures.get(&creature_id).map(|c| c.position)
                    {
                        let task_id = TaskId::new(&mut self.rng);
                        let new_task = task::Task {
                            id: task_id,
                            kind: task::TaskKind::EatBread,
                            state: task::TaskState::InProgress,
                            location: creature_pos,
                            progress: 0,
                            total_cost: 0,
                            required_species: None,
                            origin: task::TaskOrigin::Autonomous,
                            target_creature: None,
                            restrict_to_creature_id: None,
                            prerequisite_task_id: None,
                            required_civ_id: None,
                        };
                        self.insert_task(new_task);
                        if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                            creature.current_task = Some(task_id);
                            let _ = self.db.update_creature(creature);
                        }
                        ate_bread = true;
                    }
                }

                // Herbivores try grazing before fruit. If grass is depleted,
                // herbivores do NOT fall back to fruit — they wander and starve.
                // Fruit fallback is only for non-herbivore species.
                let is_herbivore = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .map(|c| self.species_table[&c.species].is_herbivore)
                    .unwrap_or(false);
                let mut started_grazing = false;
                if should_seek_food
                    && !ate_bread
                    && is_herbivore
                    && let Some((grass_pos, _nav_node)) = self.find_nearest_grass(creature_id)
                {
                    let task_id = TaskId::new(&mut self.rng);
                    let new_task = task::Task {
                        id: task_id,
                        kind: task::TaskKind::Graze { grass_pos },
                        state: task::TaskState::InProgress,
                        location: grass_pos,
                        progress: 0,
                        total_cost: 0,
                        required_species: None,
                        origin: task::TaskOrigin::Autonomous,
                        target_creature: None,
                        restrict_to_creature_id: None,
                        prerequisite_task_id: None,
                        required_civ_id: None,
                    };
                    self.insert_task(new_task);
                    if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                        creature.current_task = Some(task_id);
                        let _ = self.db.update_creature(creature);
                    }
                    started_grazing = true;
                }

                // Fall back to seeking fruit if no bread was available.
                // Herbivores do NOT eat fruit — they only graze. Fruit
                // foraging for herbivores belongs to F-wild-foraging.
                if should_seek_food
                    && !ate_bread
                    && !started_grazing
                    && !is_herbivore
                    && let Some((fruit_pos, _nav_node)) = self.find_nearest_fruit(creature_id)
                {
                    let task_id = TaskId::new(&mut self.rng);
                    let new_task = task::Task {
                        id: task_id,
                        kind: task::TaskKind::EatFruit { fruit_pos },
                        state: task::TaskState::InProgress,
                        location: fruit_pos,
                        progress: 0,
                        total_cost: 0,
                        required_species: None,
                        origin: task::TaskOrigin::Autonomous,
                        target_creature: None,
                        restrict_to_creature_id: None,
                        prerequisite_task_id: None,
                        required_civ_id: None,
                    };
                    self.insert_task(new_task);
                    if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                        creature.current_task = Some(task_id);
                        let _ = self.db.update_creature(creature);
                    }
                }

                // Phase 2b: if tired and idle (and not hungry), find a bed
                // or fall back to sleeping on the ground.
                // Priority: assigned home bed → dormitory bed → ground.
                if should_seek_sleep {
                    let (bed_pos, task_coord, sleep_ticks, sleep_location) =
                        if let Some((bp, _nn, sid)) = self.find_assigned_home_bed(creature_id) {
                            (
                                Some(bp),
                                bp,
                                self.config.sleep_ticks_bed,
                                task::SleepLocation::Home(sid),
                            )
                        } else if let Some((bp, _nn, sid)) = self.find_nearest_bed(creature_id) {
                            (
                                Some(bp),
                                bp,
                                self.config.sleep_ticks_bed,
                                task::SleepLocation::Dormitory(sid),
                            )
                        } else if let Some(creature) = self.db.creatures.get(&creature_id)
                            && self
                                .graph_for_species(creature.species)
                                .node_at(creature.position)
                                .is_some()
                        {
                            (
                                None,
                                creature.position,
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
                        location: task_coord,
                        progress: 0,
                        total_cost: (sleep_ticks / self.config.sleep_action_ticks) as i64,
                        required_species: None,
                        origin: task::TaskOrigin::Autonomous,
                        target_creature: None,
                        restrict_to_creature_id: None,
                        prerequisite_task_id: None,
                        required_civ_id: None,
                    };
                    self.insert_task(new_task);
                    if let Some(mut creature) = self.db.creatures.get(&creature_id) {
                        creature.current_task = Some(task_id);
                        let _ = self.db.update_creature(creature);
                    }
                }

                // Phase 2b½: mood-based moping check. Only applies to elves
                // (only species with meaningful thoughts currently).
                self.check_mope(creature_id);

                // Phase 2b¾: military equipment — drop unwanted items, then
                // acquire missing equipment if idle.
                self.military_equipment_drop(creature_id);
                let still_idle_for_equip = self
                    .db
                    .creatures
                    .get(&creature_id)
                    .is_some_and(|c| c.current_task.is_none());
                if still_idle_for_equip {
                    self.check_military_equipment_wants(creature_id);
                }

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
            ScheduledEventKind::_LegacyCreatureActivation { .. } => {
                // Legacy event from old save files — ignored.
                // Poll-based activation replaced this in F-activation-revamp.
            }
            ScheduledEventKind::TreeHeartbeat { tree_id } => {
                if self.db.trees.contains(&tree_id) {
                    // Fruit production.
                    self.attempt_fruit_spawn(tree_id);

                    // Tree mana accumulates from elf overflow in
                    // CreatureHeartbeat; trees do not generate mana.

                    // Reschedule.
                    let next_tick = self.tick + self.config.tree_heartbeat_interval_ticks;
                    self.event_queue
                        .schedule(next_tick, ScheduledEventKind::TreeHeartbeat { tree_id });
                }
            }
            ScheduledEventKind::LogisticsHeartbeat => {
                // Periodic gravity sweep: drop any floating piles and
                // unsupported creatures before logistics processing.
                self.apply_pile_gravity();
                self.apply_creature_gravity(events);
                self.process_logistics_heartbeat();
                let next_tick = self.tick + self.config.logistics_heartbeat_interval_ticks;
                self.event_queue
                    .schedule(next_tick, ScheduledEventKind::LogisticsHeartbeat);
            }
            ScheduledEventKind::ProjectileTick => {
                self.process_projectile_tick(events);
            }
            ScheduledEventKind::GrassRegrowth => {
                self.process_grass_regrowth();
                let next_tick = self.tick + self.config.grass_regrowth_interval_ticks;
                self.event_queue
                    .schedule(next_tick, ScheduledEventKind::GrassRegrowth);
            }
        }
    }

    /// Process an incapacitated creature's heartbeat: apply HP regen (if the
    /// species has it) minus 1 HP bleed-out. If regen outpaces bleeding, the
    /// creature recovers to Alive (e.g., trolls). Death at HP <= -hp_max.
    fn process_incapacitated_heartbeat(
        &mut self,
        creature_id: CreatureId,
        events: &mut Vec<SimEvent>,
    ) {
        enum Outcome {
            Die,
            Revive,
            StillDown,
        }

        let outcome = if let Some(mut c) = self.db.creatures.get(&creature_id) {
            let species_data = &self.species_table[&c.species];
            let interval = species_data.heartbeat_interval_ticks;

            // HP regen (same formula as the alive heartbeat).
            let hp_regen = if species_data.ticks_per_hp_regen > 0 {
                (interval / species_data.ticks_per_hp_regen) as i64
            } else {
                0
            };

            // Net change: regen minus 1 HP bleed-out, clamped to hp_max.
            c.hp = (c.hp + hp_regen - 1).min(c.hp_max);

            let outcome = if c.hp <= -c.hp_max {
                Outcome::Die
            } else if c.hp > 0 {
                c.vital_status = VitalStatus::Alive;
                Outcome::Revive
            } else {
                Outcome::StillDown
            };

            let _ = self.db.update_creature(c);

            // Reschedule heartbeat unless dying.
            if !matches!(outcome, Outcome::Die) {
                self.event_queue.schedule(
                    self.tick + interval,
                    ScheduledEventKind::CreatureHeartbeat { creature_id },
                );
            }

            outcome
        } else {
            return;
        };

        match outcome {
            Outcome::Die => {
                self.handle_creature_death(creature_id, DeathCause::Damage, events);
            }
            Outcome::Revive => {
                // Restart the activation chain so the creature can act again.
                self.schedule_reactivation(creature_id);
            }
            Outcome::StillDown => {}
        }
    }

    /// Start a simple action with a given kind and duration. Used for one-shot
    /// actions (Eat, Harvest, AcquireItem) that need no extra setup logic.
    fn start_simple_action(
        &mut self,
        creature_id: CreatureId,
        action_kind: ActionKind,
        duration: u64,
    ) {
        if let Some(mut c) = self.db.creatures.get(&creature_id) {
            c.action_kind = action_kind;
            c.next_available_tick = Some(self.tick + duration);
            let _ = self.db.update_creature(c);
        }
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
                // DineAtHall resolves instantly on arrival (in activation),
                // so it never reaches here.
                match task_kind_tag {
                    Some(crate::db::TaskKindTag::EatFruit) => {
                        let fruit_pos = self
                            .task_voxel_ref(tid, crate::db::TaskVoxelRole::FruitTarget)
                            .expect("EatFruit task missing FruitTarget voxel ref");
                        self.resolve_eat_fruit_action(creature_id, tid, fruit_pos)
                    }
                    _ => self.resolve_eat_bread_action(creature_id, tid),
                }
            }
            ActionKind::Graze => {
                let tid = match task_id {
                    Some(t) => t,
                    None => return false,
                };
                let grass_pos = self
                    .task_voxel_ref(tid, crate::db::TaskVoxelRole::GrazeTarget)
                    .expect("Graze task missing GrazeTarget voxel ref");
                self.resolve_graze_action(creature_id, tid, grass_pos)
            }
            ActionKind::Harvest => {
                let tid = match task_id {
                    Some(t) => t,
                    None => return false,
                };
                let fruit_pos = self
                    .task_voxel_ref(tid, crate::db::TaskVoxelRole::FruitTarget)
                    .expect("Harvest task missing FruitTarget voxel ref");
                self.resolve_harvest_action(creature_id, tid, fruit_pos)
            }
            ActionKind::AcquireItem => {
                let tid = match task_id {
                    Some(t) => t,
                    None => return false,
                };
                self.resolve_acquire_item_action(creature_id, tid)
            }
            ActionKind::AcquireMilitaryEquipment => {
                let tid = match task_id {
                    Some(t) => t,
                    None => return false,
                };
                self.resolve_acquire_military_equipment_action(creature_id, tid)
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
            let _ = self.db.update_task(task);
        }

        for cid in assignees.iter().map(|c| &c.id) {
            if let Some(mut creature) = self.db.creatures.get(cid) {
                creature.current_task = None;
                creature.path = None;
                let _ = self.db.update_creature(creature);
            }
        }
    }

    /// Look up the fruit species at a voxel position.
    ///
    /// Returns the full `FruitSpecies` record, or `None` if the voxel has no
    /// tracked species (pre-fruit-variety fruit or empty).
    pub fn fruit_species_at(&self, pos: VoxelCoord) -> Option<crate::fruit::FruitSpecies> {
        let species_id = self.fruit_voxel_species.get(&pos)?;
        self.db.fruit_species.get(species_id)
    }

    /// Resolve the visual color for an item stack. Priority:
    /// 1. Explicit `dye_color` on the stack → used directly.
    /// 2. Fruit-species material → fruit's `exterior_color`, muted.
    /// 3. Other material (wood) → `Material::base_color()`, muted.
    /// 4. No material → `DEFAULT_ITEM_COLOR`.
    pub fn item_color(&self, stack: &crate::db::ItemStack) -> inventory::ItemColor {
        // Dyed items use the dye color as-is.
        if let Some(dye) = stack.dye_color {
            return dye;
        }
        match stack.material {
            Some(inventory::Material::FruitSpecies(id)) => {
                if let Some(species) = self.db.fruit_species.get(&id) {
                    inventory::ItemColor::from(species.appearance.exterior_color).muted()
                } else {
                    // Unknown fruit species — fall back to generic fruit color, muted.
                    inventory::Material::FruitSpecies(id).base_color().muted()
                }
            }
            Some(mat) => mat.base_color().muted(),
            None => inventory::DEFAULT_ITEM_COLOR,
        }
    }

    /// Build the full display name for an item stack.
    ///
    /// Format: `[Quality] [DyeColor] [Material/Species] ItemKind [suffixes]`.
    ///
    /// Examples:
    /// - `"Crude Blue Oak Bow (worn)"`
    /// - `"Fine Red Oak Breastplate"`
    /// - `"Superior Tunic (equipped)"`
    /// - `"Fine Shinethúni Pod"`
    /// - `"Crude Bread"`
    ///
    /// Quality prefix comes from `inventory::quality_label()`. Dye color
    /// names come from `ItemColor::display_name()`. Material prefix uses
    /// `Material::display_name()` for wood types; fruit-species items use
    /// the Vaelith species name. Suffixes: "(equipped)" if in a slot,
    /// "(worn)"/"(damaged)" if durability is below threshold.
    pub fn item_display_name(&self, stack: &crate::db::ItemStack) -> String {
        let mut name = String::new();

        // Quality prefix (e.g., "Crude", "Fine", "Superior").
        if let Some(label) = inventory::quality_label(stack.quality) {
            name.push_str(label);
            name.push(' ');
        }

        // Dye color prefix (only for explicitly dyed items).
        if let Some(dye) = stack.dye_color {
            name.push_str(dye.display_name());
            name.push(' ');
        }

        // Material/species + item kind.
        if let Some(inventory::Material::FruitSpecies(id)) = stack.material
            && let Some(species) = self.db.fruit_species.get(&id)
        {
            if stack.kind == inventory::ItemKind::Fruit {
                let noun = species.appearance.shape.item_noun();
                name.push_str(&format!("{} {}", species.vaelith_name, noun));
            } else {
                name.push_str(&format!(
                    "{} {}",
                    species.vaelith_name,
                    stack.kind.display_name()
                ));
            }
        } else if let Some(mat) = stack.material
            && mat.is_wood()
        {
            name.push_str(mat.display_name());
            name.push(' ');
            name.push_str(stack.kind.display_name());
        } else {
            name.push_str(stack.kind.display_name());
        }

        if stack.equipped_slot.is_some() {
            name.push_str(" (equipped)");
        }
        if let Some(label) = Self::condition_label(
            stack.current_hp,
            stack.max_hp,
            self.config.durability_worn_pct,
            self.config.durability_damaged_pct,
        ) {
            name.push(' ');
            name.push_str(label);
        }
        name
    }

    /// Return the condition label for an item based on its HP ratio, or `None`
    /// if the item is at full health or indestructible.
    pub fn condition_label(
        current_hp: i32,
        max_hp: i32,
        worn_pct: i32,
        damaged_pct: i32,
    ) -> Option<&'static str> {
        match inventory::WearCategory::from_hp(current_hp, max_hp, worn_pct, damaged_pct) {
            inventory::WearCategory::Good => None,
            inventory::WearCategory::Worn => Some("(worn)"),
            inventory::WearCategory::Damaged => Some("(damaged)"),
        }
    }

    /// Display name for an item kind + specific material combination. For fruit
    /// species, uses the Vaelith name + shape noun (for Fruit) or
    /// "SpeciesName ItemType" (for all other fruit-species items). For wood
    /// materials, uses "Oak Bow" etc. Falls back to the item kind's display name.
    pub fn material_item_display_name(
        &self,
        kind: inventory::ItemKind,
        material: inventory::Material,
    ) -> String {
        if let inventory::Material::FruitSpecies(id) = material
            && let Some(species) = self.db.fruit_species.get(&id)
        {
            if kind == inventory::ItemKind::Fruit {
                let noun = species.appearance.shape.item_noun();
                return format!("{} {}", species.vaelith_name, noun);
            }
            // All other fruit-species items: "SpeciesName ItemType".
            return format!("{} {}", species.vaelith_name, kind.display_name());
        }
        format!("{} {}", material.display_name(), kind.display_name())
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
        let seq = self.db.thoughts.next_seq();
        let _ = self.db.insert_thought(crate::db::Thought {
            creature_id,
            seq,
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
            let _ = self
                .db
                .remove_thought(&(thoughts[0].creature_id, thoughts[0].seq));
        }
    }

    pub(crate) fn add_notification(&mut self, message: String) {
        let _ = self
            .db
            .insert_notification_auto(|id| crate::db::Notification {
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
                let _ = self.db.remove_thought(&(t.creature_id, t.seq));
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

    /// Rebuild all transient (`#[serde(skip)]`) fields after deserialization.
    ///
    /// The voxel world is now serialized directly, so only derived data
    /// structures need rebuilding: `nav_graph` (from world geometry),
    /// `species_table` (from config), `spatial_index` (from creatures +
    /// species footprints), `lexicon` (from embedded JSON),
    /// `structure_voxels` (from completed blueprints + structures).
    pub fn rebuild_transient_state(&mut self) {
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

    /// Rebuild the spatial index from scratch using all creatures in the DB.
    /// Must be called after `species_table` is populated (footprint lookups
    /// depend on `SpeciesData`).
    fn rebuild_spatial_index(&mut self) {
        self.spatial_index.clear();
        let entries: Vec<(CreatureId, Species, VoxelCoord)> = self
            .db
            .creatures
            .iter_all()
            .filter(|c| c.vital_status != VitalStatus::Dead)
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
        // Backfill Outcast path for elves from old saves that predate the
        // path system (F-path-core).
        state.backfill_outcast_paths();
        // Backfill GrassRegrowth event for saves that predate F-wild-grazing.
        state.backfill_grass_regrowth_event();
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
                    player_name: String::new(),
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
mod tests;
