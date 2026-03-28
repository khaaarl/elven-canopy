// Core types shared across the simulation.
//
// Defines spatial coordinates, entity identifiers, voxel types, and game enums
// used throughout the sim. This is the foundational types file — nearly every
// other module imports from here.
//
// ## Sections
//
// - **Spatial types:** `VoxelCoord` — integer position in the 3D voxel grid.
// - **Entity IDs:** Strongly-typed UUID v4 wrappers generated via the
//   `entity_id!` macro. Each entity type gets its own newtype around `SimUuid`
//   so the compiler prevents mixing them up. Current IDs: `TreeId`,
//   `CreatureId`, `ProjectId`, `TaskId`, `ActivityId`.
// - **Sequential IDs:** `StructureId` is a sequential `u64` newtype (not
//   UUID-based) for user-friendly numbering (#0, #1, #2). Assigned by
//   `SimState` when a build completes.
// - **Nav graph IDs:** `NavNodeId` and `NavEdgeId` — compact `u32` wrappers
//   (not UUIDs) since nav nodes are rebuilt from world geometry and never
//   persisted across sessions.
// - **Simulation enums:** `Species`, `Priority`, `BuildType`.
// - **Creature biology:** `TraitKind` (enum of all biological trait names)
//   and `TraitValue` (Int or Text sum type). Stored in the `creature_traits`
//   table in `db.rs`. See `sim/creature.rs` for trait rolling at spawn time.
// - **Vital status:** `VitalStatus` (Alive/Incapacitated/Dead), `DeathCause` (Debug/Damage/Starvation).
//   Dead creatures remain in the DB; all live-creature queries filter by status.
// - **Thought system:** `ThoughtKind` — event-driven creature thoughts with
//   per-kind dedup and expiry. `Thought` — a timestamped thought instance.
// - **Voxel types:** `VoxelType` — the material at each grid cell (`Air`,
//   `Trunk`, `Branch`, `Root`, `Leaf`, `Dirt`, etc.).
//
// `SimUuid` is a hand-rolled UUID v4 (RFC 4122) generated deterministically
// from the sim's `GameRng`. It serializes as the standard 8-4-4-4-12 hex
// string so it can serve as a JSON map key.
//
// All types derive `Serialize` and `Deserialize` for save/load and multiplayer
// state transfer.
//
// See also: `elven_canopy_prng` for the PRNG that generates entity IDs,
// `sim/mod.rs` for the `SimState` that owns entities keyed by these IDs,
// `world.rs` for the voxel grid indexed by `VoxelCoord`.
//
// **Critical constraint: determinism.** Entity IDs are generated from the sim's
// `GameRng` (from `elven_canopy_prng`). Do not use external UUID libraries or
// OS entropy.

use crate::prng::GameRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use tabulosity::Bounded;

// ---------------------------------------------------------------------------
// Spatial types
// ---------------------------------------------------------------------------

/// A position in the 3D voxel grid. Each component is in voxel units.
///
/// The coordinate system uses right-handed conventions:
/// - X: east  (positive) / west  (negative)
/// - Y: up    (positive) / down  (negative)
/// - Z: south (positive) / north (negative)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VoxelCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl VoxelCoord {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Manhattan distance between two coordinates.
    pub fn manhattan_distance(self, other: Self) -> u32 {
        ((self.x - other.x).unsigned_abs())
            + ((self.y - other.y).unsigned_abs())
            + ((self.z - other.z).unsigned_abs())
    }

    /// Returns all voxel coordinates along a **6-connected** line from `self`
    /// to `other`, inclusive of both endpoints.
    ///
    /// Every consecutive pair of voxels in the returned list shares a face
    /// (differs by exactly 1 on a single axis). This produces longer paths
    /// than 26-connected Bresenham but guarantees structural face-adjacency
    /// throughout the strut.
    ///
    /// **Symmetry guarantee:** `a.line_to(b)` produces the same set of
    /// coordinates as `b.line_to(a)` (reversed order). Achieved by always
    /// iterating from the lexicographically smaller endpoint (x, then y,
    /// then z to break ties).
    ///
    /// **Tie-breaking:** When multiple axes have equal accumulated error,
    /// the axis with lower index (x < y < z) is stepped first. Combined
    /// with the canonical direction, this ensures deterministic results.
    pub fn line_to(self, other: VoxelCoord) -> Vec<VoxelCoord> {
        // Canonical direction: always iterate from the lex-smaller endpoint.
        let (start, end) = if self <= other {
            (self, other)
        } else {
            (other, self)
        };

        let dx = (end.x - start.x).abs();
        let dy = (end.y - start.y).abs();
        let dz = (end.z - start.z).abs();

        let sx = (end.x - start.x).signum();
        let sy = (end.y - start.y).signum();
        let sz = (end.z - start.z).signum();

        // Total number of steps in a 6-connected line is dx + dy + dz.
        let total_steps = (dx + dy + dz) as usize;
        let mut result = Vec::with_capacity(total_steps + 1);

        let mut cur = start;
        result.push(cur);

        // Error accumulators: track how far behind each axis is.
        // We use a scaled error approach: each step increments all axis
        // errors by their respective deltas. We then step whichever axis
        // has the largest accumulated error (ties broken by axis index).
        let mut err_x: i32 = 0;
        let mut err_y: i32 = 0;
        let mut err_z: i32 = 0;

        for _ in 0..total_steps {
            err_x += dx;
            err_y += dy;
            err_z += dz;

            // Step the axis with the largest error. Ties broken: x > y > z
            // (lower-index axis preferred — since we compare >=).
            if err_x >= err_y && err_x >= err_z {
                cur.x += sx;
                err_x -= dx + dy + dz;
            } else if err_y >= err_z {
                cur.y += sy;
                err_y -= dx + dy + dz;
            } else {
                cur.z += sz;
                err_z -= dx + dy + dz;
            }

            result.push(cur);
        }

        // If we iterated from `other` to `self`, reverse so the caller's
        // endpoint order is preserved.
        if self > other {
            result.reverse();
        }

        result
    }
}

impl fmt::Display for VoxelCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.x, self.y, self.z)
    }
}

// ---------------------------------------------------------------------------
// Entity IDs — deterministic UUID v4
// ---------------------------------------------------------------------------

/// A UUID v4, generated deterministically from the simulation PRNG.
///
/// Layout follows RFC 4122: 128 bits with version nibble (bits 48–51) set
/// to `0100` and variant bits (bits 64–65) set to `10`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SimUuid([u8; 16]);

impl SimUuid {
    /// Generate a deterministic UUID v4 from the simulation PRNG.
    pub fn new_v4(rng: &mut GameRng) -> Self {
        let mut bytes = rng.next_128_bits();
        // Set version nibble (byte 6, upper nibble) to 0100.
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        // Set variant bits (byte 8, upper 2 bits) to 10.
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        Self(bytes)
    }

    /// Parse a UUID from its 8-4-4-4-12 hex string representation.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        let hex: String = s.chars().filter(|c| *c != '-').collect();
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

// Custom serde: serialize as the 8-4-4-4-12 hex string so SimUuid can be
// used as a JSON map key (serde_json requires string keys).
impl Serialize for SimUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SimUuid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        SimUuid::from_str(&s).ok_or_else(|| serde::de::Error::custom("invalid UUID format"))
    }
}

impl Bounded for SimUuid {
    const MIN: Self = SimUuid([0u8; 16]);
    const MAX: Self = SimUuid([0xFF; 16]);
}

impl fmt::Debug for SimUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SimUuid({})", self)
    }
}

impl fmt::Display for SimUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Standard 8-4-4-4-12 hex representation.
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15],
        )
    }
}

// ---------------------------------------------------------------------------
// Strongly-typed entity ID wrappers
// ---------------------------------------------------------------------------

macro_rules! entity_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub SimUuid);

        impl $name {
            pub fn new(rng: &mut GameRng) -> Self {
                Self(SimUuid::new_v4(rng))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

entity_id!(/// Unique identifier for a tree entity.
TreeId);
entity_id!(/// Unique identifier for a creature entity (elf, capybara, etc.).
CreatureId);
entity_id!(/// Unique identifier for an in-progress build project.
ProjectId);
entity_id!(/// Unique identifier for a task (go-to, build, harvest, etc.).
TaskId);
entity_id!(/// Unique identifier for a group activity (dance, construction choir, etc.).
ActivityId);

// Bounded impls for UUID-based entity IDs. These enable use as tabulosity
// primary keys. AutoIncrementable is intentionally not implemented — UUIDs
// are generated from the PRNG, not auto-incremented.
impl Bounded for TreeId {
    const MIN: Self = TreeId(SimUuid::MIN);
    const MAX: Self = TreeId(SimUuid::MAX);
}
impl Bounded for CreatureId {
    const MIN: Self = CreatureId(SimUuid::MIN);
    const MAX: Self = CreatureId(SimUuid::MAX);
}
impl Bounded for ProjectId {
    const MIN: Self = ProjectId(SimUuid::MIN);
    const MAX: Self = ProjectId(SimUuid::MAX);
}
impl Bounded for TaskId {
    const MIN: Self = TaskId(SimUuid::MIN);
    const MAX: Self = TaskId(SimUuid::MAX);
}
impl Bounded for ActivityId {
    const MIN: Self = ActivityId(SimUuid::MIN);
    const MAX: Self = ActivityId(SimUuid::MAX);
}

// ---------------------------------------------------------------------------
// Sequential IDs — user-friendly numbering, not UUIDs.
// ---------------------------------------------------------------------------

/// Unique identifier for a completed structure (platform, bridge, etc.).
///
/// Sequential `u64` newtype assigned by `SimState` when a build completes,
/// providing user-friendly numbering (#0, #1, #2, ...) instead of UUIDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StructureId(pub u64);

impl Bounded for StructureId {
    const MIN: Self = StructureId(u64::MIN);
    const MAX: Self = StructureId(u64::MAX);
}

impl fmt::Display for StructureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Auto-PK newtypes for tabulosity child tables
// ---------------------------------------------------------------------------

macro_rules! auto_pk_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Bounded)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

auto_pk_id!(/// Auto-increment ID for abstract inventory containers.
InventoryId);
auto_pk_id!(/// Auto-increment ID for item stacks within inventories.
ItemStackId);
auto_pk_id!(/// Auto-increment ID for ground item piles.
GroundPileId);
auto_pk_id!(/// Auto-increment ID for furniture placements.
FurnitureId);
auto_pk_id!(/// Auto-increment ID for player-visible notifications.
NotificationId);
auto_pk_id!(/// Auto-increment ID for item enchantment instances.
EnchantmentId);
auto_pk_id!(/// Auto-increment ID for active recipe entries on crafting buildings.
ActiveRecipeId);
auto_pk_id!(/// Auto-increment ID for per-output target rows on active recipes.
ActiveRecipeTargetId);
auto_pk_id!(/// Auto-increment ID for music compositions.
CompositionId);
auto_pk_id!(/// Auto-increment ID for projectiles in flight.
ProjectileId);
auto_pk_id!(/// Auto-increment ID for support struts.
StrutId);
auto_pk_id!(/// Auto-increment ID for military groups within a civilization.
MilitaryGroupId);
auto_pk_id!(/// Auto-increment ID for player selection groups (Ctrl+1–9).
SelectionGroupId);

// ---------------------------------------------------------------------------
// Creature biology traits
// ---------------------------------------------------------------------------

/// Identifies a biological trait. Flat namespace covering all species — each
/// creature only has rows for species-relevant traits. Stored as a column in
/// the `creature_traits` table alongside a `TraitValue`.
///
/// Visual traits are palette indices (`TraitValue::Int`) into the color/style
/// arrays in `elven_canopy_sprites`. `BioSeed` is a raw PRNG output stored
/// for future trait derivation without advancing the sim PRNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TraitKind {
    /// Per-creature random seed for future trait derivation.
    BioSeed,
    // -- Elf traits --
    /// Index into `HAIR_COLORS` palette (0–6).
    HairColor,
    /// Index into `EYE_COLORS` palette (0–4).
    EyeColor,
    /// Index into `SKIN_TONES` palette (0–3).
    SkinTone,
    /// Index into `HAIR_STYLES` array (0–2).
    HairStyle,
    // -- Shared body color (Deer, Boar, Elephant, Capybara) --
    /// Index into species-specific `BODY_COLORS` palette.
    BodyColor,
    // -- Deer traits --
    /// Index into `ANTLER_STYLES` array (0–2).
    AntlerStyle,
    /// Index into `SPOT_PATTERNS` array (0–1).
    SpotPattern,
    // -- Boar traits --
    /// Index into `TUSK_SIZES` array (0–2).
    TuskSize,
    // -- Troll / Wyvern traits --
    /// Index into species-specific `HORN_STYLES` array (0–2).
    HornStyle,
    // -- Squirrel / Monkey traits (shared name, different palettes) --
    /// Index into species-specific `FUR_COLORS` palette.
    FurColor,
    /// Index into `TAIL_TYPES` array (0–2). Squirrel-specific.
    TailType,
    // -- Elephant traits --
    /// Index into `TUSK_TYPES` array (0–2).
    TuskType,
    // -- Monkey traits --
    /// Index into `FACE_MARKINGS` array (0–2).
    FaceMarking,
    // -- Orc traits --
    /// Index into `WAR_PAINTS` array (0–2).
    WarPaint,
    // -- Goblin traits --
    /// Index into `EAR_STYLES` array (0–2).
    EarStyle,
    // -- Shared skin color (Goblin, Orc, Troll) --
    /// Index into species-specific `SKIN_COLORS` palette.
    SkinColor,
    // -- Capybara traits --
    /// Index into `ACCESSORIES` array (0–3).
    Accessory,
    // -- Hornet traits --
    /// Index into `STRIPE_PATTERNS` array (0–2).
    StripePattern,
    /// Index into `WING_STYLES` array (0–2).
    WingStyle,
    // -- Wyvern traits --
    /// Index into `SCALE_PATTERNS` array (0–2).
    ScalePattern,
    // -- Creature skills (F-creature-skills) --
    // Integer scale starting at 0. Every +100 doubles mechanical intensity
    // via the same exponential stat multiplier table used by creature stats.
    // Advancement is probabilistic: each relevant action rolls against a
    // configurable probability; success increments the skill by 1.
    // Capped by path tier (see SkillConfig).
    /// Melee combat proficiency.
    Striking,
    /// Ranged combat proficiency.
    Archery,
    /// Dodge and threat avoidance.
    Evasion,
    /// Stealth, navigation, tracking, awareness.
    Ranging,
    /// Plant lifecycle: foraging, cultivation, extraction.
    Herbalism,
    /// Creature interaction and taming.
    Beastcraft,
    /// Food preparation.
    Cuisine,
    /// Textile work: weaving, sewing, dyeing.
    Tailoring,
    /// Wood knowledge, carpentry, non-magical shaping.
    Woodcraft,
    /// Potions, salves, magical material transformation.
    Alchemy,
    /// Musical performance and vocal mana channeling.
    Singing,
    /// Mana sensitivity and channeling efficiency.
    Channeling,
    /// Written/verbal composition, lore, scholarship.
    Literature,
    /// Visual creation: sculpture, decoration, aesthetics.
    Art,
    /// Persuasion, intimidation, social maneuvering.
    Influence,
    /// Tradition, ceremony, ritual, storytelling.
    Culture,
    /// Teaching, mentoring, guidance.
    Counsel,
    // -- Creature stats (F-creature-stats) --
    // Integer scale centered on 0 (human baseline). Every +100 doubles
    // mechanical intensity via the exponential stat multiplier table.
    /// Physical power. Melee damage multiplier, projectile velocity.
    Strength,
    /// Nimbleness. Move speed, climb speed (blended with Strength).
    Agility,
    /// Fine motor control. Arrow deviation, crafting quality (future).
    Dexterity,
    /// Toughness. HP max multiplier.
    Constitution,
    /// Mental fortitude. Mana pool size (WIL scales mp_max at spawn),
    /// mana regen rate (avg(WIL, INT) scales mana_per_tick at heartbeat).
    Willpower,
    /// Cognitive ability. Mana regen rate (avg(WIL, INT) scales mana_per_tick
    /// at heartbeat), crafting quality (future).
    Intelligence,
    /// Awareness. Hostile detection range (F-per-detection), crafting (future).
    Perception,
    /// Social magnetism. Singing effectiveness, mana recharge rate (future).
    Charisma,
}

/// The value of a creature biological trait. `Int` covers palette indices,
/// seeds, and future numeric stats. `Text` is available for freeform traits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TraitValue {
    Int(i64),
    Text(String),
}

impl TraitValue {
    /// Extract as integer, returning `default` if this is a `Text` variant.
    pub fn as_int(&self, default: i64) -> i64 {
        match self {
            TraitValue::Int(v) => *v,
            TraitValue::Text(_) => default,
        }
    }

    /// Extract as string, returning `default` if this is an `Int` variant.
    pub fn as_text(&self, default: &str) -> String {
        match self {
            TraitValue::Text(s) => s.clone(),
            TraitValue::Int(_) => default.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Elf paths — discipline a creature walks (F-path-core).
// ---------------------------------------------------------------------------

/// Identifier for a creature's path (discipline/specialization).
///
/// Paths determine which skills receive elevated caps and extra advancement
/// rolls. Each path is associated with a set of skills defined in
/// `PathConfig`. Currently a fixed enum; future paths (specializations,
/// esoteric) will extend this.
///
/// `Outcast` is the default path assigned to all elves at spawn. It confers
/// no skill bonuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PathId {
    /// Default path — no skill bonuses. All elves start here.
    Outcast,
    /// Combat path. Associated skills: Striking, Archery, Evasion.
    Warrior,
    /// Combat path focused on exploration and beast-taming. Associated skills: Beastcraft, Ranging.
    Scout,
}

impl PathId {
    /// Full display name shown in the UI (e.g., "Way of the Warrior").
    pub fn display_name(self) -> &'static str {
        match self {
            PathId::Outcast => "Way of the Outcast",
            PathId::Warrior => "Way of the Warrior",
            PathId::Scout => "Way of the Scout",
        }
    }

    /// Short name for compact UI contexts (e.g., "Warrior").
    pub fn short_name(self) -> &'static str {
        match self {
            PathId::Outcast => "Outcast",
            PathId::Warrior => "Warrior",
            PathId::Scout => "Scout",
        }
    }

    /// All path variants, in display order.
    pub const ALL: &[PathId] = &[PathId::Outcast, PathId::Warrior, PathId::Scout];
}

impl fmt::Display for PathId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Category of a path. Combat paths are player-assigned only; civil paths
/// can eventually self-assign. Currently all implemented paths are Combat
/// (or Outcast which is its own category).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PathCategory {
    /// No category — the default Outcast path.
    None,
    /// Warrior, Archer, Guard, etc.
    Combat,
    /// Cook, Harvester, Artisan, etc.
    Civil,
}

// ---------------------------------------------------------------------------
// Civilization IDs — sequential u16, assigned by worldgen in batch.
// ---------------------------------------------------------------------------

/// Civilization identifier. Assigned sequentially by worldgen (0, 1, 2, ...).
/// Not auto-increment — civs are batch-created during worldgen and not
/// created at runtime in the initial implementation.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Bounded,
)]
pub struct CivId(pub u16);

impl fmt::Display for CivId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CivId({})", self.0)
    }
}

// ---------------------------------------------------------------------------
// Fruit species IDs — sequential u16, assigned by worldgen in batch.
// ---------------------------------------------------------------------------

/// Fruit species identifier. Assigned sequentially by worldgen (0, 1, 2, ...).
/// Not auto-increment — species are batch-created during worldgen.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Bounded,
)]
pub struct FruitSpeciesId(pub u16);

impl fmt::Display for FruitSpeciesId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FruitSpeciesId({})", self.0)
    }
}

// ---------------------------------------------------------------------------
// Nav graph IDs — simple integers, not UUIDs, for compactness.
// ---------------------------------------------------------------------------

/// Compact identifier for a navigation graph node.
/// Not serializable — nav node IDs are ephemeral and change when the graph is rebuilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NavNodeId(pub u32);

/// Compact identifier for a navigation graph edge.
/// Not serializable — nav edge IDs are ephemeral and change when the graph is rebuilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NavEdgeId(pub u32);

// ---------------------------------------------------------------------------
// Simulation enums
// ---------------------------------------------------------------------------

/// Species of creature. Used as a key into `SpeciesData` in `GameConfig`
/// to drive all behavioral differences from data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Species {
    Elf,
    Capybara,
    Boar,
    Deer,
    Elephant,
    Goblin,
    Monkey,
    Orc,
    Squirrel,
    Troll,
    Hornet,
    Wyvern,
}

/// Species that can form civilizations. Separate from the sim-active `Species`
/// enum — `Species` tracks creature types with instances, rendering, and
/// pathfinding, while `CivSpecies` tracks sapient species that form organized
/// societies. They overlap at `Elf` but serve different purposes. See
/// `docs/drafts/elfcyclopedia_civs.md` §Species Enums for the convergence plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CivSpecies {
    Elf,
    Human,
    Dwarf,
    Goblin,
    Orc,
    Troll,
}

impl CivSpecies {
    /// Convert to the sim-active `Species` enum. Returns `None` for species
    /// that don't have creature instances yet (Human, Dwarf).
    pub fn to_species(self) -> Option<Species> {
        match self {
            CivSpecies::Elf => Some(Species::Elf),
            CivSpecies::Goblin => Some(Species::Goblin),
            CivSpecies::Orc => Some(Species::Orc),
            CivSpecies::Troll => Some(Species::Troll),
            CivSpecies::Human | CivSpecies::Dwarf => None,
        }
    }

    /// All variants in declaration order, for iteration.
    pub const ALL: [CivSpecies; 6] = [
        CivSpecies::Elf,
        CivSpecies::Human,
        CivSpecies::Dwarf,
        CivSpecies::Goblin,
        CivSpecies::Orc,
        CivSpecies::Troll,
    ];

    /// Human-readable display string.
    pub fn display_str(self) -> &'static str {
        match self {
            CivSpecies::Elf => "Elf",
            CivSpecies::Human => "Human",
            CivSpecies::Dwarf => "Dwarf",
            CivSpecies::Goblin => "Goblin",
            CivSpecies::Orc => "Orc",
            CivSpecies::Troll => "Troll",
        }
    }
}

/// Diplomatic opinion one civilization holds of another. Ordered from most
/// positive to most negative for easy comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CivOpinion {
    Friendly,
    Neutral,
    Suspicious,
    Hostile,
}

impl CivOpinion {
    /// Human-readable display string.
    pub fn display_str(self) -> &'static str {
        match self {
            CivOpinion::Friendly => "Friendly",
            CivOpinion::Neutral => "Neutral",
            CivOpinion::Suspicious => "Suspicious",
            CivOpinion::Hostile => "Hostile",
        }
    }

    /// Shift one step toward friendlier (Hostile→Suspicious→Neutral→Friendly).
    /// Returns self if already Friendly.
    pub fn shift_friendlier(self) -> Self {
        match self {
            CivOpinion::Friendly => CivOpinion::Friendly,
            CivOpinion::Neutral => CivOpinion::Friendly,
            CivOpinion::Suspicious => CivOpinion::Neutral,
            CivOpinion::Hostile => CivOpinion::Suspicious,
        }
    }

    /// Shift one step toward hostility (Friendly→Neutral→Suspicious→Hostile).
    /// Returns self if already Hostile.
    pub fn shift_hostile(self) -> Self {
        match self {
            CivOpinion::Friendly => CivOpinion::Neutral,
            CivOpinion::Neutral => CivOpinion::Suspicious,
            CivOpinion::Suspicious => CivOpinion::Hostile,
            CivOpinion::Hostile => CivOpinion::Hostile,
        }
    }
}

/// How a subject (creature or civ) feels about an object (creature or civ).
///
/// Used by combat AI (`is_non_hostile`, `detect_hostile_targets`), the bridge
/// layer (minimap dots, selection ring colors), and GDScript UI code.
/// Computed by `SimState::diplomatic_relation()` and its convenience wrappers.
///
/// Currently three variants; designed for future extension (e.g., `Allied`
/// for a friendly non-player civ we won't defend but avoid hitting with
/// stray arrows, or `Suspicious` for pre-hostility tension).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiplomaticRelation {
    /// Same civilization — allies, defend each other, avoid friendly fire.
    Friendly,
    /// No strong opinion — neither help nor hinder.
    Neutral,
    /// Actively hostile — valid combat target.
    Hostile,
}

impl DiplomaticRelation {
    /// Returns `true` if the relation is `Hostile`.
    pub fn is_hostile(self) -> bool {
        matches!(self, Self::Hostile)
    }

    /// Returns `true` if the relation is `Friendly`.
    pub fn is_friendly(self) -> bool {
        matches!(self, Self::Friendly)
    }
}

/// Cultural flavor tag for a civilization. Assigned during worldgen with
/// species-biased random selection. Not mechanically significant in the
/// initial implementation — purely for flavor and elfcyclopedia display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CultureTag {
    Woodland,
    Mountain,
    Coastal,
    Subterranean,
    Nomadic,
    Martial,
}

impl CultureTag {
    /// Human-readable display string.
    pub fn display_str(self) -> &'static str {
        match self {
            CultureTag::Woodland => "Woodland",
            CultureTag::Mountain => "Mountain",
            CultureTag::Coastal => "Coastal",
            CultureTag::Subterranean => "Subterranean",
            CultureTag::Nomadic => "Nomadic",
            CultureTag::Martial => "Martial",
        }
    }
}

/// Priority level for build projects and tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}

/// Subtype of ladder construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LadderKind {
    Wood,
    Rope,
}

/// Types of furnishing that can be applied to a completed building.
///
/// Each variant maps to a specific `FurnitureKind` via `furniture_kind()`.
/// The furniture kind determines the visual representation (mesh shape and
/// color) in the renderer, while the furnishing type drives functional
/// behavior (future: room bonuses, elf preferences).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FurnishingType {
    ConcertHall,
    DanceHall,
    DiningHall,
    Dormitory,
    Greenhouse,
    Home,
    Kitchen,
    Storehouse,
    Workshop,
}

impl FurnishingType {
    /// The kind of furniture placed in a room of this type.
    ///
    /// DanceHall has no furniture — this method should never be called for it
    /// in the furniture pipeline (furnish_structure early-exits). Returns Bench
    /// as a harmless default for exhaustive matching.
    pub fn furniture_kind(self) -> FurnitureKind {
        match self {
            FurnishingType::ConcertHall | FurnishingType::DanceHall => FurnitureKind::Bench,
            FurnishingType::DiningHall => FurnitureKind::Table,
            FurnishingType::Dormitory | FurnishingType::Home => FurnitureKind::Bed,
            FurnishingType::Greenhouse => FurnitureKind::Planter,
            FurnishingType::Kitchen => FurnitureKind::Counter,
            FurnishingType::Storehouse => FurnitureKind::Shelf,
            FurnishingType::Workshop => FurnitureKind::Workbench,
        }
    }

    /// Human-readable display string for this furnishing type.
    pub fn display_str(self) -> &'static str {
        match self {
            FurnishingType::ConcertHall => "Concert Hall",
            FurnishingType::DanceHall => "Dance Hall",
            FurnishingType::DiningHall => "Dining Hall",
            FurnishingType::Dormitory => "Dormitory",
            FurnishingType::Greenhouse => "Greenhouse",
            FurnishingType::Home => "Home",
            FurnishingType::Kitchen => "Kitchen",
            FurnishingType::Storehouse => "Storehouse",
            FurnishingType::Workshop => "Workshop",
        }
    }
}

/// Visual kind of furniture placed inside a furnished building.
///
/// Derived from `FurnishingType::furniture_kind()` — not stored per-item.
/// The renderer uses the discriminant value (as `i32`) for dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(i32)]
pub enum FurnitureKind {
    Bed = 0,
    Bench = 1,
    Counter = 2,
    Shelf = 3,
    Table = 4,
    Workbench = 5,
    Planter = 6,
}

impl FurnitureKind {
    /// Plural noun for this furniture kind, used in UI labels
    /// (e.g. "4 beds", "2 tables").
    pub fn noun_plural(self) -> &'static str {
        match self {
            FurnitureKind::Bed => "beds",
            FurnitureKind::Bench => "benches",
            FurnitureKind::Counter => "counters",
            FurnitureKind::Shelf => "shelves",
            FurnitureKind::Table => "tables",
            FurnitureKind::Workbench => "workbenches",
            FurnitureKind::Planter => "planters",
        }
    }
}

// ---------------------------------------------------------------------------
// Vital status and death
// ---------------------------------------------------------------------------

/// Whether a creature is alive or dead. Dead creatures remain in the database
/// (row NOT deleted) to support future states (ghost, spirit absorbed into
/// tree, undead) and to preserve history for UI/narrative. `Incapacitated`
/// creatures are at or below 0 HP and bleeding out — they cannot act but are
/// still targetable and rendered (with visual changes). The `Dead` variant
/// terminates heartbeat/activation chains and excludes the creature from
/// rendering, task assignment, and all active simulation queries.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum VitalStatus {
    #[default]
    Alive,
    /// Creature is at or below 0 HP and bleeding out. Cannot act, but is
    /// still targetable. True death occurs when HP reaches `-hp_max`.
    Incapacitated,
    Dead,
    // Future: Ghost, SpiritInTree, Undead, etc.
}

/// Why a creature died. Stored in the `CreatureDied` event for narrative
/// display and future gameplay effects (e.g., different funeral rites for
/// starvation vs. combat death).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeathCause {
    /// Killed by debug command.
    Debug,
    /// HP reduced past -hp_max by damage (incapacitation bleed-out or overkill).
    Damage,
    /// Food gauge reached zero.
    Starvation,
    /// Fell from a height (creature gravity).
    Falling,
}

// ---------------------------------------------------------------------------
// Thought system
// ---------------------------------------------------------------------------

/// The kind of thought a creature can have. Each variant carries IDs needed
/// to distinguish meaningfully different instances (e.g., "low ceiling in
/// building A" vs. "low ceiling in building B"). `PartialEq` is derived for
/// dedup comparison in `Creature::add_thought()`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThoughtKind {
    SleptInOwnHome(StructureId),
    SleptInDormitory(StructureId),
    SleptOnGround,
    /// Ate a meal in a dining hall. Provides a small mood boost.
    /// `AteMeal` from old saves deserializes to this variant.
    #[serde(alias = "AteMeal")]
    AteDining,
    /// Ate alone (carried food or foraging). Small mood penalty.
    AteAlone,
    LowCeiling(StructureId),
    /// Recurring small mood boost while participating in a group dance.
    EnjoyingDance,
    /// Moderate mood boost on completing a group dance.
    DancedInGroup,
}

impl ThoughtKind {
    /// Human-readable description for UI display.
    pub fn description(&self) -> &'static str {
        match self {
            ThoughtKind::SleptInOwnHome(_) => "Slept in own home",
            ThoughtKind::SleptInDormitory(_) => "Slept in a dormitory",
            ThoughtKind::SleptOnGround => "Slept on the ground",
            ThoughtKind::AteDining => "Ate in a dining hall",
            ThoughtKind::AteAlone => "Ate alone",
            ThoughtKind::LowCeiling(_) => "Bothered by a low ceiling",
            ThoughtKind::EnjoyingDance => "Enjoying a group dance",
            ThoughtKind::DancedInGroup => "Danced with friends",
        }
    }
}

/// Coarse mood category derived from the numeric mood score. Seven tiers from
/// worst to best, used for UI display and as thresholds for gameplay effects
/// (e.g., mana generation multiplier). See `MoodConfig::tier()` for the mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoodTier {
    Devastated,
    Miserable,
    Unhappy,
    Neutral,
    Content,
    Happy,
    Elated,
}

impl MoodTier {
    /// Human-readable label for UI display.
    pub fn label(&self) -> &'static str {
        match self {
            MoodTier::Devastated => "Devastated",
            MoodTier::Miserable => "Miserable",
            MoodTier::Unhappy => "Unhappy",
            MoodTier::Neutral => "Neutral",
            MoodTier::Content => "Content",
            MoodTier::Happy => "Happy",
            MoodTier::Elated => "Elated",
        }
    }
}

// ---------------------------------------------------------------------------
// Group activity system — multi-creature coordination layer
// ---------------------------------------------------------------------------

/// What kind of group activity this is. Each kind has default config for
/// departure policy, late-join behavior, and recruitment mode. Extension
/// tables (keyed by `ActivityId`) carry kind-specific data — see
/// `sim/activity.rs` for the lifecycle logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActivityKind {
    /// Construction choir — sing to grow a structure.
    ConstructionChoir,
    /// Social dance — recreational, generates happiness.
    Dance,
    /// Combat singing — buff nearby allies.
    CombatSinging,
    /// Heavy hauling — move a large object together.
    GroupHaul,
    /// Ceremony/ritual — seasonal or milestone event.
    Ceremony,
}

/// Lifecycle phase of a group activity. See `sim/activity.rs` for transition
/// rules and the full state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActivityPhase {
    /// Open for volunteers / awaiting directed assignments. Participants with
    /// `Volunteered` status are tentative — not yet committed.
    Recruiting,
    /// Participants have been committed and are walking to their positions.
    /// Transitions to `Executing` when enough have arrived.
    Assembling,
    /// The group activity is in progress. Progress advances each tick.
    Executing,
    /// Activity finished successfully.
    Complete,
    /// Execution paused because a participant left and the departure policy
    /// is `PauseAndWait`. Resumes if the gap is filled; cancelled on timeout.
    Paused,
    /// Activity was cancelled.
    Cancelled,
}

/// How participants join this activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RecruitmentMode {
    /// Push: the activity (or player) selects and assigns specific creatures.
    Directed,
    /// Pull: the activity advertises open slots. Idle creatures discover it
    /// during their activation loop and volunteer to join.
    Open,
}

/// What happens when a participant leaves during the `Executing` phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DeparturePolicy {
    /// Remaining participants keep going at reduced effectiveness.
    Continue,
    /// Activity pauses, waiting for a replacement. Cancelled after timeout.
    PauseAndWait {
        /// How long to wait (in sim ticks) before cancelling.
        timeout_ticks: u64,
    },
    /// Activity is cancelled immediately if any participant leaves.
    CancelOnDeparture,
}

/// A participant's role within the activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParticipantRole {
    /// Organizer/lead — initiated the activity.
    Organizer,
    /// Regular participant.
    Member,
}

/// A participant's commitment status within the activity. Progresses from
/// `Volunteered` (tentative, creature is free) → `Traveling` (committed,
/// has GoTo task) → `Arrived` (at position, waiting or executing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParticipantStatus {
    /// Tentative — creature is free to do other things. If the creature picks
    /// up a task or activity before quorum is reached, this row is pruned.
    /// `creature.current_activity` is NOT set in this state.
    Volunteered,
    /// Committed — has a GoTo task, walking to assigned position.
    /// `creature.current_activity` IS set.
    Traveling,
    /// At assigned position, waiting for others or actively executing.
    /// `creature.current_activity` IS set.
    Arrived,
}

/// Types of structures that can be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildType {
    Platform = 0,
    // 1 and 2 formerly Bridge and Stairs — removed (dead code, never producible).
    Wall = 3,
    Enclosure = 4,
    /// A building with paper-thin walls. Produces `BuildingInterior` voxels
    /// with per-face restrictions stored in `SimState.face_data`.
    Building = 5,
    /// A wood ladder — anchored to any adjacent solid voxel along its face.
    WoodLadder = 6,
    /// A rope ladder — hangs from the topmost anchor point.
    RopeLadder = 7,
    /// Carve (remove) solid voxels to Air. The inverse of construction.
    Carve = 8,
    /// A diagonal support strut between two endpoints. Produces `Strut` voxels
    /// along a 6-connected line. Uses custom replacement validation instead of
    /// the normal `allows_tree_overlap()` flow.
    Strut = 9,
}

impl BuildType {
    /// Returns true for structural build types that can overlap tree geometry.
    ///
    /// When true, designation allows Trunk/Branch/Root voxels (skipped) and
    /// Leaf/Fruit voxels (converted during construction). When false, all
    /// voxels must be Air (existing behavior).
    pub fn allows_tree_overlap(self) -> bool {
        matches!(
            self,
            BuildType::Platform | BuildType::WoodLadder | BuildType::RopeLadder
        )
    }

    /// Returns true for the Carve build type (removes voxels instead of placing them).
    pub fn is_carve(self) -> bool {
        matches!(self, BuildType::Carve)
    }

    /// Map a build type to the voxel type it produces when materialized.
    pub fn to_voxel_type(self) -> VoxelType {
        match self {
            BuildType::Platform => VoxelType::GrownPlatform,
            BuildType::Wall | BuildType::Enclosure => VoxelType::GrownWall,
            BuildType::Building => VoxelType::BuildingInterior,
            BuildType::WoodLadder => VoxelType::WoodLadder,
            BuildType::RopeLadder => VoxelType::RopeLadder,
            BuildType::Carve => VoxelType::Air,
            BuildType::Strut => VoxelType::Strut,
        }
    }
}

// ---------------------------------------------------------------------------
// Face types for building construction
// ---------------------------------------------------------------------------

/// A cardinal direction on a voxel face. Used to describe per-face properties
/// of `BuildingInterior` voxels (walls, windows, doors, etc.).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FaceDirection {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl FaceDirection {
    /// All six face directions in a fixed order.
    pub const ALL: [FaceDirection; 6] = [
        FaceDirection::PosX,
        FaceDirection::NegX,
        FaceDirection::PosY,
        FaceDirection::NegY,
        FaceDirection::PosZ,
        FaceDirection::NegZ,
    ];

    /// Return the opposite face direction.
    pub fn opposite(self) -> FaceDirection {
        match self {
            FaceDirection::PosX => FaceDirection::NegX,
            FaceDirection::NegX => FaceDirection::PosX,
            FaceDirection::PosY => FaceDirection::NegY,
            FaceDirection::NegY => FaceDirection::PosY,
            FaceDirection::PosZ => FaceDirection::NegZ,
            FaceDirection::NegZ => FaceDirection::PosZ,
        }
    }

    /// Convert a face direction to a unit offset vector.
    pub fn to_offset(self) -> (i32, i32, i32) {
        match self {
            FaceDirection::PosX => (1, 0, 0),
            FaceDirection::NegX => (-1, 0, 0),
            FaceDirection::PosY => (0, 1, 0),
            FaceDirection::NegY => (0, -1, 0),
            FaceDirection::PosZ => (0, 0, 1),
            FaceDirection::NegZ => (0, 0, -1),
        }
    }

    /// Convert a unit offset to a face direction, if the offset is cardinal.
    pub fn from_offset(dx: i32, dy: i32, dz: i32) -> Option<FaceDirection> {
        match (dx, dy, dz) {
            (1, 0, 0) => Some(FaceDirection::PosX),
            (-1, 0, 0) => Some(FaceDirection::NegX),
            (0, 1, 0) => Some(FaceDirection::PosY),
            (0, -1, 0) => Some(FaceDirection::NegY),
            (0, 0, 1) => Some(FaceDirection::PosZ),
            (0, 0, -1) => Some(FaceDirection::NegZ),
            _ => None,
        }
    }

    /// Return the index of this direction in `FaceDirection::ALL`.
    pub fn index(self) -> usize {
        match self {
            FaceDirection::PosX => 0,
            FaceDirection::NegX => 1,
            FaceDirection::PosY => 2,
            FaceDirection::NegY => 3,
            FaceDirection::PosZ => 4,
            FaceDirection::NegZ => 5,
        }
    }
}

/// The type of a voxel face. Determines visibility and movement restrictions
/// for `BuildingInterior` voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FaceType {
    /// No barrier — movement passes freely.
    Open,
    /// Opaque barrier — blocks movement and sight.
    Wall,
    /// Transparent barrier — blocks movement, allows sight.
    Window,
    /// Passable barrier — allows movement, visually distinct.
    Door,
    /// Horizontal barrier above — blocks upward movement.
    Ceiling,
    /// Horizontal barrier below — blocks downward movement, walkable.
    Floor,
}

impl FaceType {
    /// Returns true if this face type blocks creature movement through it.
    pub fn blocks_movement(self) -> bool {
        match self {
            FaceType::Open | FaceType::Door => false,
            FaceType::Wall | FaceType::Window | FaceType::Ceiling | FaceType::Floor => true,
        }
    }
}

/// Per-face data for `BuildingInterior` and ladder voxels. Stores one
/// `FaceType` per cardinal direction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaceData {
    /// Face types indexed by `FaceDirection::index()`.
    pub faces: [FaceType; 6],
}

impl Default for FaceData {
    fn default() -> Self {
        Self {
            faces: [FaceType::Open; 6],
        }
    }
}

impl FaceData {
    /// Get the face type for a given direction.
    pub fn get(&self, dir: FaceDirection) -> FaceType {
        self.faces[dir.index()]
    }

    /// Set the face type for a given direction.
    pub fn set(&mut self, dir: FaceDirection, face_type: FaceType) {
        self.faces[dir.index()] = face_type;
    }
}

// ---------------------------------------------------------------------------
// Voxel types
// ---------------------------------------------------------------------------

/// The material/type of a single voxel in the world grid.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[repr(u8)]
pub enum VoxelType {
    #[default]
    Air = 0,
    Trunk = 1,
    Branch = 2,
    GrownPlatform = 3,
    GrownWall = 4,
    // 5 and 6 formerly GrownStairs and Bridge — removed.
    // 7 formerly ForestFloor — removed (Dirt terrain covers the world).
    Dirt = 8,
    Leaf = 9,
    Fruit = 10,
    Root = 11,
    /// A passable voxel inside a building. Movement restrictions come from
    /// per-face `FaceData` stored separately in `SimState.face_data`, not from
    /// the voxel type itself. This means `is_solid()` returns false.
    BuildingInterior = 12,
    /// A wood ladder voxel. Non-solid, with per-face restrictions in `face_data`
    /// and orientation stored in `ladder_orientations`.
    WoodLadder = 13,
    /// A rope ladder voxel. Non-solid, same face/orientation storage as WoodLadder.
    RopeLadder = 14,
    /// A support strut voxel. Solid like wood, but carries rod-spring metadata
    /// for efficient diagonal load transfer via chain-topology springs along
    /// the strut axis. Can be placed through natural materials (dirt, trunk,
    /// leaves) and existing completed struts.
    Strut = 15,
}

/// Classification of a voxel for construction overlap with tree geometry.
///
/// Used by `designate_build()` when `BuildType::allows_tree_overlap()` is true
/// to decide which voxels need blueprint entries, which are skipped, and which
/// block placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlapClassification {
    /// Air — normal blueprint voxel, will be built.
    Exterior,
    /// Leaf or Fruit — will be replaced with the target type during construction.
    Convertible,
    /// Trunk, Branch, or Root — already wood, skip (no blueprint voxel needed).
    AlreadyWood,
    /// Dirt, existing construction — blocks placement.
    Blocked,
}

impl VoxelType {
    /// Returns `true` for opaque voxel types that block visibility of
    /// adjacent faces for mesh generation. Opaque voxels cull the faces of
    /// neighboring voxels that touch them. Distinct from `is_solid()`:
    /// `Leaf` is solid but non-opaque (transparent faces must still be
    /// rendered).
    pub fn is_opaque(self) -> bool {
        matches!(
            self,
            VoxelType::Trunk
                | VoxelType::Branch
                | VoxelType::Root
                | VoxelType::Dirt
                | VoxelType::GrownPlatform
                | VoxelType::GrownWall
                | VoxelType::Strut
        )
    }

    /// Returns `true` for any voxel type that blocks occupancy. Non-solid
    /// types: `Air`, `BuildingInterior`, `WoodLadder`, `RopeLadder`.
    pub fn is_solid(self) -> bool {
        !matches!(
            self,
            VoxelType::Air
                | VoxelType::BuildingInterior
                | VoxelType::WoodLadder
                | VoxelType::RopeLadder
        )
    }

    /// Returns `true` for voxel types a flying creature can occupy. Includes
    /// all non-solid types plus Leaf and Fruit (sparse canopy doesn't block
    /// flight, same as it doesn't block line-of-sight). Flying creatures can
    /// pass through air, building interiors, ladders, leaves, and fruit but
    /// not through trunk, branch, walls, floors, etc.
    pub fn is_flyable(self) -> bool {
        !self.is_solid() || matches!(self, VoxelType::Leaf | VoxelType::Fruit)
    }

    /// Returns `true` for voxel types that block line-of-sight for ranged
    /// attacks. Same as `is_solid()` except Leaf and Fruit are transparent
    /// (sparse canopy doesn't block arrows).
    pub fn blocks_los(self) -> bool {
        self.is_solid() && !matches!(self, VoxelType::Leaf | VoxelType::Fruit)
    }

    /// Returns true if this voxel type is a ladder (wood or rope).
    pub fn is_ladder(self) -> bool {
        matches!(self, VoxelType::WoodLadder | VoxelType::RopeLadder)
    }

    /// One past the highest discriminant value. Used for iteration in tests.
    /// Not equal to the variant count when there are gaps from removed variants.
    pub const MAX_DISCRIMINANT_PLUS_ONE: usize = 16;

    /// Convert a `u8` discriminant back to a `VoxelType`. Returns `Air` for
    /// out-of-range or gap values.
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => VoxelType::Air,
            1 => VoxelType::Trunk,
            2 => VoxelType::Branch,
            3 => VoxelType::GrownPlatform,
            4 => VoxelType::GrownWall,
            // 5, 6: formerly GrownStairs, Bridge — removed.
            // 7: formerly ForestFloor — removed.
            8 => VoxelType::Dirt,
            9 => VoxelType::Leaf,
            10 => VoxelType::Fruit,
            11 => VoxelType::Root,
            12 => VoxelType::BuildingInterior,
            13 => VoxelType::WoodLadder,
            14 => VoxelType::RopeLadder,
            15 => VoxelType::Strut,
            _ => VoxelType::Air,
        }
    }

    /// Convert to the `u8` discriminant. Safe because `VoxelType` is `#[repr(u8)]`.
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Classify this voxel type for construction overlap with tree geometry.
    pub fn classify_for_overlap(self) -> OverlapClassification {
        match self {
            VoxelType::Air => OverlapClassification::Exterior,
            VoxelType::Leaf | VoxelType::Fruit => OverlapClassification::Convertible,
            VoxelType::Trunk | VoxelType::Branch | VoxelType::Root => {
                OverlapClassification::AlreadyWood
            }
            VoxelType::Dirt
            | VoxelType::GrownPlatform
            | VoxelType::GrownWall
            | VoxelType::BuildingInterior
            | VoxelType::WoodLadder
            | VoxelType::RopeLadder
            | VoxelType::Strut => OverlapClassification::Blocked,
        }
    }
}

// Compile-time check: VoxelType fits in a u8.
const _: () = assert!(std::mem::size_of::<VoxelType>() == 1);

/// Compute the `FaceData` for a ladder voxel with the given orientation.
///
/// The orientation is the face the ladder panel is on (e.g., PosX means the
/// ladder panel is on the +X face of the voxel). Only the ladder panel
/// itself blocks movement; all other faces are open.
pub fn ladder_face_data(orientation: FaceDirection) -> FaceData {
    let mut fd = FaceData::default();
    fd.set(orientation, FaceType::Wall);
    fd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v4_version_and_variant_bits() {
        let mut rng = GameRng::new(42);
        for _ in 0..1000 {
            let uuid = SimUuid::new_v4(&mut rng);
            let bytes = uuid.as_bytes();
            // Version nibble (byte 6 upper) must be 0x4_.
            assert_eq!(bytes[6] >> 4, 4, "UUID version must be 4");
            // Variant bits (byte 8 upper 2) must be 0b10.
            assert_eq!(bytes[8] >> 6, 2, "UUID variant must be RFC 4122");
        }
    }

    #[test]
    fn uuid_determinism() {
        let mut rng_a = GameRng::new(42);
        let mut rng_b = GameRng::new(42);
        for _ in 0..100 {
            assert_eq!(SimUuid::new_v4(&mut rng_a), SimUuid::new_v4(&mut rng_b));
        }
    }

    #[test]
    fn entity_id_determinism() {
        let mut rng_a = GameRng::new(99);
        let mut rng_b = GameRng::new(99);
        assert_eq!(TreeId::new(&mut rng_a), TreeId::new(&mut rng_b));
        assert_eq!(CreatureId::new(&mut rng_a), CreatureId::new(&mut rng_b));
    }

    #[test]
    fn uuid_display_format() {
        let mut rng = GameRng::new(42);
        let uuid = SimUuid::new_v4(&mut rng);
        let s = uuid.to_string();
        // 8-4-4-4-12 hex = 32 hex chars + 4 dashes = 36 chars
        assert_eq!(s.len(), 36);
        assert_eq!(&s[8..9], "-");
        assert_eq!(&s[13..14], "-");
        assert_eq!(&s[18..19], "-");
        assert_eq!(&s[23..24], "-");
    }

    #[test]
    fn uuid_serialization_roundtrip() {
        let mut rng = GameRng::new(42);
        let uuid = SimUuid::new_v4(&mut rng);
        let json = serde_json::to_string(&uuid).unwrap();
        let restored: SimUuid = serde_json::from_str(&json).unwrap();
        assert_eq!(uuid, restored);
    }

    #[test]
    fn voxel_coord_manhattan_distance() {
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(3, 4, 5);
        assert_eq!(a.manhattan_distance(b), 12);
        assert_eq!(b.manhattan_distance(a), 12);
    }

    #[test]
    fn voxel_type_discriminant_values() {
        // Explicit discriminants are load-bearing for save compatibility.
        // If these change, old saves with packed voxel data will break.
        assert_eq!(VoxelType::Air as u8, 0);
        assert_eq!(VoxelType::Trunk as u8, 1);
        assert_eq!(VoxelType::Branch as u8, 2);
        assert_eq!(VoxelType::GrownPlatform as u8, 3);
        assert_eq!(VoxelType::GrownWall as u8, 4);
        // 5, 6: removed (formerly GrownStairs, Bridge)
        // 7: removed (formerly ForestFloor)
        assert_eq!(VoxelType::Dirt as u8, 8);
        assert_eq!(VoxelType::Leaf as u8, 9);
        assert_eq!(VoxelType::Fruit as u8, 10);
        assert_eq!(VoxelType::Root as u8, 11);
        assert_eq!(VoxelType::BuildingInterior as u8, 12);
        assert_eq!(VoxelType::WoodLadder as u8, 13);
        assert_eq!(VoxelType::RopeLadder as u8, 14);
        assert_eq!(VoxelType::Strut as u8, 15);
    }

    #[test]
    fn build_type_to_voxel_type() {
        assert_eq!(
            BuildType::Platform.to_voxel_type(),
            VoxelType::GrownPlatform
        );
        assert_eq!(BuildType::Wall.to_voxel_type(), VoxelType::GrownWall);
        assert_eq!(BuildType::Enclosure.to_voxel_type(), VoxelType::GrownWall);
        assert_eq!(
            BuildType::Building.to_voxel_type(),
            VoxelType::BuildingInterior
        );
    }

    #[test]
    fn building_interior_not_solid() {
        assert!(!VoxelType::BuildingInterior.is_solid());
        assert!(!VoxelType::Air.is_solid());
        assert!(VoxelType::Trunk.is_solid());
        assert!(VoxelType::GrownPlatform.is_solid());
    }

    #[test]
    fn face_type_blocks_movement() {
        assert!(!FaceType::Open.blocks_movement());
        assert!(!FaceType::Door.blocks_movement());
        assert!(FaceType::Wall.blocks_movement());
        assert!(FaceType::Window.blocks_movement());
        assert!(FaceType::Ceiling.blocks_movement());
        assert!(FaceType::Floor.blocks_movement());
    }

    #[test]
    fn face_direction_opposite() {
        assert_eq!(FaceDirection::PosX.opposite(), FaceDirection::NegX);
        assert_eq!(FaceDirection::NegX.opposite(), FaceDirection::PosX);
        assert_eq!(FaceDirection::PosY.opposite(), FaceDirection::NegY);
        assert_eq!(FaceDirection::NegY.opposite(), FaceDirection::PosY);
        assert_eq!(FaceDirection::PosZ.opposite(), FaceDirection::NegZ);
        assert_eq!(FaceDirection::NegZ.opposite(), FaceDirection::PosZ);
        // Double opposite is identity.
        for dir in FaceDirection::ALL {
            assert_eq!(dir.opposite().opposite(), dir);
        }
    }

    #[test]
    fn face_data_default_all_open() {
        let fd = FaceData::default();
        for dir in FaceDirection::ALL {
            assert_eq!(fd.get(dir), FaceType::Open);
        }
    }

    #[test]
    fn face_data_get_set() {
        let mut fd = FaceData::default();
        fd.set(FaceDirection::PosX, FaceType::Wall);
        fd.set(FaceDirection::NegY, FaceType::Floor);
        assert_eq!(fd.get(FaceDirection::PosX), FaceType::Wall);
        assert_eq!(fd.get(FaceDirection::NegY), FaceType::Floor);
        assert_eq!(fd.get(FaceDirection::PosZ), FaceType::Open);
    }

    #[test]
    fn allows_tree_overlap_structural_types() {
        assert!(BuildType::Platform.allows_tree_overlap());
    }

    #[test]
    fn allows_tree_overlap_non_structural_types() {
        assert!(!BuildType::Wall.allows_tree_overlap());
        assert!(!BuildType::Enclosure.allows_tree_overlap());
        assert!(!BuildType::Building.allows_tree_overlap());
        assert!(!BuildType::Carve.allows_tree_overlap());
    }

    #[test]
    fn carve_build_type() {
        assert_eq!(BuildType::Carve.to_voxel_type(), VoxelType::Air);
        assert!(BuildType::Carve.is_carve());
        assert!(!BuildType::Platform.is_carve());
    }

    #[test]
    fn classify_for_overlap_exterior() {
        assert_eq!(
            VoxelType::Air.classify_for_overlap(),
            OverlapClassification::Exterior
        );
    }

    #[test]
    fn classify_for_overlap_convertible() {
        assert_eq!(
            VoxelType::Leaf.classify_for_overlap(),
            OverlapClassification::Convertible
        );
        assert_eq!(
            VoxelType::Fruit.classify_for_overlap(),
            OverlapClassification::Convertible
        );
    }

    #[test]
    fn classify_for_overlap_already_wood() {
        assert_eq!(
            VoxelType::Trunk.classify_for_overlap(),
            OverlapClassification::AlreadyWood
        );
        assert_eq!(
            VoxelType::Branch.classify_for_overlap(),
            OverlapClassification::AlreadyWood
        );
        assert_eq!(
            VoxelType::Root.classify_for_overlap(),
            OverlapClassification::AlreadyWood
        );
    }

    // --- Activity type serde roundtrip tests ---

    #[test]
    fn activity_enums_serde_roundtrip() {
        use crate::types::{
            ActivityKind, ActivityPhase, DeparturePolicy, ParticipantRole, ParticipantStatus,
            RecruitmentMode,
        };

        // ActivityKind
        let kinds = [
            ActivityKind::ConstructionChoir,
            ActivityKind::Dance,
            ActivityKind::CombatSinging,
            ActivityKind::GroupHaul,
            ActivityKind::Ceremony,
        ];
        for kind in &kinds {
            let json = serde_json::to_string(kind).unwrap();
            let restored: ActivityKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, restored);
        }

        // ActivityPhase
        let phases = [
            ActivityPhase::Recruiting,
            ActivityPhase::Assembling,
            ActivityPhase::Executing,
            ActivityPhase::Complete,
            ActivityPhase::Paused,
            ActivityPhase::Cancelled,
        ];
        for phase in &phases {
            let json = serde_json::to_string(phase).unwrap();
            let restored: ActivityPhase = serde_json::from_str(&json).unwrap();
            assert_eq!(*phase, restored);
        }

        // RecruitmentMode
        let modes = [RecruitmentMode::Directed, RecruitmentMode::Open];
        for mode in &modes {
            let json = serde_json::to_string(mode).unwrap();
            let restored: RecruitmentMode = serde_json::from_str(&json).unwrap();
            assert_eq!(*mode, restored);
        }

        // DeparturePolicy
        let policies = [
            DeparturePolicy::Continue,
            DeparturePolicy::PauseAndWait {
                timeout_ticks: 5000,
            },
            DeparturePolicy::CancelOnDeparture,
        ];
        for policy in &policies {
            let json = serde_json::to_string(policy).unwrap();
            let restored: DeparturePolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(*policy, restored);
        }

        // ParticipantRole
        let roles = [ParticipantRole::Organizer, ParticipantRole::Member];
        for role in &roles {
            let json = serde_json::to_string(role).unwrap();
            let restored: ParticipantRole = serde_json::from_str(&json).unwrap();
            assert_eq!(*role, restored);
        }

        // ParticipantStatus
        let statuses = [
            ParticipantStatus::Volunteered,
            ParticipantStatus::Traveling,
            ParticipantStatus::Arrived,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let restored: ParticipantStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*status, restored);
        }
    }

    #[test]
    fn activity_id_serde_roundtrip() {
        let mut rng = crate::prng::GameRng::new(42);
        let id = ActivityId::new(&mut rng);
        let json = serde_json::to_string(&id).unwrap();
        let restored: ActivityId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, restored);
    }

    #[test]
    fn activity_table_serde_roundtrip() {
        use crate::db::{Activity, ActivityParticipant};
        use crate::task::TaskOrigin;

        let mut rng = crate::prng::GameRng::new(99);
        let activity_id = ActivityId::new(&mut rng);
        let creature_id = CreatureId::new(&mut rng);

        let activity = Activity {
            id: activity_id,
            kind: ActivityKind::Dance,
            phase: ActivityPhase::Recruiting,
            location: VoxelCoord::new(10, 51, 20),
            min_count: Some(3),
            desired_count: Some(3),
            progress: 0,
            total_cost: 1000,
            origin: TaskOrigin::Automated,
            recruitment: RecruitmentMode::Open,
            departure_policy: DeparturePolicy::Continue,
            allows_late_join: true,
            civ_id: Some(CivId(0)),
            required_species: Some(Species::Elf),
            execution_start_tick: None,
            pause_started_tick: None,
            assembly_started_tick: None,
        };
        let json = serde_json::to_string(&activity).unwrap();
        let restored: Activity = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, activity.id);
        assert_eq!(restored.kind, activity.kind);
        assert_eq!(restored.phase, activity.phase);
        assert_eq!(restored.min_count, activity.min_count);
        assert_eq!(restored.civ_id, activity.civ_id);
        assert_eq!(restored.required_species, activity.required_species);

        let participant = ActivityParticipant {
            activity_id,
            creature_id,
            role: ParticipantRole::Member,
            status: ParticipantStatus::Volunteered,
            assigned_position: VoxelCoord::new(10, 51, 20),
            travel_task: None,
            dance_slot: None,
            waypoint_cursor: 0,
        };
        let json = serde_json::to_string(&participant).unwrap();
        let restored: ActivityParticipant = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.activity_id, participant.activity_id);
        assert_eq!(restored.creature_id, participant.creature_id);
        assert_eq!(restored.role, participant.role);
        assert_eq!(restored.status, participant.status);
    }

    #[test]
    fn dance_thought_kinds_serde_roundtrip() {
        let kinds = [ThoughtKind::EnjoyingDance, ThoughtKind::DancedInGroup];
        for kind in &kinds {
            let json = serde_json::to_string(kind).unwrap();
            let restored: ThoughtKind = serde_json::from_str(&json).unwrap();
            assert_eq!(*kind, restored);
        }
    }

    #[test]
    fn classify_for_overlap_blocked() {
        assert_eq!(
            VoxelType::GrownPlatform.classify_for_overlap(),
            OverlapClassification::Blocked
        );
        assert_eq!(
            VoxelType::GrownWall.classify_for_overlap(),
            OverlapClassification::Blocked
        );
        assert_eq!(
            VoxelType::BuildingInterior.classify_for_overlap(),
            OverlapClassification::Blocked
        );
    }

    #[test]
    fn is_opaque_covers_expected_types() {
        // Opaque types — cull adjacent faces in mesh generation.
        assert!(VoxelType::Trunk.is_opaque());
        assert!(VoxelType::Branch.is_opaque());
        assert!(VoxelType::Root.is_opaque());
        assert!(VoxelType::Dirt.is_opaque());
        assert!(VoxelType::GrownPlatform.is_opaque());
        assert!(VoxelType::GrownWall.is_opaque());
        // Non-opaque types — faces toward these are still visible.
        assert!(!VoxelType::Air.is_opaque());
        assert!(!VoxelType::Leaf.is_opaque());
        assert!(!VoxelType::Fruit.is_opaque());
        assert!(!VoxelType::BuildingInterior.is_opaque());
        assert!(!VoxelType::WoodLadder.is_opaque());
        assert!(!VoxelType::RopeLadder.is_opaque());
    }

    #[test]
    fn dirt_is_solid() {
        assert!(VoxelType::Dirt.is_solid());
    }

    #[test]
    fn dirt_blocks_overlap() {
        assert_eq!(
            VoxelType::Dirt.classify_for_overlap(),
            OverlapClassification::Blocked
        );
    }

    #[test]
    fn voxel_coord_ordering() {
        // Verify VoxelCoord has a total order (needed for BTreeMap keys).
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(1, 0, 0);
        assert!(a < b);
    }

    #[test]
    fn thought_kind_description() {
        assert_eq!(
            ThoughtKind::SleptInOwnHome(StructureId(0)).description(),
            "Slept in own home"
        );
        assert_eq!(
            ThoughtKind::SleptInDormitory(StructureId(1)).description(),
            "Slept in a dormitory"
        );
        assert_eq!(
            ThoughtKind::SleptOnGround.description(),
            "Slept on the ground"
        );
        assert_eq!(ThoughtKind::AteDining.description(), "Ate in a dining hall");
        assert_eq!(ThoughtKind::AteAlone.description(), "Ate alone");
        assert_eq!(
            ThoughtKind::LowCeiling(StructureId(2)).description(),
            "Bothered by a low ceiling"
        );
    }

    #[test]
    fn thought_kind_equality_with_different_structure_ids() {
        // Same variant, same ID → equal (for dedup).
        assert_eq!(
            ThoughtKind::SleptInOwnHome(StructureId(1)),
            ThoughtKind::SleptInOwnHome(StructureId(1))
        );
        // Same variant, different ID → not equal (distinct buildings).
        assert_ne!(
            ThoughtKind::SleptInOwnHome(StructureId(1)),
            ThoughtKind::SleptInOwnHome(StructureId(2))
        );
        // Different variants → not equal.
        assert_ne!(ThoughtKind::SleptOnGround, ThoughtKind::AteDining);
    }

    #[test]
    fn thought_serialization_roundtrip() {
        let thought = crate::db::Thought {
            creature_id: CreatureId(SimUuid::new_v4(&mut crate::prng::GameRng::new(1))),
            seq: 1,
            kind: ThoughtKind::SleptInOwnHome(StructureId(42)),
            tick: 12345,
        };
        let json = serde_json::to_string(&thought).unwrap();
        let restored: crate::db::Thought = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.kind, thought.kind);
        assert_eq!(restored.tick, thought.tick);
    }

    // --- Strut type property tests ---

    #[test]
    fn strut_voxel_type_properties() {
        assert!(VoxelType::Strut.is_solid());
        assert!(VoxelType::Strut.is_opaque());
        assert!(VoxelType::Strut.blocks_los());
        assert!(!VoxelType::Strut.is_ladder());
        assert_eq!(
            VoxelType::Strut.classify_for_overlap(),
            OverlapClassification::Blocked
        );
    }

    #[test]
    fn strut_build_type_properties() {
        assert_eq!(BuildType::Strut.to_voxel_type(), VoxelType::Strut);
        assert!(!BuildType::Strut.allows_tree_overlap());
        assert!(!BuildType::Strut.is_carve());
    }

    // --- VoxelCoord::line_to() tests ---

    /// Helper: verify every consecutive pair in the line shares a face.
    fn assert_face_connected(line: &[VoxelCoord]) {
        for pair in line.windows(2) {
            let d = pair[0].manhattan_distance(pair[1]);
            assert_eq!(
                d, 1,
                "Consecutive voxels {:?} and {:?} have manhattan distance {}, expected 1",
                pair[0], pair[1], d
            );
        }
    }

    #[test]
    fn line_to_same_point() {
        let a = VoxelCoord::new(5, 5, 5);
        let line = a.line_to(a);
        assert_eq!(line, vec![a]);
    }

    #[test]
    fn line_to_axis_aligned_x() {
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(5, 0, 0);
        let line = a.line_to(b);
        assert_eq!(line.len(), 6); // 0..=5
        assert_eq!(line[0], a);
        assert_eq!(line[5], b);
        assert_face_connected(&line);
    }

    #[test]
    fn line_to_axis_aligned_y() {
        let a = VoxelCoord::new(3, 1, 7);
        let b = VoxelCoord::new(3, 8, 7);
        let line = a.line_to(b);
        assert_eq!(line.len(), 8); // dy=7, total steps=7, +1 for start
        assert_eq!(line[0], a);
        assert_eq!(*line.last().unwrap(), b);
        assert_face_connected(&line);
    }

    #[test]
    fn line_to_axis_aligned_z() {
        let a = VoxelCoord::new(2, 3, 0);
        let b = VoxelCoord::new(2, 3, 10);
        let line = a.line_to(b);
        assert_eq!(line.len(), 11);
        assert_face_connected(&line);
    }

    #[test]
    fn line_to_negative_direction() {
        let a = VoxelCoord::new(10, 10, 10);
        let b = VoxelCoord::new(5, 10, 10);
        let line = a.line_to(b);
        assert_eq!(line.len(), 6);
        assert_eq!(line[0], a);
        assert_eq!(*line.last().unwrap(), b);
        assert_face_connected(&line);
    }

    #[test]
    fn line_to_diagonal_2d() {
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(4, 4, 0);
        let line = a.line_to(b);
        // 6-connected: dx=4, dy=4, dz=0 → 8 steps + 1 = 9 voxels
        assert_eq!(line.len(), 9);
        assert_eq!(line[0], a);
        assert_eq!(*line.last().unwrap(), b);
        assert_face_connected(&line);
    }

    #[test]
    fn line_to_diagonal_3d() {
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(3, 3, 3);
        let line = a.line_to(b);
        // 6-connected: dx+dy+dz = 9 steps + 1 = 10 voxels
        assert_eq!(line.len(), 10);
        assert_eq!(line[0], a);
        assert_eq!(*line.last().unwrap(), b);
        assert_face_connected(&line);
    }

    #[test]
    fn line_to_arbitrary_slope() {
        let a = VoxelCoord::new(1, 2, 3);
        let b = VoxelCoord::new(7, 4, 6);
        // dx=6, dy=2, dz=3 → 11 steps + 1 = 12 voxels
        let line = a.line_to(b);
        assert_eq!(line.len(), 12);
        assert_eq!(line[0], a);
        assert_eq!(*line.last().unwrap(), b);
        assert_face_connected(&line);
    }

    #[test]
    fn line_to_symmetry_same_voxel_set() {
        // a.line_to(b) and b.line_to(a) must produce the same set of voxels.
        let cases = vec![
            (VoxelCoord::new(0, 0, 0), VoxelCoord::new(5, 3, 7)),
            (VoxelCoord::new(1, 2, 3), VoxelCoord::new(7, 4, 6)),
            (VoxelCoord::new(0, 0, 0), VoxelCoord::new(4, 4, 0)),
            (VoxelCoord::new(10, 5, 3), VoxelCoord::new(2, 8, 1)),
            (VoxelCoord::new(0, 0, 0), VoxelCoord::new(0, 0, 0)),
        ];
        for (a, b) in cases {
            let forward = a.line_to(b);
            let backward = b.line_to(a);
            // Same length.
            assert_eq!(
                forward.len(),
                backward.len(),
                "Different lengths for {:?} → {:?}",
                a,
                b
            );
            // Same voxel set (backward is the reverse of forward).
            let mut forward_sorted = forward.clone();
            forward_sorted.sort();
            let mut backward_sorted = backward.clone();
            backward_sorted.sort();
            assert_eq!(
                forward_sorted, backward_sorted,
                "Different voxel sets for {:?} → {:?}",
                a, b
            );
            // More specifically: backward should be forward reversed.
            let mut forward_rev = forward.clone();
            forward_rev.reverse();
            assert_eq!(
                forward_rev, backward,
                "b.line_to(a) should be the reverse of a.line_to(b) for {:?} → {:?}",
                a, b
            );
        }
    }

    #[test]
    fn line_to_length_is_manhattan_distance_plus_one() {
        // For a 6-connected line, the number of voxels is manhattan_distance + 1.
        let cases = vec![
            (VoxelCoord::new(0, 0, 0), VoxelCoord::new(5, 3, 7)),
            (VoxelCoord::new(1, 1, 1), VoxelCoord::new(1, 1, 1)),
            (VoxelCoord::new(0, 0, 0), VoxelCoord::new(10, 0, 0)),
            (VoxelCoord::new(3, 7, 2), VoxelCoord::new(8, 1, 5)),
        ];
        for (a, b) in cases {
            let line = a.line_to(b);
            let expected = a.manhattan_distance(b) as usize + 1;
            assert_eq!(
                line.len(),
                expected,
                "Line from {:?} to {:?}: expected {} voxels, got {}",
                a,
                b,
                expected,
                line.len()
            );
        }
    }

    #[test]
    fn line_to_tie_breaking_dx_eq_dy() {
        // When dx == dy, the algorithm should break ties deterministically.
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(3, 3, 0);
        let line = a.line_to(b);
        assert_face_connected(&line);
        assert_eq!(line.len(), 7);
        // The first step should be X (lower axis index breaks ties).
        assert_eq!(line[1], VoxelCoord::new(1, 0, 0));
    }

    #[test]
    fn line_to_tie_breaking_all_equal() {
        // dx == dy == dz: the algorithm should still produce a face-connected line.
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(2, 2, 2);
        let line = a.line_to(b);
        assert_face_connected(&line);
        assert_eq!(line.len(), 7);
        // First step should be X (lowest axis index).
        assert_eq!(line[1], VoxelCoord::new(1, 0, 0));
    }

    #[test]
    fn line_to_no_duplicate_voxels() {
        let cases = vec![
            (VoxelCoord::new(0, 0, 0), VoxelCoord::new(5, 3, 7)),
            (VoxelCoord::new(0, 0, 0), VoxelCoord::new(3, 3, 3)),
            (VoxelCoord::new(2, 5, 1), VoxelCoord::new(8, 2, 9)),
        ];
        for (a, b) in cases {
            let line = a.line_to(b);
            let mut seen = std::collections::BTreeSet::new();
            for &v in &line {
                assert!(
                    seen.insert(v),
                    "Duplicate voxel {:?} in line from {:?} to {:?}",
                    v,
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn line_to_adjacent_endpoints() {
        // Minimum strut: 2 voxels (the two endpoints sharing a face).
        let a = VoxelCoord::new(5, 5, 5);
        let b = VoxelCoord::new(5, 6, 5);
        let line = a.line_to(b);
        assert_eq!(line, vec![a, b]);
    }
}
