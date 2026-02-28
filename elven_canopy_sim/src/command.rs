// Commands that mutate simulation state.
//
// All external mutations to the simulation go through `SimCommand`. This is
// the only way outside code can change sim state — the sim is a pure function
// `(state, commands) -> (new_state, events)`, and commands are the input.
//
// The full flow for a player action:
//   GDScript UI → `sim_bridge.rs` (gdext) → constructs a `SimCommand` →
//   `SimState::step()` in `sim.rs` processes it.
//
// In multiplayer (future), commands are broadcast to all peers, canonically
// ordered by tick, then applied — guaranteeing identical state.
//
// A `SimCommand` carries a `player_id`, a `tick` (when to apply), and a
// `SimAction` enum. Current actions:
// - `DesignateBuild` — validate and create a platform blueprint (see `blueprint.rs`).
// - `DesignateBuilding` — validate and create a building blueprint with per-face
//   layout (see `building.rs`).
// - `CancelBuild` — remove a blueprint by ProjectId.
// - `SetTaskPriority` — build system (placeholder, not yet wired).
// - `SetSimSpeed` — pause / play / fast-forward.
// - `SpawnCreature` — place a creature of any species at a voxel position.
// - `CreateTask` — create a task at a voxel position (see `task.rs` for
//   `TaskKind`). The handler in `sim.rs` snaps the position to the nearest
//   nav node.
//
// See also: `sim.rs` for `process_command()` which dispatches these,
// `task.rs` for `TaskKind`, `types.rs` for the ID and enum types used here,
// `sim_bridge.rs` (in the gdext crate) for the GDScript-facing wrappers.
//
// **Critical constraint: determinism.** Commands are the sole external input
// to the sim. Internal state changes come from scheduled events (see
// `event.rs`).

use crate::task::TaskKind;
use crate::types::*;
use serde::{Deserialize, Serialize};

/// A player-issued command targeting a specific simulation tick.
///
/// In single-player, `tick` is the current sim tick when the player acts.
/// In multiplayer, `tick` is the agreed-upon canonical application tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SimCommand {
    pub player_id: PlayerId,
    pub tick: u64,
    pub action: SimAction,
}

/// The specific action a command performs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SimAction {
    /// Designate a new build project.
    DesignateBuild {
        build_type: BuildType,
        voxels: Vec<VoxelCoord>,
        priority: Priority,
    },
    /// Cancel an in-progress or queued build project.
    CancelBuild { project_id: ProjectId },
    /// Change the priority of an existing build project.
    SetTaskPriority {
        project_id: ProjectId,
        priority: Priority,
    },
    /// Change the simulation speed.
    SetSimSpeed { speed: SimSpeed },
    /// Spawn a creature of the given species at the given position (snapped to
    /// nearest nav node, or nearest ground node for ground-only species).
    SpawnCreature {
        species: Species,
        position: VoxelCoord,
    },
    /// Create a task at the given position (snapped to nearest nav node).
    /// If `required_species` is set, only that species can claim the task.
    CreateTask {
        kind: TaskKind,
        position: VoxelCoord,
        required_species: Option<Species>,
    },
    /// Designate a building with paper-thin walls. `anchor` is the minimum
    /// corner of the footprint at foundation level. Interior voxels are placed
    /// above it. Width/depth must be >= 3.
    DesignateBuilding {
        anchor: VoxelCoord,
        width: i32,
        depth: i32,
        height: i32,
        priority: Priority,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::GameRng;

    #[test]
    fn command_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let cmd = SimCommand {
            player_id: PlayerId::new(&mut rng),
            tick: 100,
            action: SimAction::DesignateBuild {
                build_type: BuildType::Platform,
                voxels: vec![VoxelCoord::new(10, 20, 30), VoxelCoord::new(11, 20, 30)],
                priority: Priority::Normal,
            },
        };

        let json = serde_json::to_string(&cmd).unwrap();
        let restored: SimCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(cmd.player_id, restored.player_id);
        assert_eq!(cmd.tick, restored.tick);
        // SimAction doesn't derive PartialEq (unnecessary overhead for an
        // enum with Vec fields), so we verify via re-serialization.
        assert_eq!(json, serde_json::to_string(&restored).unwrap());
    }
}
