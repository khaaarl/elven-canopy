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
// - `SetCreatureFood` — directly set a creature's food value (initial spawning).
// - `SetCreatureRest` — directly set a creature's rest value (initial spawning).
// - `AddCreatureItem` — add items to a creature's inventory.
// - `AddGroundPileItem` — add items to a ground pile (creating it if needed).
// - `DebugNotification` — create a debug notification for testing.
// - `DiscoverCiv` — a civ becomes aware of another civ, creating a
//   CivRelationship row. No-op if already aware.
// - `SetCivOpinion` — update a civ's opinion of another civ. No-op if
//   unaware (no CivRelationship row exists).
// - `DebugKillCreature` — kill a creature immediately (debug/testing).
// - `DamageCreature` — reduce a creature's HP. Death at 0 HP.
// - `HealCreature` — restore a creature's HP (clamped to hp_max, no-op on dead).
// - `AttackCreature` — player-directed attack: creates an AttackTarget task with
//   PlayerCombat preemption, pursues target until dead.
// - `DirectedGoTo` — player-directed goto for a specific creature, preempting
//   lower-priority tasks.
// - `AttackMove` — player-directed attack-move: creature walks toward a
//   destination, engaging hostiles en route. Creates an AttackMove task with
//   PlayerCombat preemption.
// - `GroupGoTo` — like `DirectedGoTo` but for multiple creatures. Spreads
//   destinations across nearby nav nodes via BFS so creatures don't stack.
// - `GroupAttackMove` — like `AttackMove` but for multiple creatures with
//   spread destinations.
//
// See also: `sim.rs` for `process_command()` which dispatches these,
// `task.rs` for `TaskKind`, `types.rs` for the ID and enum types used here,
// `sim_bridge.rs` (in the gdext crate) for the GDScript-facing wrappers.
//
// **Critical constraint: determinism.** Commands are the sole external input
// to the sim. Internal state changes come from scheduled events (see
// `event.rs`).

use crate::building::LogisticsWant;
use crate::db::HostileResponse;
use crate::inventory::ItemKind;
use crate::recipe::RecipeKey;
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
    /// For `Greenhouse`, `greenhouse_species` must be set to a cultivable fruit.
    FurnishStructure {
        structure_id: StructureId,
        furnishing_type: FurnishingType,
        /// Required for Greenhouse; ignored for other types.
        greenhouse_species: Option<FruitSpeciesId>,
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
    /// Set the unified crafting toggle for a building.
    SetCraftingEnabled {
        structure_id: StructureId,
        enabled: bool,
    },
    /// Add a recipe to a building's active recipe list. Output targets are
    /// initialized to 0 — the user must set at least one non-zero target.
    /// Rejects duplicates (same recipe_key already active on the structure).
    AddActiveRecipe {
        structure_id: StructureId,
        recipe_key: RecipeKey,
    },
    /// Remove an active recipe from a building. Interrupts any in-progress
    /// craft task for this recipe.
    RemoveActiveRecipe { active_recipe_id: ActiveRecipeId },
    /// Set the target quantity for a specific recipe output.
    SetRecipeOutputTarget {
        active_recipe_target_id: ActiveRecipeTargetId,
        target_quantity: u32,
    },
    /// Configure auto-logistics for an active recipe.
    SetRecipeAutoLogistics {
        active_recipe_id: ActiveRecipeId,
        auto_logistics: bool,
        spare_iterations: u32,
    },
    /// Toggle an individual active recipe without removing it.
    SetRecipeEnabled {
        active_recipe_id: ActiveRecipeId,
        enabled: bool,
    },
    /// Move an active recipe up in priority (lower sort_order). No-op if
    /// already at the top within its structure.
    MoveActiveRecipeUp { active_recipe_id: ActiveRecipeId },
    /// Move an active recipe down in priority (higher sort_order). No-op if
    /// already at the bottom within its structure.
    MoveActiveRecipeDown { active_recipe_id: ActiveRecipeId },
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
    /// Kill a creature immediately (debug/testing). Triggers full death
    /// handling: task interruption, inventory drop, event emission, etc.
    DebugKillCreature { creature_id: CreatureId },
    /// Deal damage to a creature. Positive `amount` reduces HP. If HP reaches
    /// 0 the creature dies via the standard death handler.
    DamageCreature {
        creature_id: CreatureId,
        amount: i64,
    },
    /// Heal a creature. Positive `amount` restores HP up to `hp_max`.
    /// No effect on dead creatures.
    HealCreature {
        creature_id: CreatureId,
        amount: i64,
    },
    /// Order a melee attack: attacker strikes target (debug/testing).
    /// Calls `try_melee_strike` which validates range, cooldown, etc.
    DebugMeleeAttack {
        attacker_id: CreatureId,
        target_id: CreatureId,
    },
    /// Order a ranged attack: shooter fires an arrow at target (debug/testing).
    /// Validates: shooter alive, idle, has bow + arrow, aim feasibility, LOS.
    DebugShootAction {
        attacker_id: CreatureId,
        target_id: CreatureId,
    },
    /// Player-directed attack: the attacker creature pursues and attacks the
    /// target creature until the target is dead. Creates an AttackTarget task
    /// with PlayerCombat preemption level.
    AttackCreature {
        attacker_id: CreatureId,
        target_id: CreatureId,
    },
    /// Player-directed goto for a specific creature. Creates a GoTo task and
    /// immediately assigns it to the specified creature, preempting lower-
    /// priority tasks.
    DirectedGoTo {
        creature_id: CreatureId,
        position: VoxelCoord,
    },
    /// Player-directed attack-move: the creature walks toward the destination,
    /// engaging any hostiles detected en route. Creates an AttackMove task with
    /// PlayerCombat preemption level.
    AttackMove {
        creature_id: CreatureId,
        destination: VoxelCoord,
    },
    /// Group move: spread multiple creatures across nearby nav nodes around the
    /// destination instead of stacking them all on the same voxel. Each creature
    /// gets a unique nearby destination assigned via BFS outward from the target.
    GroupGoTo {
        creature_ids: Vec<CreatureId>,
        position: VoxelCoord,
    },
    /// Group attack-move: like `GroupGoTo` but each creature attack-moves to its
    /// assigned spread destination, engaging hostiles en route.
    GroupAttackMove {
        creature_ids: Vec<CreatureId>,
        destination: VoxelCoord,
    },
    /// Create a new military group for the player's civ.
    CreateMilitaryGroup { name: String },
    /// Delete a non-civilian military group. Members return to civilian status
    /// (their `military_group` field is nullified by the FK policy).
    DeleteMilitaryGroup { group_id: MilitaryGroupId },
    /// Reassign a creature to a different military group, or `None` for
    /// civilian. Rejects non-civ creatures and cross-civ assignments.
    ReassignMilitaryGroup {
        creature_id: CreatureId,
        group_id: Option<MilitaryGroupId>,
    },
    /// Rename a military group (including the civilian group).
    RenameMilitaryGroup {
        group_id: MilitaryGroupId,
        name: String,
    },
    /// Change a military group's hostile response setting.
    SetGroupHostileResponse {
        group_id: MilitaryGroupId,
        hostile_response: HostileResponse,
    },
    /// Spawn a projectile at a position with a given velocity (debug/testing).
    /// Creates a projectile with an arrow item in its inventory.
    DebugSpawnProjectile {
        /// Position to spawn from (voxel coordinate, will be converted to
        /// sub-voxel center).
        origin: VoxelCoord,
        /// Target voxel — the aim solver computes the launch velocity.
        target: VoxelCoord,
        /// Creature that "shot" this projectile (for attribution). Optional.
        shooter_id: Option<CreatureId>,
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
