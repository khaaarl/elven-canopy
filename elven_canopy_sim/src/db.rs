// SimDb — tabulosity-based relational store for all simulation entities.
//
// Replaces the `BTreeMap<Id, Entity>` collections on `SimState` with a typed,
// FK-validated, indexed in-memory database. See `docs/drafts/sim_db_schema_v4.md`
// for the design rationale.
//
// ## Table layout
//
// The database has 22 tables organized in three tiers:
//
// **Entity tables:** `creatures`, `tasks`, `blueprints`, `structures` — the
// primary simulation entities, keyed by UUID-based or sequential IDs.
//
// **Child tables:** `thoughts`, `notifications`, `inventories`, `item_stacks`,
// `ground_piles`, `logistics_wants`, `furniture`, `music_compositions` —
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
// the determinism requirement. No `HashMap` usage.
//
// See also: `sim.rs` for `SimState` which owns the `SimDb` instance,
// `types.rs` for all ID types, `docs/drafts/sim_db_schema_v4.md` for the
// full schema design.
//
// **Critical constraint: determinism.** All iteration is in deterministic
// BTreeMap order. No hash-based collections.

use crate::inventory::{EffectKind, ItemKind, Material};
use crate::task::{HaulPhase, TaskOrigin, TaskState};
use crate::types::{
    BuildType, CompositionId, CreatureId, EnchantmentEffectId, EnchantmentId, FurnishingType,
    FurnitureId, GroundPileId, InventoryId, ItemStackId, ItemSubcomponentId, LogisticsWantId,
    NavNodeId, NotificationId, ProjectId, Species, StructureId, TaskAcquireDataId,
    TaskBlueprintRefId, TaskCraftDataId, TaskHaulDataId, TaskId, TaskSleepDataId,
    TaskStructureRefId, TaskVoxelRefId, ThoughtId, ThoughtKind, VoxelCoord,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tabulosity::{Database, Table};

use crate::blueprint::BlueprintState;
use crate::sim::CreaturePath;
use crate::types::{FaceData, Priority, VoxelType};

// ---------------------------------------------------------------------------
// Task decomposition enums
// ---------------------------------------------------------------------------

/// Discriminant-only tag for `TaskKind`. The base `Task` row stores this
/// instead of the full enum — variant-specific data lives in relationship
/// and extension tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskKindTag {
    GoTo,
    Build,
    EatBread,
    EatFruit,
    Furnish,
    Sleep,
    Haul,
    Cook,
    Harvest,
    AcquireItem,
    Mope,
    Craft,
}

impl TaskKindTag {
    /// Human-readable display name for UI purposes.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::GoTo => "GoTo",
            Self::Build => "Build",
            Self::EatBread => "EatBread",
            Self::EatFruit => "EatFruit",
            Self::Furnish => "Furnish",
            Self::Sleep => "Sleep",
            Self::Haul => "Haul",
            Self::Cook => "Cook",
            Self::Harvest => "Harvest",
            Self::AcquireItem => "AcquireItem",
            Self::Mope => "Moping",
            Self::Craft => "Craft",
        }
    }

    /// Derive the tag from a full `TaskKind` enum value.
    pub fn from_kind(kind: &crate::task::TaskKind) -> Self {
        use crate::task::TaskKind;
        match kind {
            TaskKind::GoTo => Self::GoTo,
            TaskKind::Build { .. } => Self::Build,
            TaskKind::EatBread => Self::EatBread,
            TaskKind::EatFruit { .. } => Self::EatFruit,
            TaskKind::Furnish { .. } => Self::Furnish,
            TaskKind::Sleep { .. } => Self::Sleep,
            TaskKind::Haul { .. } => Self::Haul,
            TaskKind::Cook { .. } => Self::Cook,
            TaskKind::Harvest { .. } => Self::Harvest,
            TaskKind::AcquireItem { .. } => Self::AcquireItem,
            TaskKind::Mope => Self::Mope,
            TaskKind::Craft { .. } => Self::Craft,
        }
    }
}

/// Role of a task-to-structure reference. Determines why a task references
/// a particular structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskStructureRole {
    FurnishTarget,
    CookAt,
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
    pub current_node: Option<NavNodeId>,
    #[indexed]
    pub current_task: Option<TaskId>,
    pub food: i64,
    pub rest: i64,
    #[indexed]
    pub assigned_home: Option<StructureId>,
    #[indexed]
    pub inventory_id: InventoryId,
    pub path: Option<CreaturePath>,
    // Movement interpolation metadata (rendering only).
    pub move_from: Option<VoxelCoord>,
    pub move_to: Option<VoxelCoord>,
    pub move_start_tick: u64,
    pub move_end_tick: u64,
}

/// A timestamped thought belonging to a creature.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Thought {
    #[primary_key(auto_increment)]
    pub id: ThoughtId,
    #[indexed]
    pub creature_id: CreatureId,
    pub kind: ThoughtKind,
    pub tick: u64,
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
    pub location: NavNodeId,
    pub progress: f32,
    pub total_cost: f32,
    #[indexed]
    pub required_species: Option<Species>,
    pub origin: TaskOrigin,
}

/// Task-to-blueprint reference (Build tasks only).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskBlueprintRef {
    #[primary_key(auto_increment)]
    pub id: TaskBlueprintRefId,
    #[indexed]
    pub task_id: TaskId,
    #[indexed]
    pub project_id: ProjectId,
}

/// Task-to-structure reference with role discriminant.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskStructureRef {
    #[primary_key(auto_increment)]
    pub id: TaskStructureRefId,
    #[indexed]
    pub task_id: TaskId,
    #[indexed]
    pub structure_id: StructureId,
    pub role: TaskStructureRole,
}

/// Task-to-voxel position reference with role discriminant.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskVoxelRef {
    #[primary_key(auto_increment)]
    pub id: TaskVoxelRefId,
    #[indexed]
    pub task_id: TaskId,
    pub coord: VoxelCoord,
    #[indexed]
    pub role: TaskVoxelRole,
}

/// Haul-specific mutable state (extension table). Uses auto-PK so that
/// task_id can be an indexed FK field for cascade-delete support.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskHaulData {
    #[primary_key(auto_increment)]
    pub id: TaskHaulDataId,
    #[indexed]
    pub task_id: TaskId,
    pub item_kind: ItemKind,
    pub quantity: u32,
    pub phase: HaulPhase,
    pub source_kind: HaulSourceKind,
    pub destination_nav_node: NavNodeId,
}

/// Sleep-specific state (extension table).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskSleepData {
    #[primary_key(auto_increment)]
    pub id: TaskSleepDataId,
    #[indexed]
    pub task_id: TaskId,
    pub sleep_location: SleepLocationType,
}

/// AcquireItem-specific state (extension table).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskAcquireData {
    #[primary_key(auto_increment)]
    pub id: TaskAcquireDataId,
    #[indexed]
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

impl Creature {
    /// Compute an interpolated world position for rendering.
    pub fn interpolated_position(&self, render_tick: f64) -> (f32, f32, f32) {
        if let (Some(from), Some(to)) = (self.move_from, self.move_to) {
            let duration = self.move_end_tick as f64 - self.move_start_tick as f64;
            if duration > 0.0 {
                let t =
                    ((render_tick - self.move_start_tick as f64) / duration).clamp(0.0, 1.0) as f32;
                let x = from.x as f32 + (to.x as f32 - from.x as f32) * t;
                let y = from.y as f32 + (to.y as f32 - from.y as f32) * t;
                let z = from.z as f32 + (to.z as f32 - from.z as f32) * t;
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
    pub cooking_enabled: bool,
    pub cooking_bread_target: u32,
    #[serde(default)]
    pub workshop_enabled: bool,
    #[serde(default)]
    pub workshop_recipe_ids: Vec<String>,
    /// Per-recipe output targets. Key = recipe ID, value = target quantity.
    /// A target of 0 or missing entry means "don't craft this recipe."
    #[serde(default)]
    pub workshop_recipe_targets: std::collections::BTreeMap<String, u32>,
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
            cooking_enabled: false,
            cooking_bread_target: 0,
            workshop_enabled: false,
            workshop_recipe_ids: Vec::new(),
            workshop_recipe_targets: std::collections::BTreeMap::new(),
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
            BuildType::Bridge => "Bridge",
            BuildType::Stairs => "Stairs",
            BuildType::Wall => "Wall",
            BuildType::Enclosure => "Enclosure",
            BuildType::Building => "Building",
            BuildType::WoodLadder => "Wood Ladder",
            BuildType::RopeLadder => "Rope Ladder",
            BuildType::Carve => "Carve",
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

/// An abstract inventory container. Creatures, structures, and ground piles
/// each own one via an `inventory_id` FK.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct Inventory {
    #[primary_key(auto_increment)]
    pub id: InventoryId,
    pub owner_kind: InventoryOwnerKind,
}

/// A stack of items within an inventory.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
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
    #[serde(default)]
    #[indexed]
    pub enchantment_id: Option<EnchantmentId>,
    #[indexed]
    pub owner: Option<CreatureId>,
    #[indexed]
    pub reserved_by: Option<TaskId>,
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

/// A desired item kind and target quantity for an inventory.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct LogisticsWantRow {
    #[primary_key(auto_increment)]
    pub id: LogisticsWantId,
    #[indexed]
    pub inventory_id: InventoryId,
    pub item_kind: ItemKind,
    pub target_quantity: u32,
}

/// A subcomponent record for a crafted item stack. Records what went into
/// crafting each item in the stack (e.g., a Bow contains 1 Bowstring).
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct ItemSubcomponent {
    #[primary_key(auto_increment)]
    pub id: ItemSubcomponentId,
    #[indexed]
    pub item_stack_id: ItemStackId,
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
pub struct EnchantmentEffect {
    #[primary_key(auto_increment)]
    pub id: EnchantmentEffectId,
    #[indexed]
    pub enchantment_id: EnchantmentId,
    pub effect_kind: EffectKind,
    pub magnitude: i32,
    pub threshold: Option<i32>,
}

/// Craft task extension data — stores the recipe ID for a Craft task.
#[derive(Table, Clone, Debug, Serialize, Deserialize)]
pub struct TaskCraftData {
    #[primary_key(auto_increment)]
    pub id: TaskCraftDataId,
    #[indexed]
    pub task_id: TaskId,
    pub recipe_id: String,
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
            .finish()
    }
}

#[derive(Database)]
pub struct SimDb {
    #[table(singular = "creature",
            fks(current_task? = "tasks",
                assigned_home? = "structures"))]
    pub creatures: CreatureTable,

    #[table(singular = "thought",
            auto,
            fks(creature_id = "creatures" on_delete cascade))]
    pub thoughts: ThoughtTable,

    #[table(singular = "task")]
    pub tasks: TaskTable,

    #[table(singular = "task_blueprint_ref",
            auto,
            fks(task_id = "tasks" on_delete cascade,
                project_id = "blueprints"))]
    pub task_blueprint_refs: TaskBlueprintRefTable,

    #[table(singular = "task_structure_ref",
            auto,
            fks(task_id = "tasks" on_delete cascade,
                structure_id = "structures"))]
    pub task_structure_refs: TaskStructureRefTable,

    #[table(singular = "task_voxel_ref",
            auto,
            fks(task_id = "tasks" on_delete cascade))]
    pub task_voxel_refs: TaskVoxelRefTable,

    #[table(singular = "task_haul_data",
            auto,
            fks(task_id = "tasks" on_delete cascade))]
    pub task_haul_data: TaskHaulDataTable,

    #[table(singular = "task_sleep_data",
            auto,
            fks(task_id = "tasks" on_delete cascade))]
    pub task_sleep_data: TaskSleepDataTable,

    #[table(singular = "task_acquire_data",
            auto,
            fks(task_id = "tasks" on_delete cascade))]
    pub task_acquire_data: TaskAcquireDataTable,

    #[table(singular = "task_craft_data",
            auto,
            fks(task_id = "tasks" on_delete cascade))]
    pub task_craft_data: TaskCraftDataTable,

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
            auto,
            fks(item_stack_id = "item_stacks" on_delete cascade))]
    pub item_subcomponents: ItemSubcomponentTable,

    #[table(singular = "item_enchantment", auto)]
    pub item_enchantments: ItemEnchantmentTable,

    #[table(singular = "enchantment_effect",
            auto,
            fks(enchantment_id = "item_enchantments" on_delete cascade))]
    pub enchantment_effects: EnchantmentEffectTable,

    #[table(singular = "ground_pile", auto)]
    pub ground_piles: GroundPileTable,

    #[table(
        singular = "logistics_want_row",
        auto,
        fks(inventory_id = "inventories")
    )]
    pub logistics_want_rows: LogisticsWantRowTable,

    #[table(singular = "furniture", auto, fks(structure_id = "structures"))]
    pub furniture: FurnitureTable,

    #[table(singular = "notification", auto)]
    pub notifications: NotificationTable,
}
