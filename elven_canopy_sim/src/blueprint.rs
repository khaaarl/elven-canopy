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
//    and creates a `Blueprint` in `Designated` state.
// 3. (Future) Construction tasks are created for creatures to work on.
// 4. (Future) On completion, the blueprint transitions to `Complete` and
//    voxels are placed in the world.
//
// The `Complete` state exists for forward compatibility but is not yet used.
// Cancellation removes the blueprint entirely (see `CancelBuild` in `sim.rs`).
//
// See also: `sim.rs` for the `blueprints` map and command handlers,
// `command.rs` for `DesignateBuild` / `CancelBuild`, `event.rs` for
// `BlueprintDesignated`, `types.rs` for `ProjectId`, `BuildType`, `Priority`.
//
// **Critical constraint: determinism.** Blueprints are created with
// `ProjectId`s generated from the sim's PRNG. Blueprint storage uses
// `BTreeMap` for deterministic iteration order.

use crate::types::{BuildType, Priority, ProjectId, VoxelCoord};
use serde::{Deserialize, Serialize};

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
    fn blueprint_serialization_roundtrip() {
        let mut rng = GameRng::new(99);
        let id = ProjectId::new(&mut rng);
        let bp = Blueprint {
            id,
            build_type: BuildType::Platform,
            voxels: vec![VoxelCoord::new(10, 1, 10)],
            priority: Priority::High,
            state: BlueprintState::Designated,
        };

        let json = serde_json::to_string(&bp).unwrap();
        let restored: Blueprint = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, bp.id);
        assert_eq!(restored.state, BlueprintState::Designated);
        assert_eq!(restored.voxels.len(), 1);
        assert_eq!(restored.priority, Priority::High);
    }
}
