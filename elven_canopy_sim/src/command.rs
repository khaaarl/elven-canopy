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
// - `DesignateCarve` — validate and create a carve blueprint that removes solid
//   voxels to Air (the inverse of construction).
// - `CancelBuild` — remove a blueprint by ProjectId.
// - `SetTaskPriority` — build system (placeholder, not yet wired).
// - `SpawnCreature` — place a creature of any species at a voxel position.
// - `CreateTask` — create a task at a voxel position (see `task.rs` for
//   `TaskKind`). The handler in `sim.rs` snaps the position to the nearest
//   nav node.
// - `RenameStructure` — set or clear a completed structure's user-editable name.
// - `DesignateLadder` — place a wood or rope ladder at an anchor position.
// - `FurnishStructure` — begin furnishing a completed building (e.g. Dormitory).
// - `AssignHome` — assign a creature to a home structure, or unassign.
// - `SetLogisticsPriority` — enable/disable logistics on a building and set
//   its pull priority (higher = served first).
// - `SetLogisticsWants` — set which items and quantities a building wants
//   hauled to it. The `LogisticsHeartbeat` creates `Haul` tasks to fill these.
// - `SetCookingConfig` — enable/disable cooking on a kitchen and set the
//   bread production target.
// - `SetCreatureFood` — directly set a creature's food value (initial spawning).
// - `SetCreatureRest` — directly set a creature's rest value (initial spawning).
// - `AddCreatureItem` — add items to a creature's inventory.
// - `AddGroundPileItem` — add items to a ground pile (creating it if needed).
// - `DebugNotification` — create a debug notification for testing.
// - `SetWorkshopConfig` — enable/disable a workshop and set which recipe IDs
//   it should produce. Recomputes logistics wants from recipe inputs.
// - `DiscoverCiv` — a civ becomes aware of another civ, creating a
//   CivRelationship row. No-op if already aware.
// - `SetCivOpinion` — update a civ's opinion of another civ. No-op if
//   unaware (no CivRelationship row exists).
//
// See also: `sim.rs` for `process_command()` which dispatches these,
// `task.rs` for `TaskKind`, `types.rs` for the ID and enum types used here,
// `sim_bridge.rs` (in the gdext crate) for the GDScript-facing wrappers.
//
// **Critical constraint: determinism.** Commands are the sole external input
// to the sim. Internal state changes come from scheduled events (see
// `event.rs`).

use crate::building::LogisticsWant;
use crate::inventory::ItemKind;
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
    /// Designate a ladder (wood or rope). `anchor` is the bottom voxel.
    /// The ladder extends upward for `height` voxels. `orientation` is the
    /// face the ladder panel is on (must be horizontal: PosX/NegX/PosZ/NegZ).
    DesignateLadder {
        anchor: VoxelCoord,
        height: i32,
        orientation: FaceDirection,
        kind: LadderKind,
        priority: Priority,
    },
    /// Designate a rectangular region of solid voxels for carving (removal to Air).
    /// Air and ForestFloor voxels in the selection are silently skipped.
    DesignateCarve {
        voxels: Vec<VoxelCoord>,
        priority: Priority,
    },
    /// Rename a completed structure. `None` resets to the auto-generated default.
    RenameStructure {
        structure_id: StructureId,
        name: Option<String>,
    },
    /// Begin furnishing a completed building with the given type (e.g. Dormitory).
    FurnishStructure {
        structure_id: StructureId,
        furnishing_type: FurnishingType,
    },
    /// Assign a creature to a home structure, or unassign (`structure_id: None`).
    /// Only valid for Elf creatures and Home-furnished buildings.
    AssignHome {
        creature_id: CreatureId,
        structure_id: Option<StructureId>,
    },
    /// Set the logistics priority for a building. `None` disables logistics.
    SetLogisticsPriority {
        structure_id: StructureId,
        priority: Option<u8>,
    },
    /// Set the logistics wants (item kind + target quantity) for a building.
    SetLogisticsWants {
        structure_id: StructureId,
        wants: Vec<LogisticsWant>,
    },
    /// Set the cooking configuration for a kitchen building.
    SetCookingConfig {
        structure_id: StructureId,
        cooking_enabled: bool,
        cooking_bread_target: u32,
    },
    /// Directly set a creature's food value (for initial spawning overrides).
    SetCreatureFood { creature_id: CreatureId, food: i64 },
    /// Directly set a creature's rest value (for initial spawning overrides).
    SetCreatureRest { creature_id: CreatureId, rest: i64 },
    /// Add items to a creature's inventory.
    AddCreatureItem {
        creature_id: CreatureId,
        item_kind: ItemKind,
        quantity: u32,
    },
    /// Add items to a ground pile (creating it if it doesn't exist).
    AddGroundPileItem {
        position: VoxelCoord,
        item_kind: ItemKind,
        quantity: u32,
    },
    /// Create a debug notification for testing the notification pipeline.
    DebugNotification { message: String },
    /// Set workshop configuration (enabled state and active recipe configs).
    /// Each recipe config carries a recipe ID and an output target (0 = don't craft).
    SetWorkshopConfig {
        structure_id: StructureId,
        workshop_enabled: bool,
        recipe_configs: Vec<WorkshopRecipeEntry>,
    },
    /// A civ becomes aware of another civ. Creates a CivRelationship row with
    /// the specified initial opinion. No-op if the relationship already exists.
    DiscoverCiv {
        civ_id: CivId,
        discovered_civ: CivId,
        initial_opinion: CivOpinion,
    },
    /// Update a civ's opinion of another civ. No-op if unaware (no
    /// CivRelationship row exists).
    SetCivOpinion {
        civ_id: CivId,
        target_civ: CivId,
        opinion: CivOpinion,
    },
}

/// A recipe configuration entry for workshop commands.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkshopRecipeEntry {
    pub recipe_id: String,
    pub target: u32,
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
