// Task entities — units of work that creatures can be assigned to.
//
// Tasks are the core of the assignment system: the sim maintains a registry
// of tasks (`BTreeMap<TaskId, Task>` on `SimState`), and each creature's
// activation loop checks for available tasks before defaulting to wandering.
//
// Each `TaskKind` has a behavior script evaluated per activation: walk toward
// the task location if not there yet, otherwise do work or complete instantly.
//
// See also: `sim.rs` for the activation loop that executes task behavior,
// `types.rs` for `TaskId`, `command.rs` for commands that create tasks.
//
// **Critical constraint: determinism.** Tasks are stored in `BTreeMap` and
// iterated in deterministic order. Task IDs come from the sim PRNG.

use crate::types::{CreatureId, NavNodeId, Species, TaskId};
use serde::{Deserialize, Serialize};

/// The type of work a task represents. Each variant carries kind-specific data
/// and defines a behavior script (see `sim.rs` activation loop).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TaskKind {
    /// Walk to a location. Completes instantly on arrival (total_cost = 0).
    GoTo,
    // Future: Build { build_type, voxels }, Harvest { source }, etc.
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
