// Task entities — units of work that creatures can be assigned to.
//
// Tasks are the core of the assignment system. The sim maintains a task
// registry (`BTreeMap<TaskId, Task>` on `SimState`), and each creature's
// activation loop checks for available tasks before defaulting to wandering.
//
// ## Data model
//
// A `Task` has a `kind` (`TaskKind` enum), a `state` (`TaskState` lifecycle),
// a `location` (nav node where work happens), and an `assignees` list. Tasks
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
// - `EatFruit { fruit_pos }` — walk to a fruit voxel and eat it, restoring
//   food. Created automatically by the heartbeat hunger check when a
//   creature's food drops below `food_hunger_threshold_pct`. On arrival,
//   `do_eat_fruit()` restores `food_restore_pct`% of `food_max`, removes
//   the fruit from the world and tree, and completes the task.
//
// ## Lifecycle
//
// `TaskState` tracks where a task is in its lifecycle:
// - `Available` — no creature is working on it yet. Idle creatures check for
//   these during their activation loop.
// - `InProgress` — at least one creature has claimed it and is walking toward
//   it or doing work. `find_available_task()` in `sim.rs` skips these, so
//   only one creature transitions a task out of `Available`.
// - `Complete` — finished. All assignees have their `current_task` cleared
//   and return to wandering.
//
// See also: `sim.rs` for the activation loop that executes task behavior and
// handles assignment/completion, `types.rs` for `TaskId`, `command.rs` for
// the `CreateTask` command that adds tasks, `sim_bridge.rs` (in the gdext
// crate) for the GDScript-facing `create_goto_task()` wrapper.
//
// **Critical constraint: determinism.** Tasks are stored in `BTreeMap` and
// iterated in deterministic order. Task IDs come from the sim PRNG.

use crate::types::{CreatureId, NavNodeId, ProjectId, Species, TaskId, VoxelCoord};
use serde::{Deserialize, Serialize};

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
    /// Walk to a fruit voxel and eat it, restoring food. Created automatically
    /// by the heartbeat hunger check when a creature's food drops below
    /// `food_hunger_threshold_pct`. The `fruit_pos` is the voxel coordinate
    /// of the fruit to consume (removed from world on arrival).
    EatFruit { fruit_pos: VoxelCoord },
}

/// Lifecycle state of a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub assignees: Vec<CreatureId>,
    /// Current progress toward completion (0.0 to 1.0).
    pub progress: f32,
    /// Total work units needed to complete. 0.0 for instant tasks (e.g. GoTo).
    pub total_cost: f32,
    /// If set, only creatures of this species can claim this task.
    pub required_species: Option<Species>,
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
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: 5000.0,
            required_species: Some(Species::Elf),
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
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: 0.0,
            required_species: Some(Species::Elf),
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
            assignees: Vec::new(),
            progress: 0.0,
            total_cost: 0.0,
            required_species: None,
        };

        let mut registry: BTreeMap<TaskId, Task> = BTreeMap::new();
        registry.insert(task_id, task);

        let retrieved = &registry[&task_id];
        assert_eq!(retrieved.id, task_id);
        assert_eq!(retrieved.state, TaskState::Available);
        assert_eq!(retrieved.location, location);
        assert!(retrieved.assignees.is_empty());
        assert_eq!(retrieved.progress, 0.0);
        assert_eq!(retrieved.total_cost, 0.0);
    }
}
