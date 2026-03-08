// Task creation types — DTOs for constructing tasks before DB insertion.
//
// This file defines `Task`, `TaskKind`, `TaskState`, `TaskOrigin`, and
// supporting enums (`HaulSource`, `HaulPhase`, `SleepLocation`) used when
// creating tasks. At runtime, task data is stored in the tabulosity DB:
// the base row lives in `db::Task` (with `kind_tag` discriminant), and
// variant-specific data lives in extension tables (`task_blueprint_refs`,
// `task_structure_refs`, `task_voxel_refs`, `task_haul_data`,
// `task_sleep_data`, `task_acquire_data`). See `db.rs` for the table
// definitions and `sim.rs` `insert_task()` for the decomposition logic.
//
// The `Task` struct here serves as a **creation DTO**: callers construct a
// `task::Task` with the full `TaskKind` enum, pass it to `insert_task()`,
// which decomposes it into DB tables. The `TaskKind` enum is never
// reconstructed from the DB — reads use query helpers on `SimState`.
//
// ## Data model
//
// A `Task` has a `kind` (`TaskKind` enum), a `state` (`TaskState` lifecycle),
// a `location` (nav node where work happens), and progress tracking. Tasks
// with nonzero `total_cost` track `progress` toward completion; tasks with
// `total_cost == 0.0` (like `GoTo`) complete instantly on arrival.
//
// `required_species` optionally restricts which species can claim the task.
// If `Some(Species::Elf)`, only elves will pick it up. If `None`, any idle
// creature of any species can claim it.
//
// ## Task kinds and behavior scripts
//
// Each `TaskKind` defines a per-activation behavior script, dispatched via
// match in `sim.rs` `execute_task_behavior()`:
//
// - `GoTo` — walk toward `location`; complete instantly on arrival. Used by
//   the "Summon Elf" UI button to direct an elf to a clicked location.
// - `Build { project_id }` — walk to the build site, then do incremental
//   work. Each activation adds 1.0 to progress; every
//   `build_work_ticks_per_voxel` units, one blueprint voxel materializes.
//   Linked to a `Blueprint` via `project_id`. See `sim.rs` `do_build_work()`.
// - `EatBread` — eat bread from inventory, restoring food. Created
//   automatically by the heartbeat hunger check when a creature has owned
//   bread. Completes instantly at the creature's current node (no travel).
//   `do_eat_bread()` removes 1 bread from inventory, restores
//   `bread_restore_pct`% of `food_max`, and completes the task. Takes
//   priority over `EatFruit` since no travel is needed.
// - `EatFruit { fruit_pos }` — walk to a fruit voxel and eat it, restoring
//   food. Created automatically by the heartbeat hunger check when a
//   creature's food drops below `food_hunger_threshold_pct`. On arrival,
//   `do_eat_fruit()` restores `food_restore_pct`% of `food_max`, removes
//   the fruit from the world and tree, and completes the task.
// - `Sleep { bed_pos, location }` — sleep to restore rest. Created automatically
//   by the heartbeat tiredness check when rest drops below
//   `rest_tired_threshold_pct`. `bed_pos` is `Some(pos)` for bed sleep, `None`
//   for ground sleep (fallback). `location` is a `SleepLocation` enum
//   (Home/Dormitory/Ground) used to determine which thought to generate on
//   completion. Multi-activation: each activation restores rest proportional to
//   `rest_per_sleep_tick`; completes when progress reaches `total_cost` or rest
//   is full.
// - `Haul { item_kind, quantity, source, destination, phase, destination_nav_node }`
//   — two-phase item transport. In `GoingToSource` phase, creature walks to the
//   source (ground pile or building), picks up reserved items, then switches to
//   `GoingToDestination` and walks to the destination building to deposit them.
//   Items are reserved at source creation to prevent double-claiming. On
//   abandonment: GoingToSource clears reservations; GoingToDestination drops
//   carried items as a ground pile.
// - `Cook { structure_id }` — converts reserved fruit into bread at a kitchen.
//   Created by `process_kitchen_monitor()` when a kitchen has unreserved fruit
//   and `cooking_enabled == true`. Progress increments each tick; on completion,
//   fruit is consumed and bread is added to the kitchen's inventory.
// - `Harvest { fruit_pos }` — walk to a fruit voxel, remove it, and create a
//   ground pile with 1 Fruit at the elf's position. Instant (`total_cost = 0`).
//   Created by `process_harvest_tasks()` when logistics buildings want fruit
//   but not enough fruit items exist as ground piles or building inventory.
//   Bridges the gap between tree fruit voxels and the item-based logistics
//   system.
// - `AcquireItem { source, item_kind, quantity }` — pick up unowned items from
//   a source and add them to the creature's personal inventory with ownership.
//   Single-phase: creature walks to source, picks up reserved items on arrival.
//   Instant (`total_cost = 0`). Created by the heartbeat Phase 2c acquisition
//   check when a creature's inventory is below its personal `wants` target.
//   On abandonment, reservations are cleared at the source.
// - `Mope` — idle at a location due to low mood. Multi-activation: each tick
//   increments progress by 1.0 until reaching `total_cost`. Created by the
//   heartbeat mood check (Phase 2b½) when mood is Unhappy or worse. Location
//   is the creature's assigned home if available, else current node. No side
//   effects beyond consuming the creature's time.
//
// ## Lifecycle
//
// `TaskState` tracks where a task is in its lifecycle:
// - `Available` — no creature is working on it yet. Idle creatures check for
//   these during their activation loop.
// - `InProgress` — at least one creature has claimed it and is walking toward
//   it or doing work. `find_available_task()` in `sim.rs` skips these, so
//   only one creature transitions a task out of `Available`.
// - `Complete` — finished. All assigned creatures have their `current_task`
//   cleared and return to wandering.
//
// See also: `sim.rs` for the activation loop that executes task behavior and
// handles assignment/completion, `types.rs` for `TaskId`, `command.rs` for
// the `CreateTask` command that adds tasks, `sim_bridge.rs` (in the gdext
// crate) for the GDScript-facing `create_goto_task()` wrapper.
//
// **Critical constraint: determinism.** Tasks are stored in tabulosity tables
// (BTreeMap-backed) and iterated in deterministic order. Task IDs come from
// the sim PRNG.

use crate::inventory::ItemKind;
use crate::types::{NavNodeId, ProjectId, Species, StructureId, TaskId, VoxelCoord};
use serde::{Deserialize, Serialize};

/// Where a haul task picks up items from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HaulSource {
    GroundPile(VoxelCoord),
    Building(StructureId),
}

/// Which phase a haul task is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HaulPhase {
    GoingToSource,
    GoingToDestination,
}

/// Where a creature slept. Recorded in `TaskKind::Sleep` to determine which
/// thought to generate on sleep completion. `Ground` is the default for
/// backward compatibility with old saves that lack this field.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum SleepLocation {
    Home(StructureId),
    Dormitory(StructureId),
    #[default]
    Ground,
}

/// The type of work a task represents. Each variant carries kind-specific data
/// and defines a behavior script (see `sim.rs` activation loop).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TaskKind {
    /// Walk to a location. Completes instantly on arrival (total_cost = 0).
    GoTo,
    /// Build a structure from a blueprint. The elf pathfinds to the site,
    /// then does work over multiple activations, materializing one voxel
    /// per `build_work_ticks_per_voxel` units of progress.
    Build { project_id: ProjectId },
    /// Eat bread from inventory. Created automatically by the heartbeat hunger
    /// check when a creature has owned bread and is hungry. Completes instantly
    /// at the creature's current location (no travel needed). Removes 1 bread
    /// from inventory and restores `bread_restore_pct`% of `food_max`.
    EatBread,
    /// Walk to a fruit voxel and eat it, restoring food. Created automatically
    /// by the heartbeat hunger check when a creature's food drops below
    /// `food_hunger_threshold_pct`. The `fruit_pos` is the voxel coordinate
    /// of the fruit to consume (removed from world on arrival).
    EatFruit { fruit_pos: VoxelCoord },
    /// Furnish a completed building. The elf walks to the building interior,
    /// then does incremental work. Each `furnish_work_ticks_per_item` units of
    /// progress, one item is placed from the structure's `planned_furniture`
    /// into `furniture_positions`.
    Furnish { structure_id: StructureId },
    /// Sleep to restore rest. Created automatically by the heartbeat tiredness
    /// check when a creature's rest drops below `rest_tired_threshold_pct`.
    /// `bed_pos` is `Some(pos)` when sleeping in a dormitory bed, or `None`
    /// for ground sleep (fallback when no beds are available). The task is
    /// multi-activation: each activation restores rest, and the task completes
    /// when `progress >= total_cost` or rest reaches `rest_max`.
    /// `location` records where the creature is sleeping for thought generation.
    Sleep {
        bed_pos: Option<VoxelCoord>,
        #[serde(default)]
        location: SleepLocation,
    },
    /// Haul items from a source (ground pile or building) to a destination
    /// building. Multi-phase: creature walks to source, picks up items, walks
    /// to destination, deposits items. Created by the logistics heartbeat.
    Haul {
        item_kind: ItemKind,
        quantity: u32,
        source: HaulSource,
        destination: StructureId,
        phase: HaulPhase,
        destination_nav_node: NavNodeId,
    },
    /// Cook food in a kitchen. An elf walks to the kitchen, works for
    /// `cook_work_ticks`, then converts `cook_fruit_input` fruit into
    /// `cook_bread_output` bread in the kitchen's inventory.
    Cook { structure_id: StructureId },
    /// Harvest a fruit voxel from a tree. The elf walks to the fruit's nav
    /// node, removes the fruit voxel from the world and tree, and creates a
    /// ground pile with 1 Fruit item at the elf's position. Instant
    /// (`total_cost = 0`). Created by `process_harvest_tasks()` when
    /// logistics buildings want fruit but none is available as items.
    Harvest { fruit_pos: VoxelCoord },
    /// Pick up unowned items from a source (ground pile or building) and add
    /// them to the creature's personal inventory with ownership. Single-phase:
    /// creature walks to source, picks up reserved items on arrival, done.
    /// Created by the heartbeat acquisition check (Phase 2c) when an elf's
    /// personal inventory is below its `wants` target for an item kind.
    AcquireItem {
        source: HaulSource,
        item_kind: ItemKind,
        quantity: u32,
    },
    /// Mope at a location due to low mood. Multi-activation: creature idles
    /// at the location for the configured duration. Created by the heartbeat
    /// mood check when mood is Unhappy or worse. Duration comes from
    /// `total_cost` on the Task struct (same pattern as Sleep).
    Mope,
    /// Craft an item at a workshop. An elf walks to the workshop, works for
    /// the recipe's `work_ticks`, then converts reserved inputs into outputs
    /// in the workshop's inventory. Created by `process_workshop_monitor()`
    /// when a workshop has available inputs for a configured recipe.
    Craft {
        structure_id: StructureId,
        recipe_id: String,
    },
}

/// Where a task originated — used by the UI to group tasks into sections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskOrigin {
    /// Explicitly requested by the player (build, goto, furnish, etc.).
    #[default]
    PlayerDirected,
    /// Created automatically by the sim (heartbeat hunger/sleep checks).
    Autonomous,
    /// Created by automated management systems (not yet used).
    Automated,
}

/// Lifecycle state of a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskState {
    /// No one is working on this task yet. Available for assignment.
    Available,
    /// At least one creature is assigned and working.
    InProgress,
    /// Task is finished. Will be cleaned up.
    Complete,
}

/// A task entity — a unit of work that one or more creatures can be assigned to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub state: TaskState,
    /// The nav node where creatures go to work on this task.
    pub location: NavNodeId,
    /// Creatures currently assigned to this task.
    /// Current progress toward completion (0.0 to 1.0).
    pub progress: f32,
    /// Total work units needed to complete. 0.0 for instant tasks (e.g. GoTo).
    pub total_cost: f32,
    /// If set, only creatures of this species can claim this task.
    pub required_species: Option<Species>,
    /// Where this task originated (player command, autonomous decision, etc.).
    #[serde(default)]
    pub origin: TaskOrigin,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::GameRng;

    #[test]
    fn build_task_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let project_id = crate::types::ProjectId::new(&mut rng);
        let location = NavNodeId(3);

        let task = Task {
            id: task_id,
            kind: TaskKind::Build { project_id },
            state: TaskState::Available,
            location,

            progress: 0.0,
            total_cost: 5000.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, task_id);
        match &restored.kind {
            TaskKind::Build { project_id: pid } => assert_eq!(*pid, project_id),
            _ => panic!("Expected Build task kind"),
        }
        assert_eq!(restored.total_cost, 5000.0);
        assert_eq!(restored.required_species, Some(Species::Elf));
        assert_eq!(restored.origin, TaskOrigin::PlayerDirected);
    }

    #[test]
    fn eat_fruit_task_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let location = NavNodeId(7);
        let fruit_pos = VoxelCoord::new(10, 5, 10);

        let task = Task {
            id: task_id,
            kind: TaskKind::EatFruit { fruit_pos },
            state: TaskState::InProgress,
            location,

            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Autonomous,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, task_id);
        match &restored.kind {
            TaskKind::EatFruit { fruit_pos: fp } => assert_eq!(*fp, fruit_pos),
            _ => panic!("Expected EatFruit task kind"),
        }
        assert_eq!(restored.state, TaskState::InProgress);
        assert_eq!(restored.required_species, Some(Species::Elf));
        assert_eq!(restored.origin, TaskOrigin::Autonomous);
    }

    #[test]
    fn eat_bread_task_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let location = NavNodeId(5);

        let task = Task {
            id: task_id,
            kind: TaskKind::EatBread,
            state: TaskState::InProgress,
            location,

            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, task_id);
        assert!(matches!(restored.kind, TaskKind::EatBread));
        assert_eq!(restored.state, TaskState::InProgress);
        assert_eq!(restored.origin, TaskOrigin::Autonomous);
    }

    #[test]
    fn sleep_task_with_location_roundtrip() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let location = NavNodeId(10);

        let task = Task {
            id: task_id,
            kind: TaskKind::Sleep {
                bed_pos: Some(VoxelCoord::new(5, 3, 8)),
                location: SleepLocation::Home(StructureId(7)),
            },
            state: TaskState::InProgress,
            location,

            progress: 0.0,
            total_cost: 10000.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Autonomous,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: Task = serde_json::from_str(&json).unwrap();

        match &restored.kind {
            TaskKind::Sleep { bed_pos, location } => {
                assert_eq!(*bed_pos, Some(VoxelCoord::new(5, 3, 8)));
                match location {
                    SleepLocation::Home(sid) => assert_eq!(*sid, StructureId(7)),
                    other => panic!("Expected Home, got {:?}", other),
                }
            }
            other => panic!("Expected Sleep task, got {:?}", other),
        }
    }

    #[test]
    fn sleep_location_backward_compat() {
        // Old save format: Sleep without `location` field defaults to Ground.
        let json = r#"{
            "Sleep": { "bed_pos": [5, 3, 8] }
        }"#;
        let kind: TaskKind = serde_json::from_str(json).unwrap();
        match kind {
            TaskKind::Sleep { bed_pos, location } => {
                assert_eq!(bed_pos, Some(VoxelCoord::new(5, 3, 8)));
                match location {
                    SleepLocation::Ground => {} // expected
                    other => panic!("Expected Ground default, got {:?}", other),
                }
            }
            other => panic!("Expected Sleep, got {:?}", other),
        }
    }

    #[test]
    fn haul_task_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let location = NavNodeId(5);
        let dest_node = NavNodeId(20);

        // Test GroundPile source, GoingToSource phase.
        let task = Task {
            id: task_id,
            kind: TaskKind::Haul {
                item_kind: crate::inventory::ItemKind::Bread,
                quantity: 3,
                source: HaulSource::GroundPile(VoxelCoord::new(10, 1, 10)),
                destination: StructureId(7),
                phase: HaulPhase::GoingToSource,
                destination_nav_node: dest_node,
            },
            state: TaskState::Available,
            location,

            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Automated,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, task_id);
        match &restored.kind {
            TaskKind::Haul {
                item_kind,
                quantity,
                source,
                destination,
                phase,
                destination_nav_node,
            } => {
                assert_eq!(*item_kind, crate::inventory::ItemKind::Bread);
                assert_eq!(*quantity, 3);
                assert_eq!(*source, HaulSource::GroundPile(VoxelCoord::new(10, 1, 10)));
                assert_eq!(*destination, StructureId(7));
                assert_eq!(*phase, HaulPhase::GoingToSource);
                assert_eq!(*destination_nav_node, dest_node);
            }
            _ => panic!("Expected Haul task kind"),
        }
        assert_eq!(restored.origin, TaskOrigin::Automated);

        // Test Building source, GoingToDestination phase.
        let task2 = Task {
            id: task_id,
            kind: TaskKind::Haul {
                item_kind: crate::inventory::ItemKind::Fruit,
                quantity: 5,
                source: HaulSource::Building(StructureId(3)),
                destination: StructureId(9),
                phase: HaulPhase::GoingToDestination,
                destination_nav_node: dest_node,
            },
            state: TaskState::InProgress,
            location: dest_node,

            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::Automated,
        };

        let json2 = serde_json::to_string(&task2).unwrap();
        let restored2: Task = serde_json::from_str(&json2).unwrap();
        match &restored2.kind {
            TaskKind::Haul { source, phase, .. } => {
                assert_eq!(*source, HaulSource::Building(StructureId(3)));
                assert_eq!(*phase, HaulPhase::GoingToDestination);
            }
            _ => panic!("Expected Haul task kind"),
        }
    }

    #[test]
    fn acquire_item_task_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let location = NavNodeId(8);

        let task = Task {
            id: task_id,
            kind: TaskKind::AcquireItem {
                source: HaulSource::GroundPile(VoxelCoord::new(5, 1, 10)),
                item_kind: crate::inventory::ItemKind::Bread,
                quantity: 3,
            },
            state: TaskState::InProgress,
            location,

            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Autonomous,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, task_id);
        match &restored.kind {
            TaskKind::AcquireItem {
                source,
                item_kind,
                quantity,
            } => {
                assert_eq!(*source, HaulSource::GroundPile(VoxelCoord::new(5, 1, 10)));
                assert_eq!(*item_kind, crate::inventory::ItemKind::Bread);
                assert_eq!(*quantity, 3);
            }
            _ => panic!("Expected AcquireItem task kind"),
        }
        assert_eq!(restored.origin, TaskOrigin::Autonomous);
    }

    #[test]
    fn mope_task_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let location = NavNodeId(12);

        let task = Task {
            id: task_id,
            kind: TaskKind::Mope,
            state: TaskState::InProgress,
            location,

            progress: 50.0,
            total_cost: 10000.0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Autonomous,
        };

        let json = serde_json::to_string(&task).unwrap();
        let restored: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, task_id);
        assert!(matches!(restored.kind, TaskKind::Mope));
        assert_eq!(restored.state, TaskState::InProgress);
        assert_eq!(restored.progress, 50.0);
        assert_eq!(restored.total_cost, 10000.0);
        assert_eq!(restored.required_species, Some(Species::Elf));
        assert_eq!(restored.origin, TaskOrigin::Autonomous);
    }

    #[test]
    fn task_creation_and_lookup() {
        use std::collections::BTreeMap;

        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let location = NavNodeId(5);

        let task = Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location,

            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
        };

        let mut registry: BTreeMap<TaskId, Task> = BTreeMap::new();
        registry.insert(task_id, task);

        let retrieved = &registry[&task_id];
        assert_eq!(retrieved.id, task_id);
        assert_eq!(retrieved.state, TaskState::Available);
        assert_eq!(retrieved.location, location);

        assert_eq!(retrieved.progress, 0.0);
        assert_eq!(retrieved.total_cost, 0.0);
    }
}
