// Commands that mutate simulation state.
//
// All simulation mutations go through `SimCommand`. In single-player, the
// Godot GDScript glue translates UI actions into commands and passes them
// to the Rust sim. In multiplayer, commands are broadcast to all peers,
// canonically ordered, then applied — guaranteeing deterministic state.
//
// See also: `types.rs` for the ID and enum types used here, `sim.rs` for
// the simulation state that processes these commands.
//
// **Critical constraint: determinism.** Commands are the sole input to the
// sim's pure function `(state, commands) -> (new_state, events)`.

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
    CancelBuild {
        project_id: ProjectId,
    },
    /// Change the priority of an existing build project.
    SetTaskPriority {
        project_id: ProjectId,
        priority: Priority,
    },
    /// Change the simulation speed.
    SetSimSpeed {
        speed: SimSpeed,
    },
    /// Spawn a new elf at the given position (snapped to nearest nav node).
    SpawnElf {
        position: VoxelCoord,
    },
    /// Spawn a new capybara at the given position (snapped to nearest ground nav node).
    SpawnCapybara {
        position: VoxelCoord,
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
                voxels: vec![
                    VoxelCoord::new(10, 20, 30),
                    VoxelCoord::new(11, 20, 30),
                ],
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
