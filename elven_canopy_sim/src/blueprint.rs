// Blueprint data model for the construction system.
//
// A blueprint is the sim's record of a player's intent to build something.
// When a player designates a build (e.g., a platform), a `Blueprint` is created
// in `Designated` state and stored in `SimState.blueprints`. The blueprint
// tracks which voxels are involved, the build type, and priority.
//
// ## Lifecycle
//
// 1. Player issues a `DesignateBuild` command (see `command.rs`).
// 2. `sim.rs` validates the designation (in-bounds, Air, adjacent to solid)
//    and creates a `Blueprint` in `Designated` state, plus a `Build` task
//    (linked via `task_id`).
// 3. An idle elf claims the Build task, pathfinds to the site, and does work.
//    Each activation increments progress; every `build_work_ticks_per_voxel`
//    units, one blueprint voxel materializes as solid.
// 4. When all voxels are placed, the blueprint transitions to `Complete`.
//
// Cancellation removes the blueprint, reverts any materialized voxels to Air,
// unassigns workers, and removes the Build task (see `cancel_build` in `sim.rs`).
//
// See also: `sim.rs` for the `blueprints` map and command handlers,
// `command.rs` for `DesignateBuild` / `CancelBuild`, `event.rs` for
// `BlueprintDesignated`, `types.rs` for `ProjectId`, `BuildType`, `Priority`.
//
// **Critical constraint: determinism.** Blueprints are created with
// `ProjectId`s generated from the sim's PRNG. Blueprint storage uses
// `BTreeMap` for deterministic iteration order.

use crate::types::{BuildType, FaceData, Priority, ProjectId, TaskId, VoxelCoord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The lifecycle state of a blueprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlueprintState {
    /// Player has designated the build; not yet constructed.
    Designated,
    /// Construction is complete; voxels have been placed.
    Complete,
}

/// A recorded build intent — the sim-side representation of a player's
/// designation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Blueprint {
    pub id: ProjectId,
    pub build_type: BuildType,
    pub voxels: Vec<VoxelCoord>,
    pub priority: Priority,
    pub state: BlueprintState,
    /// The Build task linked to this blueprint, if one has been created.
    #[serde(default)]
    pub task_id: Option<TaskId>,
    /// Per-face layout for Building blueprints. `None` for non-building types.
    /// Stored as a Vec of (coord, face_data) pairs since VoxelCoord can't be
    /// a JSON map key. Use `face_layout_map()` for O(1) lookup.
    #[serde(default)]
    pub face_layout: Option<Vec<(VoxelCoord, FaceData)>>,
    /// Set by structural validation when the blueprint is under significant
    /// stress (above warn threshold but below block threshold).
    #[serde(default)]
    pub stress_warning: bool,
}

impl Blueprint {
    /// Get the face layout as a BTreeMap for O(1) lookup. Returns None if
    /// this is not a Building blueprint.
    pub fn face_layout_map(&self) -> Option<BTreeMap<VoxelCoord, FaceData>> {
        self.face_layout
            .as_ref()
            .map(|list| list.iter().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::GameRng;
    use std::collections::BTreeMap;

    #[test]
    fn blueprint_creation_and_lookup() {
        let mut rng = GameRng::new(42);
        let id = ProjectId::new(&mut rng);
        let bp = Blueprint {
            id,
            build_type: BuildType::Platform,
            voxels: vec![VoxelCoord::new(5, 1, 5), VoxelCoord::new(6, 1, 5)],
            priority: Priority::Normal,
            state: BlueprintState::Designated,
            task_id: None,
            face_layout: None,
            stress_warning: false,
        };

        assert_eq!(bp.id, id);
        assert_eq!(bp.state, BlueprintState::Designated);
        assert_eq!(bp.voxels.len(), 2);

        let mut map = BTreeMap::new();
        map.insert(bp.id, bp);
        assert!(map.contains_key(&id));
        let retrieved = &map[&id];
        assert_eq!(retrieved.voxels.len(), 2);
        assert_eq!(retrieved.priority, Priority::Normal);
    }

    #[test]
    fn blueprint_with_task_id_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let id = ProjectId::new(&mut rng);
        let task_id = crate::types::TaskId::new(&mut rng);
        let bp = Blueprint {
            id,
            build_type: BuildType::Platform,
            voxels: vec![VoxelCoord::new(5, 1, 5)],
            priority: Priority::Normal,
            state: BlueprintState::Designated,
            task_id: Some(task_id),
            face_layout: None,
            stress_warning: false,
        };

        let json = serde_json::to_string(&bp).unwrap();
        let restored: Blueprint = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.task_id, Some(task_id));
    }

    #[test]
    fn blueprint_serialization_roundtrip() {
        let mut rng = GameRng::new(99);
        let id = ProjectId::new(&mut rng);
        let bp = Blueprint {
            id,
            build_type: BuildType::Platform,
            voxels: vec![VoxelCoord::new(10, 1, 10)],
            priority: Priority::High,
            state: BlueprintState::Designated,
            task_id: None,
            face_layout: None,
            stress_warning: false,
        };

        let json = serde_json::to_string(&bp).unwrap();
        let restored: Blueprint = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, bp.id);
        assert_eq!(restored.state, BlueprintState::Designated);
        assert_eq!(restored.voxels.len(), 1);
        assert_eq!(restored.priority, Priority::High);
    }
}
