// SimDb — tabulosity-based relational store for all simulation entities.
//
// Replaces the `BTreeMap<Id, Entity>` collections on `SimState` with a typed,
// FK-validated, indexed in-memory database. See `docs/drafts/sim_db_schema_v4.md`
// for the design rationale.
//
// ## Table layout
//
// The database has 40 tables organized in four tiers:
//
// **Player tables:** `players` — human operators identified by username string.
// One entry per connected human player; persisted in save files.
// `selection_groups` — SC2-style numbered selection groups (Ctrl+1–9) per
// player, storing creature and structure IDs for instant recall.
//
// **Entity tables:** `trees`, `creatures`, `tasks`, `blueprints`, `structures`,
// `projectiles` — the primary simulation entities, keyed by UUID-based or
// sequential IDs. `great_tree_infos` is a 1:1 child of `trees` storing
// mana economy and carrying capacity for the player's great tree.
// `Creature` includes `hp`/`hp_max` (hit points) and `vital_status`
// (`Alive`/`Incapacitated`/`Dead`, `#[indexed]` for efficient filtering).
// Dead creatures remain in the DB; all live-creature queries filter by
// vital_status. Incapacitated creatures are rendered and targetable but
// cannot act.
//
// **Child tables:** `thoughts`, `creature_traits`, `move_actions`,
// `notifications`, `inventories`, `item_stacks`, `ground_piles`,
// `logistics_wants`, `furniture`, `music_compositions` —
// normalized data that was previously stored as inline `Vec` fields on parent
// entities, plus player-visible notifications and construction music metadata.
//
// **Task decomposition tables:** `task_blueprint_refs`, `task_structure_refs`,
// `task_voxel_refs`, `task_haul_data`, `task_sleep_data`, `task_acquire_data`,
// `task_craft_data` — replace the `TaskKind` enum's variant-specific data with
// relational tables. The base `Task` row stores only `kind_tag` (discriminant).
// All variant-specific data lives exclusively in extension/relationship tables,
// queried via helper methods on `SimState` (`task_project_id`,
// `task_structure_ref`, `task_craft_data`, etc.).
//
// **Item extension tables:** `item_subcomponents`, `item_enchantments`,
// `enchantment_effects` — support quality, materials, subcomponent tracking
// (e.g., bowstring in a bow), and enchantment effects on item stacks.
//
// ## FK policies
//
// Task child tables use cascade-on-delete: removing a task automatically
// removes its refs, data, and extension rows. `Thought` also cascades on
// creature deletion. All other FKs use restrict-on-delete (the default).
//
// ## Determinism
//
// All tabulosity collections use `BTreeMap`/`BTreeSet` internally, matching
// the determinism requirement. No iterated `HashMap` — use `LookupMap` for point queries.
//
// See also: `sim/mod.rs` for `SimState` which owns the `SimDb` instance,
// `types.rs` for all ID types, `docs/drafts/sim_db_schema_v4.md` for the
// full schema design.
//
// **Critical constraint: determinism.** All iteration is in deterministic
// BTreeMap order. No hash-based collections.

use crate::fruit::{FruitSpecies, FruitSpeciesTable};
use crate::inventory::{EffectKind, EquipSlot, ItemColor, ItemKind, Material};
use crate::projectile::{SubVoxelCoord, SubVoxelVec};
use crate::task::{HaulPhase, TaskOrigin, TaskState};
use crate::types::{
    ActiveRecipeId, ActiveRecipeTargetId, ActivityId, ActivityKind, ActivityPhase, BuildType,
    CivId, CivOpinion, CivSpecies, CompositionId, CreatureId, CultureTag, DeparturePolicy,
    EnchantmentId, FruitSpeciesId, FurnishingType, FurnitureId, GroundPileId, InventoryId,
    ItemStackId, MilitaryGroupId, NotificationId, ParticipantRole, ParticipantStatus, ProjectId,
    ProjectileId, RecruitmentMode, SelectionGroupId, Species, StructureId, StrutId, TaskId,
    ThoughtKind, TraitKind, TraitValue, TreeId, VitalStatus, VoxelCoord,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tabulosity::{Database, Table};

use crate::blueprint::BlueprintState;
use crate::sim::CreaturePath;
use crate::types::{FaceData, Priority, VoxelType};

// ---------------------------------------------------------------------------
// Action system
// ---------------------------------------------------------------------------

/// What a creature is currently doing. Stored inline on the Creature row.
/// Per-action detail tables (e.g., `MoveAction`) hold additional state for
/// action kinds that need it.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[repr(u8)]
pub enum ActionKind {
    /// Idle — not performing any action.
    #[default]
    NoAction = 0,
    /// Moving one nav edge. Detail in `MoveAction` table.
    Move = 1,
    /// Building one voxel (or carving one voxel).
    Build = 2,
    /// Placing one furniture item.
    Furnish = 3,
    // 4 = reserved (was Cook)
    /// Crafting one recipe output.
    Craft = 5,
    /// One sleep cycle (~1s). Repeated until rest is full.
    Sleep = 6,
    /// Eating (bread or fruit).
    Eat = 7,
    /// Harvesting a fruit from a tree.
    Harvest = 8,
    /// Picking up an unowned item.
    AcquireItem = 9,
    /// Haul pickup phase — loading items at source.
    PickUp = 10,
    /// Haul dropoff phase — unloading items at destination.
    DropOff = 11,
    /// One mope cycle (~1s). Repeated until mope duration fulfilled.
    Mope = 12,
    /// A melee strike against an adjacent creature.
    MeleeStrike = 13,
    /// Shooting a ranged projectile at a target creature.
    Shoot = 14,
    /// Picking up items for military group equipment (no ownership transfer).
    AcquireMilitaryEquipment = 15,
}

// ---------------------------------------------------------------------------
// Task decomposition enums
// ---------------------------------------------------------------------------

/// Discriminant-only tag for `TaskKind`. The base `Task` row stores this
/// instead of the full enum — variant-specific data lives in relationship
/// and extension tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskKindTag {
    AcquireItem,
    AcquireMilitaryEquipment,
    AttackMove,
    AttackTarget,
    Build,
    Craft,
    EatBread,
    EatFruit,
    Furnish,
    GoTo,
    Harvest,
    Haul,
    Mope,
    Sleep,
}

impl TaskKindTag {
    /// Whether tasks of this kind require mana. Creatures with mp_max = 0
    /// cannot claim mana-requiring tasks.
    pub fn requires_mana(self) -> bool {
        matches!(self, Self::Build | Self::Furnish)
    }

    /// Human-readable display name for UI purposes.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AcquireItem => "AcquireItem",
            Self::AcquireMilitaryEquipment => "Equip",
            Self::AttackMove => "AttackMove",
            Self::AttackTarget => "Attack",
            Self::Build => "Build",
            Self::Craft => "Craft",
            Self::EatBread => "EatBread",
            Self::EatFruit => "EatFruit",
            Self::Furnish => "Furnish",
            Self::GoTo => "GoTo",
            Self::Harvest => "Harvest",
            Self::Haul => "Haul",
            Self::Mope => "Moping",
            Self::Sleep => "Sleep",
        }
    }

    /// Derive the tag from a full `TaskKind` enum value.
    pub fn from_kind(kind: &crate::task::TaskKind) -> Self {
        use crate::task::TaskKind;
        match kind {
            TaskKind::AcquireItem { .. } => Self::AcquireItem,
            TaskKind::AcquireMilitaryEquipment { .. } => Self::AcquireMilitaryEquipment,
            TaskKind::AttackMove => Self::AttackMove,
            TaskKind::AttackTarget { .. } => Self::AttackTarget,
            TaskKind::Build { .. } => Self::Build,
            TaskKind::Craft { .. } => Self::Craft,
            TaskKind::EatBread => Self::EatBread,
            TaskKind::EatFruit { .. } => Self::EatFruit,
            TaskKind::Furnish { .. } => Self::Furnish,
            TaskKind::GoTo => Self::GoTo,
            TaskKind::Harvest { .. } => Self::Harvest,
            TaskKind::Haul { .. } => Self::Haul,
            TaskKind::Mope => Self::Mope,
            TaskKind::Sleep { .. } => Self::Sleep,
        }
    }
}

/// Role of a task-to-structure reference. Determines why a task references
/// a particular structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskStructureRole {
    FurnishTarget,
    HaulDestination,
    HaulSourceBuilding,
    SleepAt,
    AcquireSourceBuilding,
    CraftAt,
}

/// Role of a task-to-voxel reference. Determines why a task references
/// a particular voxel position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskVoxelRole {
    FruitTarget,
    BedPosition,
    HaulSourcePile,
    AcquireSourcePile,
}

/// Discriminant for haul/acquire source type. Indicates whether the source
/// FK lives in `TaskStructureRef` or `TaskVoxelRef`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HaulSourceKind {
    Pile,
    Building,
}

/// Discriminant for sleep location type. Stored in `TaskSleepData` for
/// thought generation on completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SleepLocationType {
    Home,
    Dormitory,
    Ground,
}

/// Owner kind discriminant for inventory containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InventoryOwnerKind {
    Creature,
    Structure,
    GroundPile,
}

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

/// A tree entity — the primary world structure.
///
/// Stores both the player's great tree and lesser decorative trees. Great-tree-
/// specific data (mana, carrying capacity) lives in the `GreatTreeInfo` child
/// table, linked by a parent-PK FK (1:1). Fruit fields (`fruit_positions`,
/// `fruit_species_id`) are on this table because any tree may bear fruit.
///
/// See also: `GreatTreeInfo` (child table), `worldgen.rs` (tree construction),
/// `greenhouse.rs` (fruit spawning), `sim/mod.rs` (mana overflow).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Tree {
    #[primary_key]
    pub id: TreeId,
    pub position: VoxelCoord,
    pub health: i64,
    pub growth_level: u32,
    /// Civilization that owns this tree (`None` for wild/lesser trees).
    #[indexed]
    pub owner: Option<CivId>,
    pub trunk_voxels: Vec<VoxelCoord>,
    pub branch_voxels: Vec<VoxelCoord>,
    /// Leaf voxel positions (blobs at branch terminals).
    pub leaf_voxels: Vec<VoxelCoord>,
    /// Root voxel positions (at or below ground level).
    pub root_voxels: Vec<VoxelCoord>,
    /// Positions of fruit hanging below leaf voxels.
    pub fruit_positions: Vec<VoxelCoord>,
    /// The fruit species this tree produces. `None` if the tree doesn't bear
    /// fruit (or for pre-fruit-variety saves).
    #[serde(default)]
    #[indexed]
    pub fruit_species_id: Option<FruitSpeciesId>,
}

/// Great-tree-specific data — mana economy and carrying capacity.
///
/// 1:1 child of `Tree` via parent-PK FK: the primary key is the `TreeId` of
/// the parent `Tree` row. Only the player's home tree (and potentially future
/// sentient trees) get a `GreatTreeInfo` row.
///
/// See also: `Tree` (parent table), `sim/mod.rs` (mana overflow),
/// `sim_bridge.rs` (exposing mana/capacity to Godot).
#[derive(Table, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GreatTreeInfo {
    #[primary_key]
    pub id: TreeId,
    /// Tree-scale mana in millimana (1000 = 1.0 display mana).
    pub mana_stored: i64,
    /// Maximum tree mana in millimana.
    pub mana_capacity: i64,
    /// Fruit spawn chance per heartbeat, in parts per million (500_000 = 50%).
    pub fruit_production_rate_ppm: u32,
    pub carrying_capacity: i64,
    pub current_load: i64,
}

/// A creature entity — an autonomous agent (elf, capybara, etc.).
///
/// Items are stored in the `item_stacks` table via the `inventory_id` FK.
/// Personal wants are in the `logistics_want_rows` table (queried by
/// `inventory_id`). Thoughts are in the `thoughts` table.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Creature {
    #[primary_key]
    pub id: CreatureId,
    #[indexed]
    pub species: Species,
    pub position: VoxelCoord,
    pub name: String,
    pub name_meaning: String,
    #[indexed]
    pub current_task: Option<TaskId>,
    /// Group activity this creature is participating in. Only set when the
    /// creature is committed (Traveling or Arrived status) — NOT set for
    /// tentative volunteers. See `sim/activity.rs` for the lifecycle.
    #[serde(default)]
    #[indexed]
    pub current_activity: Option<ActivityId>,
    pub food: i64,
    pub rest: i64,
    #[indexed]
    pub assigned_home: Option<StructureId>,
    #[indexed]
    pub inventory_id: InventoryId,
    /// Civilization this creature belongs to (None = wild/unaffiliated).
    #[serde(default)]
    #[indexed]
    pub civ_id: Option<CivId>,
    /// Military group assignment. For civ creatures (`civ_id` is `Some`),
    /// `None` means civilian — governed by the civ's default civilian group
    /// settings (notably `engagement_style`). For non-civ creatures (`civ_id`
    /// is `None`), this is always `None` and behavior comes from the species'
    /// `engagement_style` instead. Group assignment is preserved on death.
    #[serde(default)]
    #[indexed]
    pub military_group: Option<MilitaryGroupId>,
    pub path: Option<CreaturePath>,
    /// What the creature is currently doing. `NoAction` when idle.
    #[serde(default)]
    pub action_kind: ActionKind,
    /// Tick when the current action completes. `None` when idle.
    #[serde(default)]
    pub next_available_tick: Option<u64>,
    /// Current hit points. Reaches 0 → creature dies.
    #[serde(default)]
    pub hp: i64,
    /// Maximum hit points (set from `SpeciesData::hp_max` at spawn).
    #[serde(default)]
    pub hp_max: i64,
    /// Whether this creature is alive, incapacitated, dead, or in a future
    /// supernatural state. Dead creatures are excluded from all active
    /// simulation queries. Incapacitated creatures are rendered and targetable
    /// but cannot act, move, or be assigned tasks.
    #[serde(default)]
    #[indexed]
    pub vital_status: VitalStatus,
    /// Current mana points. 0 for nonmagical creatures.
    #[serde(default)]
    pub mp: i64,
    /// Maximum mana points (set from `SpeciesData::mp_max` at spawn).
    /// 0 = nonmagical — cannot claim mana-requiring tasks.
    #[serde(default)]
    pub mp_max: i64,
    /// Consecutive wasted work actions due to insufficient mana. Reset to 0
    /// on any successful work action or task change (including mope interrupts).
    /// When this reaches `GameConfig::mana_abandon_threshold`, the creature
    /// abandons the current task.
    #[serde(default)]
    pub wasted_action_count: u32,
}

/// A timestamped thought belonging to a creature.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("creature_id", "seq")]
pub struct Thought {
    #[indexed]
    pub creature_id: CreatureId,
    #[auto_increment]
    #[serde(rename = "id")]
    pub seq: u64,
    pub kind: ThoughtKind,
    pub tick: u64,
}

/// A biological trait for a creature. Each row stores one trait kind and its
/// value. The compound PK `(creature_id, trait_kind)` ensures at most one
/// value per trait per creature.
///
/// Visual traits store palette indices as `TraitValue::Int`. `BioSeed` stores
/// a raw PRNG output for future trait derivation. Cascade-on-delete removes
/// all traits when the creature is deleted.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("creature_id", "trait_kind")]
pub struct CreatureTrait {
    #[indexed]
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    #[serde(default = "default_trait_value")]
    pub value: TraitValue,
}

fn default_trait_value() -> TraitValue {
    TraitValue::Int(0)
}

/// A player-visible notification. Persists across saves so the notification
/// history panel can show past events.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Notification {
    #[primary_key(auto_increment)]
    pub id: NotificationId,
    /// Sim tick when the notification was created.
    pub tick: u64,
    /// Human-readable message text.
    pub message: String,
}

/// Status of a music composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CompositionStatus {
    /// Composition has been seeded but not yet generated.
    Pending,
    /// Audio has been generated (by the rendering layer, not the sim).
    Ready,
}

/// A music composition associated with a construction project (or potentially
/// other future sources like concerts). Stores only the seed and generation
/// parameters — the actual Grid and PCM are produced by the rendering layer
/// (gdext) to keep the sim free of audio dependencies.
///
/// Created when a blueprint is designated. The rendering layer polls for
/// Pending compositions, generates audio on a background thread, and marks
/// them Ready.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct MusicComposition {
    #[primary_key(auto_increment)]
    pub id: CompositionId,
    /// PRNG seed for deterministic generation.
    pub seed: u64,
    /// Number of SA sections (2–5). More sections = longer piece.
    pub sections: u8,
    /// Church mode index (0–5: dorian, phrygian, lydian, mixolydian, aeolian, ionian).
    pub mode_index: u8,
    /// Vaelith vowel brightness (0.0–1.0).
    pub brightness: f32,
    /// SA iteration budget (higher = better quality, slower generation).
    pub sa_iterations: u32,
    /// Target playback duration in milliseconds (= build time at 1x speed).
    /// The rendering layer uses this to derive the exact BPM after generation.
    pub target_duration_ms: u32,
    /// Sim tick when the composition was requested.
    pub requested_tick: u64,
    /// Whether an elf has started building (first work tick on the blueprint).
    pub build_started: bool,
    /// Current status (Pending → Ready, set by rendering layer).
    pub status: CompositionStatus,
}

/// A task — a unit of work that a creature can be assigned to.
///
/// Variant-specific data lives in extension tables: `task_blueprint_refs`,
/// `task_structure_refs`, `task_voxel_refs`, `task_haul_data`,
/// `task_sleep_data`, `task_acquire_data`. Use the query helper methods
/// on `SimState` (`task_project_id`, `task_structure_ref`, etc.) to access
/// variant-specific data.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    #[primary_key]
    pub id: TaskId,
    #[indexed]
    pub kind_tag: TaskKindTag,
    #[indexed]
    pub state: TaskState,
    pub location: VoxelCoord,
    pub progress: i64,
    pub total_cost: i64,
    #[indexed]
    pub required_species: Option<Species>,
    pub origin: TaskOrigin,
    /// If set, this task tracks a moving creature (pursuit/combat).
    /// Updated each activation to the target's current nav node.
    /// FK to creatures with restrict-on-delete; the sim must clear this
    /// before removing the target creature.
    #[indexed]
    #[serde(default)]
    pub target_creature: Option<CreatureId>,
    /// If set, only this creature may claim this task (command queue).
    /// No FK — application code handles cleanup on creature death.
    #[indexed]
    #[serde(default)]
    pub restrict_to_creature_id: Option<CreatureId>,
    /// If set, this task stays unavailable until the prerequisite reaches
    /// Complete. Forms a linked list for sequential command queues.
    /// No FK — application code handles cascade cancellation.
    #[indexed]
    #[serde(default)]
    pub prerequisite_task_id: Option<TaskId>,
}

/// Task-to-blueprint reference (Build tasks only).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("task_id", "seq")]
pub struct TaskBlueprintRef {
    #[indexed]
    pub task_id: TaskId,
    #[auto_increment]
    #[serde(rename = "id")]
    pub seq: u64,
    #[indexed]
    pub project_id: ProjectId,
}

/// Task-to-structure reference with role discriminant.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("task_id", "seq")]
pub struct TaskStructureRef {
    #[indexed]
    pub task_id: TaskId,
    #[auto_increment]
    #[serde(rename = "id")]
    pub seq: u64,
    #[indexed]
    pub structure_id: StructureId,
    pub role: TaskStructureRole,
}

/// Task-to-voxel position reference with role discriminant.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("task_id", "seq")]
pub struct TaskVoxelRef {
    #[indexed]
    pub task_id: TaskId,
    #[auto_increment]
    #[serde(rename = "id")]
    pub seq: u64,
    pub coord: VoxelCoord,
    #[indexed]
    pub role: TaskVoxelRole,
}

/// Haul-specific mutable state (1:1 extension table, PK = task_id).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskHaulData {
    #[primary_key]
    pub task_id: TaskId,
    pub item_kind: ItemKind,
    pub quantity: u32,
    pub phase: HaulPhase,
    pub source_kind: HaulSourceKind,
    pub destination_coord: VoxelCoord,
    /// The material filter from the want that triggered this haul.
    #[serde(default)]
    pub material_filter: crate::inventory::MaterialFilter,
    /// The actual material of the items being hauled. Set when the haul is
    /// created based on what `inv_reserve_items` selected. `None` for
    /// unmaterialed items (e.g., Bread).
    #[serde(default)]
    pub hauled_material: Option<Material>,
}

/// Sleep-specific state (1:1 extension table, PK = task_id).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskSleepData {
    #[primary_key]
    pub task_id: TaskId,
    pub sleep_location: SleepLocationType,
}

/// AcquireItem-specific state (1:1 extension table, PK = task_id).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskAcquireData {
    #[primary_key]
    pub task_id: TaskId,
    pub item_kind: ItemKind,
    pub quantity: u32,
    pub source_kind: HaulSourceKind,
}

/// A build blueprint (designated or complete).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Blueprint {
    #[primary_key]
    pub id: ProjectId,
    pub build_type: BuildType,
    pub voxels: Vec<VoxelCoord>,
    pub priority: Priority,
    #[indexed]
    pub state: BlueprintState,
    #[indexed]
    pub task_id: Option<TaskId>,
    /// FK to the construction singing composition (if any).
    #[indexed]
    pub composition_id: Option<CompositionId>,
    pub face_layout: Option<Vec<(VoxelCoord, FaceData)>>,
    pub stress_warning: bool,
    pub original_voxels: Vec<(VoxelCoord, VoxelType)>,
}

/// Per-move detail table. Stores render interpolation data for a creature
/// that is currently traversing a nav edge. At most one row per creature
/// (PK = CreatureId). Cascade-deleted when the creature is removed.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct MoveAction {
    #[primary_key]
    #[indexed]
    pub creature_id: CreatureId,
    /// Visual start position for interpolation.
    pub move_from: VoxelCoord,
    /// Visual end position for interpolation.
    pub move_to: VoxelCoord,
    /// Tick when movement started (for render lerp). The end tick is
    /// `creature.next_available_tick`.
    pub move_start_tick: u64,
}

impl Creature {
    /// Compute an interpolated world position for rendering.
    ///
    /// If the creature is currently performing a Move action, lerps between
    /// `move_from` and `move_to` based on `render_tick`. Otherwise returns
    /// the creature's current position.
    pub fn interpolated_position(
        &self,
        render_tick: f64,
        move_action: Option<&MoveAction>,
    ) -> (f32, f32, f32) {
        if let (Some(ma), Some(end_tick)) = (move_action, self.next_available_tick)
            && self.action_kind == ActionKind::Move
        {
            let duration = end_tick as f64 - ma.move_start_tick as f64;
            if duration > 0.0 {
                let t =
                    ((render_tick - ma.move_start_tick as f64) / duration).clamp(0.0, 1.0) as f32;
                let x = ma.move_from.x as f32 + (ma.move_to.x as f32 - ma.move_from.x as f32) * t;
                let y = ma.move_from.y as f32 + (ma.move_to.y as f32 - ma.move_from.y as f32) * t;
                let z = ma.move_from.z as f32 + (ma.move_to.z as f32 - ma.move_from.z as f32) * t;
                return (x, y, z);
            }
        }
        (
            self.position.x as f32,
            self.position.y as f32,
            self.position.z as f32,
        )
    }
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

/// A completed structure registered in the sim.
///
/// Items are stored in the `item_stacks` table via the `inventory_id` FK.
/// Logistics wants are in the `logistics_want_rows` table (queried by
/// `inventory_id`). Furniture is in the `furniture` table.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct CompletedStructure {
    #[primary_key]
    pub id: StructureId,
    #[indexed]
    pub project_id: ProjectId,
    pub build_type: BuildType,
    pub anchor: VoxelCoord,
    pub width: i32,
    pub depth: i32,
    pub height: i32,
    pub completed_tick: u64,
    pub name: Option<String>,
    #[indexed]
    pub furnishing: Option<FurnishingType>,
    pub inventory_id: InventoryId,
    pub logistics_priority: Option<u8>,
    /// Unified crafting toggle for all building types.
    #[serde(default)]
    pub crafting_enabled: bool,
    /// For greenhouses: the fruit species being cultivated.
    #[serde(default)]
    pub greenhouse_species: Option<crate::fruit::FruitSpeciesId>,
    /// Whether this greenhouse is actively producing fruit.
    #[serde(default)]
    pub greenhouse_enabled: bool,
    /// Tick at which the greenhouse last produced a fruit. Used to pace
    /// production without a dedicated scheduled event.
    #[serde(default)]
    pub greenhouse_last_production_tick: u64,
}

impl CompletedStructure {
    /// Create a `CompletedStructure` from a completed blueprint.
    pub fn from_blueprint(
        id: StructureId,
        blueprint: &crate::blueprint::Blueprint,
        completed_tick: u64,
        inventory_id: InventoryId,
    ) -> Self {
        let (anchor, width, depth, height) = Self::compute_bounding_box(&blueprint.voxels);
        Self {
            id,
            project_id: blueprint.id,
            build_type: blueprint.build_type,
            anchor,
            width,
            depth,
            height,
            completed_tick,
            name: None,
            furnishing: None,
            inventory_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
        }
    }

    /// Return the display name for this structure.
    pub fn display_name(&self) -> String {
        if let Some(ref custom) = self.name {
            return custom.clone();
        }
        if let Some(furnishing) = &self.furnishing {
            return format!("{} #{}", furnishing.display_str(), self.id.0);
        }
        let type_str = match self.build_type {
            BuildType::Platform => "Platform",
            BuildType::Wall => "Wall",
            BuildType::Enclosure => "Enclosure",
            BuildType::Building => "Building",
            BuildType::WoodLadder => "Wood Ladder",
            BuildType::RopeLadder => "Rope Ladder",
            BuildType::Carve => "Carve",
            BuildType::Strut => "Strut",
        };
        format!("{} #{}", type_str, self.id.0)
    }

    /// Compute the ground-floor interior voxel positions.
    pub fn floor_interior_positions(&self) -> Vec<VoxelCoord> {
        let y = self.anchor.y;
        let mut positions = Vec::new();
        for z in self.anchor.z..self.anchor.z + self.depth {
            for x in self.anchor.x..self.anchor.x + self.width {
                positions.push(VoxelCoord::new(x, y, z));
            }
        }
        positions
    }

    /// Choose furniture positions for a given furnishing type.
    pub fn compute_furniture_positions(
        &self,
        furnishing_type: crate::types::FurnishingType,
        rng: &mut elven_canopy_prng::GameRng,
    ) -> Vec<VoxelCoord> {
        let floor = self.floor_interior_positions();
        if floor.is_empty() {
            return Vec::new();
        }

        let door_x = self.anchor.x + self.width / 2;
        let door_y = self.anchor.y;
        let door_z = self.anchor.z + self.depth - 1;
        let door_pos = VoxelCoord::new(door_x, door_y, door_z);

        let eligible: Vec<VoxelCoord> = floor
            .into_iter()
            .filter(|pos| {
                if *pos == door_pos {
                    return false;
                }
                let dx = (pos.x - door_pos.x).abs();
                let dz = (pos.z - door_pos.z).abs();
                dx + dz > 1
            })
            .collect();

        if eligible.is_empty() {
            return Vec::new();
        }

        if furnishing_type == crate::types::FurnishingType::Home {
            let idx = rng.next_u64() as usize % eligible.len();
            return vec![eligible[idx]];
        }

        let divisor = match furnishing_type {
            crate::types::FurnishingType::Dormitory | crate::types::FurnishingType::ConcertHall => {
                2
            }
            crate::types::FurnishingType::Kitchen | crate::types::FurnishingType::Storehouse => 3,
            crate::types::FurnishingType::DiningHall | crate::types::FurnishingType::Workshop => 4,
            crate::types::FurnishingType::Greenhouse => 2,
            crate::types::FurnishingType::Home => unreachable!(),
        };

        let total_floor = (self.width * self.depth) as usize;
        let target = (total_floor / divisor).max(1).min(eligible.len());

        let mut shuffled = eligible;
        for i in (1..shuffled.len()).rev() {
            let j = rng.next_u64() as usize % (i + 1);
            shuffled.swap(i, j);
        }
        shuffled.truncate(target);
        shuffled.sort();
        shuffled
    }

    /// Return true if `coord` is a roof voxel of this structure.
    ///
    /// A roof voxel is defined as the topmost Y layer of the bounding box for
    /// Building and Enclosure types. Other structure types (Platform, Wall,
    /// etc.) have no concept of a roof.
    ///
    /// Used by `selection_controller.gd` to decide whether a click on a
    /// structure voxel should shield creatures inside from selection.
    pub fn is_roof_voxel(&self, coord: VoxelCoord) -> bool {
        match self.build_type {
            BuildType::Building | BuildType::Enclosure => {}
            _ => return false,
        }
        let roof_y = self.anchor.y + self.height - 1;
        coord.y == roof_y
            && coord.x >= self.anchor.x
            && coord.x < self.anchor.x + self.width
            && coord.z >= self.anchor.z
            && coord.z < self.anchor.z + self.depth
    }

    /// Compute the axis-aligned bounding box of a set of voxel coordinates.
    fn compute_bounding_box(voxels: &[VoxelCoord]) -> (VoxelCoord, i32, i32, i32) {
        if voxels.is_empty() {
            return (VoxelCoord::new(0, 0, 0), 0, 0, 0);
        }
        let mut min_x = voxels[0].x;
        let mut max_x = voxels[0].x;
        let mut min_y = voxels[0].y;
        let mut max_y = voxels[0].y;
        let mut min_z = voxels[0].z;
        let mut max_z = voxels[0].z;
        for v in &voxels[1..] {
            min_x = min_x.min(v.x);
            max_x = max_x.max(v.x);
            min_y = min_y.min(v.y);
            max_y = max_y.max(v.y);
            min_z = min_z.min(v.z);
            max_z = max_z.max(v.z);
        }
        let anchor = VoxelCoord::new(min_x, min_y, min_z);
        let width = max_x - min_x + 1;
        let depth = max_z - min_z + 1;
        let height = max_y - min_y + 1;
        (anchor, width, depth, height)
    }
}

/// A diagonal support strut connecting two voxel endpoints.
///
/// Struts are 6-connected lines of `VoxelType::Strut` voxels rasterized via
/// `VoxelCoord::line_to()`. They carry rod springs along their axis for
/// structural benefit. The `blueprint_id` FK cascades on delete (cancelling the
/// blueprint removes the strut row). The `structure_id` FK nullifies on delete
/// (the strut persists even if the parent structure is removed).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Strut {
    #[primary_key(auto_increment)]
    pub id: StrutId,
    pub endpoint_a: VoxelCoord,
    pub endpoint_b: VoxelCoord,
    #[indexed]
    pub blueprint_id: Option<ProjectId>,
    #[indexed]
    pub structure_id: Option<StructureId>,
}

/// An abstract inventory container. Creatures, structures, and ground piles
/// each own one via an `inventory_id` FK.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Inventory {
    #[primary_key(auto_increment)]
    pub id: InventoryId,
    pub owner_kind: InventoryOwnerKind,
}

/// Filter predicate for the `equipped_inv_slot` compound index — only index
/// stacks that actually have an equipped slot set.
fn is_equipped(stack: &ItemStack) -> bool {
    stack.equipped_slot.is_some()
}

/// A stack of items within an inventory. Items in a stack share identical
/// properties (kind, material, quality, current_hp, max_hp, enchantment).
/// The `current_hp`/`max_hp` pair tracks item durability — 0/0 means
/// indestructible (no durability tracking). See `sim/inventory_mgmt.rs` `inv_normalize`
/// for the full stacking criteria.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[index(
    name = "equipped_inv_slot",
    fields("inventory_id", "equipped_slot"),
    filter = "is_equipped",
    unique
)]
pub struct ItemStack {
    #[primary_key(auto_increment)]
    pub id: ItemStackId,
    #[indexed]
    pub inventory_id: InventoryId,
    #[indexed]
    pub kind: ItemKind,
    pub quantity: u32,
    #[serde(default)]
    pub material: Option<Material>,
    #[serde(default)]
    pub quality: i32,
    /// Current hit points. Decremented by damage hooks (combat, wear, etc.).
    /// When both `current_hp` and `max_hp` are 0, the item has no durability
    /// tracking (indestructible). When `max_hp > 0` and `current_hp` reaches
    /// 0, the item breaks and is removed.
    #[serde(default)]
    pub current_hp: i32,
    /// Maximum hit points set at creation time from config. 0 means no
    /// durability tracking (legacy / indestructible items).
    #[serde(default)]
    pub max_hp: i32,
    #[serde(default)]
    #[indexed]
    pub enchantment_id: Option<EnchantmentId>,
    #[indexed]
    pub owner: Option<CreatureId>,
    #[indexed]
    pub reserved_by: Option<TaskId>,
    #[serde(default)]
    pub equipped_slot: Option<EquipSlot>,
    /// Explicit dye color applied to this item. When present, `item_color()`
    /// returns this color directly instead of deriving from the material.
    /// Set by the dye-application system (F-dye-application, future).
    #[serde(default)]
    pub dye_color: Option<ItemColor>,
}

/// A pile of items on the ground at a specific voxel position.
///
/// Items are stored in the `item_stacks` table via the `inventory_id` FK.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct GroundPile {
    #[primary_key(auto_increment)]
    pub id: GroundPileId,
    #[indexed(unique)]
    pub position: VoxelCoord,
    pub inventory_id: InventoryId,
}

/// A desired item kind, material filter, and target quantity for an inventory.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("inventory_id", "seq")]
pub struct LogisticsWantRow {
    #[indexed]
    pub inventory_id: InventoryId,
    #[auto_increment]
    #[serde(rename = "id")]
    pub seq: u64,
    pub item_kind: ItemKind,
    #[serde(default)]
    pub material_filter: crate::inventory::MaterialFilter,
    pub target_quantity: u32,
}

/// A subcomponent record for a crafted item stack. Records what went into
/// crafting each item in the stack (e.g., a Bow contains 1 Bowstring).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("item_stack_id", "seq")]
pub struct ItemSubcomponent {
    #[indexed]
    pub item_stack_id: ItemStackId,
    #[auto_increment]
    #[serde(rename = "id")]
    pub seq: u64,
    pub component_kind: ItemKind,
    pub material: Option<Material>,
    pub quality: i32,
    pub quantity_per_item: u32,
}

/// A shared enchantment instance. Multiple item stacks can reference the
/// same enchantment. Effects are stored in `EnchantmentEffect` rows.
/// Stubbed for future magic item system.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct ItemEnchantment {
    #[primary_key(auto_increment)]
    pub id: EnchantmentId,
}

/// An individual effect within an enchantment.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("enchantment_id", "seq")]
pub struct EnchantmentEffect {
    #[indexed]
    pub enchantment_id: EnchantmentId,
    #[auto_increment]
    #[serde(rename = "id")]
    pub seq: u64,
    pub effect_kind: EffectKind,
    pub magnitude: i32,
    pub threshold: Option<i32>,
}

/// Craft task extension data — stores the recipe key and active recipe FK for
/// a Craft task. The `active_recipe_id` FK enables `RemoveActiveRecipe` to
/// find and interrupt in-progress craft tasks for the removed recipe.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskCraftData {
    #[primary_key]
    pub task_id: TaskId,
    pub recipe: crate::recipe::Recipe,
    #[serde(default)]
    pub material: Option<crate::inventory::Material>,
    /// FK to the active recipe that spawned this craft task. Used by
    /// `RemoveActiveRecipe` to interrupt in-progress tasks.
    pub active_recipe_id: ActiveRecipeId,
}

// ---------------------------------------------------------------------------
// Group activity tables — multi-creature coordination
// ---------------------------------------------------------------------------

/// A group activity — a coordination layer above the task system for actions
/// that require multiple participants (dances, construction choirs, rituals).
/// Activities own tasks (GoTo for assembly) rather than replacing them.
/// See `sim/activity.rs` for the lifecycle logic.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Activity {
    #[primary_key]
    pub id: ActivityId,

    /// What kind of group activity this is.
    #[indexed]
    pub kind: ActivityKind,

    /// Lifecycle phase.
    #[indexed]
    pub phase: ActivityPhase,

    /// Where the group activity takes place (center point).
    pub location: VoxelCoord,

    /// Minimum participants needed before the activity can begin execution.
    /// `None` means the activity can start with any number of participants.
    pub min_count: Option<u16>,

    /// Desired number of participants. Recruitment continues up to this count.
    /// `None` means same as `min_count`.
    pub desired_count: Option<u16>,

    /// Current work progress (only advances during `Executing` phase).
    pub progress: i64,

    /// Total work needed to complete. 0 for instant-on-assembly activities.
    pub total_cost: i64,

    /// Origin — player-directed or automated.
    pub origin: TaskOrigin,

    /// How participants join this activity.
    pub recruitment: RecruitmentMode,

    /// What happens when a participant leaves during execution.
    pub departure_policy: DeparturePolicy,

    /// Whether new participants can join after execution has started.
    pub allows_late_join: bool,

    /// Civilization that owns this activity. Creatures must belong to this
    /// civ to participate (`None` = no civ restriction).
    #[serde(default)]
    pub civ_id: Option<CivId>,

    /// Species restriction. Only creatures of this species can participate
    /// (`None` = no species restriction).
    #[serde(default)]
    pub required_species: Option<Species>,

    /// Tick when the activity entered the Executing phase.
    pub execution_start_tick: Option<u64>,

    /// Tick when the activity entered the Paused phase (`PauseAndWait` only).
    /// Cleared when resuming to Executing.
    pub pause_started_tick: Option<u64>,
}

/// Links a creature to an activity with role and commitment status.
/// A creature can only be committed to one activity at a time (enforced by
/// the `current_activity` FK on Creature for Traveling/Arrived participants).
/// Volunteered participants do NOT have `current_activity` set.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("activity_id", "creature_id")]
pub struct ActivityParticipant {
    #[indexed]
    pub activity_id: ActivityId,
    #[indexed]
    pub creature_id: CreatureId,

    /// This participant's role within the activity.
    pub role: ParticipantRole,

    /// Commitment status: Volunteered → Traveling → Arrived.
    pub status: ParticipantStatus,

    /// The position this participant needs to reach for assembly.
    pub assigned_position: VoxelCoord,

    /// The GoTo task driving movement during assembly (Traveling status only).
    /// Cleared when the creature arrives or the activity transitions.
    pub travel_task: Option<TaskId>,
}

// ---------------------------------------------------------------------------
// Active recipe tables (unified crafting system)
// ---------------------------------------------------------------------------

/// An active recipe on a crafting building. Provides a unified table shared by
/// all building types (kitchens, workshops, etc.).
///
/// `sort_order` determines priority: lower values are higher priority. Globally
/// unique to guarantee deterministic iteration with no tiebreaker needed.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[index(name = "structure_sort", fields("structure_id", "sort_order"))]
pub struct ActiveRecipe {
    #[primary_key(auto_increment)]
    pub id: ActiveRecipeId,

    #[indexed]
    pub structure_id: StructureId,

    /// The recipe template for this active recipe.
    pub recipe: crate::recipe::Recipe,

    /// Material binding for the recipe.
    #[serde(default)]
    pub material: Option<crate::inventory::Material>,

    /// Can be toggled without removing the recipe.
    pub enabled: bool,

    /// Priority ordering (lower = higher priority). Globally unique.
    #[indexed(unique)]
    pub sort_order: u32,

    /// Whether to auto-generate logistics wants for inputs.
    pub auto_logistics: bool,

    /// Extra iterations of input materials to pre-stage beyond what's needed.
    pub spare_iterations: u32,
}

/// Per-output target quantity for an active recipe. One row per recipe output.
/// `target_quantity = 0` means "don't care about this output" — the recipe
/// won't run until at least one target is non-zero.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct ActiveRecipeTarget {
    #[primary_key(auto_increment)]
    pub id: ActiveRecipeTargetId,

    #[indexed]
    pub active_recipe_id: ActiveRecipeId,

    pub output_item_kind: ItemKind,
    pub output_material: Option<Material>,
    pub target_quantity: u32,
}

/// AttackTarget task extension data — stores the target creature and pathfinding
/// retry counter. The target is a plain `CreatureId`, not an FK — the task polls
/// the target's vital_status each activation and completes if the target is dead
/// or missing.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskAttackTargetData {
    #[primary_key]
    pub task_id: TaskId,
    /// Target creature to pursue and attack. Plain ID, not FK — checked each tick.
    pub target: CreatureId,
    /// Consecutive pathfinding failures. Reset to 0 on any successful step.
    #[serde(default)]
    pub path_failures: u32,
}

/// AttackMove task extension data — stores the destination voxel that the
/// creature walks toward when not engaged with a hostile. The transient combat
/// target is tracked on the base `Task.target_creature` field.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskAttackMoveData {
    #[primary_key]
    pub task_id: TaskId,
    /// The destination the creature is walking toward (original command target).
    pub destination: VoxelCoord,
}

/// A placed or planned furniture item within a structure.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Furniture {
    #[primary_key(auto_increment)]
    pub id: FurnitureId,
    #[indexed]
    pub structure_id: StructureId,
    pub coord: VoxelCoord,
    pub placed: bool,
}

// ---------------------------------------------------------------------------
// Player tables
// ---------------------------------------------------------------------------

/// A human player (operator). In single-player there is exactly one; in
/// multiplayer there is one per connected human. The username string is the
/// primary key — unique within a game, chosen by the player on first launch,
/// and persisted client-side in `user://config.json`.
///
/// `civ_id` records which civilization this player controls. Currently all
/// players share the same civ, but the schema supports per-player civs for
/// future multi-civ games.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    #[primary_key]
    pub name: String,
    pub civ_id: Option<CivId>,
}

/// A player's selection group (SC2-style Ctrl+1–9). Each row represents one
/// numbered group for one player, containing the creature and structure IDs
/// that were in the selection when the group was saved.
///
/// `player_name` + `group_number` form a logical composite key (enforced by
/// the sim command handler, which upserts by looking up existing rows).
/// `group_number` is 1–9 (the keyboard key the player pressed).
///
/// Groups persist across save/load via serde. GDScript keeps a parallel
/// local copy for instant recall; the sim copy is the persistence layer.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct SelectionGroup {
    #[primary_key(auto_increment)]
    pub id: SelectionGroupId,
    #[indexed]
    pub player_name: String,
    pub group_number: u8,
    pub creature_ids: Vec<CreatureId>,
    pub structure_ids: Vec<StructureId>,
}

// ---------------------------------------------------------------------------
// Civilization tables
// ---------------------------------------------------------------------------

/// A procedurally generated civilization. Created during worldgen.
///
/// Primary key is `CivId(u16)`, assigned sequentially by the worldgen
/// generator (not auto-increment). The player's elf civ is always `CivId(0)`.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Civilization {
    #[primary_key]
    pub id: CivId,
    pub name: String,
    pub primary_species: CivSpecies,
    /// Secondary species present in this civ (usually 0-1).
    /// Kept sorted by CivSpecies Ord for deterministic iteration.
    pub minority_species: Vec<CivSpecies>,
    pub culture_tag: CultureTag,
    /// Whether this civ is controlled by player(s) vs AI.
    /// Currently exactly one civ has this set to true.
    pub player_controlled: bool,
}

/// A military group within a civilization. Every civ has at least one group
/// with `is_default_civilian = true` (the implicit home for unassigned
/// creatures). Additional groups can be created by the player.
///
/// **Invariant:** Exactly one group per civ has `is_default_civilian = true`.
/// This is enforced at creation time (reject duplicates) and the civilian
/// group cannot be deleted or have its flag changed.
///
/// Combat behavior is configured via `engagement_style`, which controls
/// weapon preference, initiative, ammo exhaustion, and disengage threshold.
/// See `species.rs` for the `EngagementStyle` struct definition.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct MilitaryGroup {
    #[primary_key(auto_increment)]
    pub id: MilitaryGroupId,
    #[indexed]
    pub civ_id: CivId,
    pub name: String,
    /// If true, this is the civ's default civilian group. Creatures with
    /// `military_group = None` are governed by this group's settings.
    /// Write-once: set at creation, immutable thereafter.
    pub is_default_civilian: bool,
    /// Combat engagement style for this group's members. Overrides the
    /// species-level `engagement_style` for civ creatures in this group.
    #[serde(default)]
    pub engagement_style: crate::species::EngagementStyle,
    /// Equipment that group members should autonomously acquire. Items
    /// acquired for military purposes do not confer ownership — they stay
    /// unowned (or keep their existing owner) in the creature's inventory.
    /// The player can edit these via the military panel UI.
    #[serde(default)]
    pub equipment_wants: Vec<crate::building::LogisticsWant>,
}

/// Directed relationship: `from_civ`'s opinion of `to_civ`.
/// Absence of a row means unaware. Awareness is asymmetric — Civ A can know
/// about Civ B while B has never heard of A.
///
/// PK `(from_civ, to_civ)` enforces at most one row per directed pair.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
#[primary_key("from_civ", "to_civ")]
pub struct CivRelationship {
    #[indexed]
    pub from_civ: CivId,
    #[indexed]
    pub to_civ: CivId,
    pub opinion: CivOpinion,
}

// ---------------------------------------------------------------------------
// Projectile entity
// ---------------------------------------------------------------------------

/// A projectile in flight (arrow, javelin, etc.). Exists only while airborne.
///
/// The projectile owns its payload (typically a single arrow) via an inventory.
/// On impact, items are transferred to a ground pile or destroyed. Damage is
/// computed at impact time from the velocity and item properties — not stored
/// on the projectile.
///
/// `shooter` is an FK with nullify — the shooter may be cleaned up in the far
/// future, but the projectile continues in flight regardless. `inventory_id`
/// is FK restrict — manually cleaned up in `remove_projectile()`.
///
/// `origin_voxel` records the launch site; creatures at this voxel are immune
/// to this projectile (prevents friendly-fire on the shooter and allies).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Projectile {
    #[primary_key(auto_increment)]
    pub id: ProjectileId,
    /// Creature that fired this projectile. FK nullify — used for kill credit.
    #[serde(default)]
    #[indexed]
    pub shooter: Option<CreatureId>,
    /// Inventory containing the arrow (or other projectile item).
    #[indexed]
    pub inventory_id: InventoryId,
    /// High-precision position in sub-voxel units (2^30 per voxel).
    pub position: SubVoxelCoord,
    /// Velocity in sub-voxel units per tick.
    pub velocity: SubVoxelVec,
    /// Last air voxel before the current position. Used for ground pile
    /// placement on surface impact (the pile goes at prev_voxel, not inside
    /// the solid voxel the projectile entered).
    pub prev_voxel: VoxelCoord,
    /// Voxel from which the projectile was launched. Creatures in this voxel
    /// are immune to this projectile (prevents friendly-fire on launch).
    pub origin_voxel: VoxelCoord,
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

// Tabulosity's generated table types don't derive Clone, so we roundtrip
// through serde JSON. Only used in tests and save/load — not a hot path.
impl Clone for SimDb {
    fn clone(&self) -> Self {
        let json = serde_json::to_string(self).expect("SimDb serialization failed");
        serde_json::from_str(&json).expect("SimDb deserialization failed")
    }
}

impl std::fmt::Debug for SimDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimDb")
            .field("civilizations", &self.civilizations.len())
            .field("selection_groups", &self.selection_groups.len())
            .field("fruit_species", &self.fruit_species.len())
            .field("trees", &self.trees.len())
            .field("great_tree_infos", &self.great_tree_infos.len())
            .field("military_groups", &self.military_groups.len())
            .field("civ_relationships", &self.civ_relationships.len())
            .field("creatures", &self.creatures.len())
            .field("thoughts", &self.thoughts.len())
            .field("tasks", &self.tasks.len())
            .field("blueprints", &self.blueprints.len())
            .field("structures", &self.structures.len())
            .field("inventories", &self.inventories.len())
            .field("item_stacks", &self.item_stacks.len())
            .field("ground_piles", &self.ground_piles.len())
            .field("furniture", &self.furniture.len())
            .field("notifications", &self.notifications.len())
            .field("music_compositions", &self.music_compositions.len())
            .field("struts", &self.struts.len())
            .field("projectiles", &self.projectiles.len())
            .finish()
    }
}

#[derive(Database)]
pub struct SimDb {
    #[table(singular = "player")]
    pub players: PlayerTable,

    #[table(singular = "selection_group", auto)]
    pub selection_groups: SelectionGroupTable,

    #[table(singular = "civilization")]
    pub civilizations: CivilizationTable,

    #[table(singular = "fruit_species")]
    pub fruit_species: FruitSpeciesTable,

    #[table(singular = "tree",
            fks(owner? = "civilizations" on_delete nullify,
                fruit_species_id? = "fruit_species"))]
    pub trees: TreeTable,

    #[table(singular = "great_tree_info",
            fks(id = "trees" pk))]
    pub great_tree_infos: GreatTreeInfoTable,

    #[table(singular = "military_group",
            auto,
            fks(civ_id = "civilizations" on_delete cascade))]
    pub military_groups: MilitaryGroupTable,

    #[table(singular = "civ_relationship",
            fks(from_civ = "civilizations" on_delete cascade,
                to_civ = "civilizations" on_delete cascade))]
    pub civ_relationships: CivRelationshipTable,

    #[table(singular = "creature",
            fks(current_task? = "tasks",
                current_activity? = "activities" on_delete nullify,
                assigned_home? = "structures",
                civ_id? = "civilizations" on_delete nullify,
                military_group? = "military_groups" on_delete nullify))]
    pub creatures: CreatureTable,

    #[table(singular = "move_action",
            fks(creature_id = "creatures" on_delete cascade))]
    pub move_actions: MoveActionTable,

    #[table(singular = "thought",
            nonpk_auto,
            fks(creature_id = "creatures" on_delete cascade))]
    pub thoughts: ThoughtTable,

    #[table(singular = "creature_trait",
            fks(creature_id = "creatures" on_delete cascade))]
    pub creature_traits: CreatureTraitTable,

    #[table(singular = "task",
            fks(target_creature? = "creatures"))]
    pub tasks: TaskTable,

    #[table(singular = "task_blueprint_ref",
            nonpk_auto,
            fks(task_id = "tasks" on_delete cascade,
                project_id = "blueprints"))]
    pub task_blueprint_refs: TaskBlueprintRefTable,

    #[table(singular = "task_structure_ref",
            nonpk_auto,
            fks(task_id = "tasks" on_delete cascade,
                structure_id = "structures"))]
    pub task_structure_refs: TaskStructureRefTable,

    #[table(singular = "task_voxel_ref",
            nonpk_auto,
            fks(task_id = "tasks" on_delete cascade))]
    pub task_voxel_refs: TaskVoxelRefTable,

    #[table(singular = "task_haul_data",
            fks(task_id = "tasks" pk on_delete cascade))]
    pub task_haul_data: TaskHaulDataTable,

    #[table(singular = "task_sleep_data",
            fks(task_id = "tasks" pk on_delete cascade))]
    pub task_sleep_data: TaskSleepDataTable,

    #[table(singular = "task_acquire_data",
            fks(task_id = "tasks" pk on_delete cascade))]
    pub task_acquire_data: TaskAcquireDataTable,

    #[table(singular = "task_craft_data",
            fks(task_id = "tasks" pk on_delete cascade))]
    pub task_craft_data: TaskCraftDataTable,

    #[table(singular = "active_recipe", auto, fks(structure_id = "structures"))]
    pub active_recipes: ActiveRecipeTable,

    #[table(singular = "active_recipe_target",
            auto,
            fks(active_recipe_id = "active_recipes" on_delete cascade))]
    pub active_recipe_targets: ActiveRecipeTargetTable,

    #[table(singular = "task_attack_target_data",
            fks(task_id = "tasks" pk on_delete cascade))]
    pub task_attack_target_data: TaskAttackTargetDataTable,

    #[table(singular = "task_attack_move_data",
            fks(task_id = "tasks" pk on_delete cascade))]
    pub task_attack_move_data: TaskAttackMoveDataTable,

    #[table(singular = "activity")]
    pub activities: ActivityTable,

    #[table(singular = "activity_participant",
            fks(activity_id = "activities" on_delete cascade,
                creature_id = "creatures" on_delete cascade))]
    pub activity_participants: ActivityParticipantTable,

    #[table(singular = "music_composition", auto)]
    pub music_compositions: MusicCompositionTable,

    #[table(singular = "blueprint",
            fks(task_id? = "tasks",
                composition_id? = "music_compositions"))]
    pub blueprints: BlueprintTable,

    #[table(singular = "structure", fks(project_id = "blueprints"))]
    pub structures: CompletedStructureTable,

    #[table(singular = "inventory", auto)]
    pub inventories: InventoryTable,

    #[table(singular = "item_stack",
            auto,
            fks(inventory_id = "inventories",
                owner? = "creatures",
                reserved_by? = "tasks",
                enchantment_id? = "item_enchantments"))]
    pub item_stacks: ItemStackTable,

    #[table(singular = "item_subcomponent",
            nonpk_auto,
            fks(item_stack_id = "item_stacks" on_delete cascade))]
    pub item_subcomponents: ItemSubcomponentTable,

    #[table(singular = "item_enchantment", auto)]
    pub item_enchantments: ItemEnchantmentTable,

    #[table(singular = "enchantment_effect",
            nonpk_auto,
            fks(enchantment_id = "item_enchantments" on_delete cascade))]
    pub enchantment_effects: EnchantmentEffectTable,

    #[table(singular = "ground_pile", auto)]
    pub ground_piles: GroundPileTable,

    #[table(
        singular = "logistics_want_row",
        nonpk_auto,
        fks(inventory_id = "inventories")
    )]
    pub logistics_want_rows: LogisticsWantRowTable,

    #[table(singular = "furniture", auto, fks(structure_id = "structures"))]
    pub furniture: FurnitureTable,

    #[table(singular = "notification", auto)]
    pub notifications: NotificationTable,

    #[table(singular = "strut",
            auto,
            fks(blueprint_id? = "blueprints" on_delete cascade,
                structure_id? = "structures" on_delete nullify))]
    pub struts: StrutTable,

    #[table(singular = "projectile",
            auto,
            fks(shooter? = "creatures" on_delete nullify,
                inventory_id = "inventories"))]
    pub projectiles: ProjectileTable,
}
