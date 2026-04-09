// Species data — Dwarf Fortress-style data-driven creature configuration.
//
// Most behavioral differences between creature species (elves, capybaras, etc.)
// are expressed as data in `SpeciesData`, keyed by `Species` in the game config.
// The sim code uses a single `Creature` type and reads species-specific values
// from the species table at runtime, including `engagement_style` for combat
// behavior and `hostile_detection_range_sq` for detection range.
//
// Current parameters:
// - `move_ticks_per_voxel` — base ticks to traverse 1.0 units of euclidean
//   distance. Higher = slower. For flyers, this is the flight speed. Edge
//   costs for climbing/ladders are fixed ratios determined by `movement_category`.
// - `movement_category` — `MovementCategory` enum (WalkOnly, WalkOrLadder,
//   Climber, Flyer) determining which edges the species can traverse and at
//   what cost relative to `move_ticks_per_voxel`. Copied to the Creature DB
//   row at spawn so individual creatures can change modality in the future.
// - `heartbeat_interval_ticks` — interval for `CreatureHeartbeat` events.
//   Note: heartbeats do NOT drive movement (that's the activation chain in
//   `sim/activation.rs`); they handle periodic non-movement checks like mood and mana.
// - `food_max` — maximum (and starting) food level. Large i64 to avoid
//   floating-point determinism issues.
// - `food_decay_per_tick` — food subtracted per sim tick, batch-applied at
//   heartbeat time as `decay_per_tick * heartbeat_interval_ticks`.
// - `food_dinner_party_join_threshold_pct` — percentage of `food_max` below
//   which a creature is willing to join an existing dinner party (default 70).
// - `food_dinner_party_organize_threshold_pct` — percentage of `food_max`
//   below which a creature is willing to organize a dinner party (default 60).
// - `food_dining_threshold_pct` — percentage of `food_max` below which an
//   idle creature will seek solo dining at a dining hall (default 40).
// - `food_hunger_threshold_pct` — percentage of `food_max` below which an
//   idle creature will eat carried food or forage (default 30).
// - `food_restore_pct` — percentage of `food_max` restored when eating
//   fruit (default 40).
// - `bread_restore_pct` — percentage of `food_max` restored when eating
//   bread from inventory (default 30). Lower than fruit since bread is
//   convenient (no travel) but less nutritious.
// - `hp_max` — maximum (and starting) hit points. Creature dies when HP
//   reaches 0. Per-species values set in `config.rs`.
// - `melee_damage` — flat damage per melee strike. 0 = species cannot melee.
// - `melee_interval_ticks` — action duration / cooldown between strikes.
// - `melee_range_sq` — max squared distance between closest footprint points.
// - `rest_max` — maximum (and starting) rest level. Same scale as `food_max`.
// - `rest_decay_per_tick` — rest subtracted per sim tick, batch-applied at
//   heartbeat. Set to 0 for species that don't need sleep.
// - `rest_tired_threshold_pct` — percentage of `rest_max` below which an
//   idle creature will seek sleep (default 50).
// - `rest_per_sleep_tick` — rest restored per sim tick of sleeping, applied
//   at each sleep task activation.
// - `mp_max` — maximum mana pool. 0 = nonmagical (cannot construct or perform
//   mana-requiring tasks). Human-scale integer (e.g., 100). WIL scales at spawn.
// - `ticks_per_hp_regen` — ticks between regenerating 1 HP. Batch-applied at
//   heartbeat with remainder accumulation, clamped to `hp_max`. 0 = no passive
//   regen (default). Trolls use this for their signature regeneration.
// - `ticks_per_mp_regen` — ticks between regenerating 1 MP. Batch-applied at
//   heartbeat with remainder accumulation and stat scaling via divisor.
//   Excess overflows 1:1 to the bonded tree.
// - `engagement_style` — `EngagementStyle` struct controlling combat behavior:
//   weapon preference (melee/ranged), engagement initiative (aggressive/
//   defensive/passive), ammo exhaustion behavior (switch to melee/flee), and
//   disengage threshold (HP% below which the creature flees). For civ creatures,
//   the military group's `engagement_style` overrides the species default.
// - `hostile_detection_range_sq` — base squared 3D euclidean distance at
//   which a creature can detect hostiles, modified at runtime by the
//   creature's Perception stat via `effective_detection_range_sq()` in
//   `sim/creature.rs`. 0 = no detection (passive species, unaffected by
//   Perception). E.g. 225 = 15-voxel radius, 400 = 20-voxel radius. Height
//   is included, so ground-level goblins cannot detect canopy elves directly.
//   See `sim/combat.rs` `should_flee()` / `flee_step()`.
// - `sex_weights` — probability weights `[None, Male, Female]` for
//   `CreatureSex` assignment at spawn. Ratio-based (any nonneg integers,
//   sum >= 1). Default `[0, 1, 1]` (equal male/female). See
//   `roll_creature_sex()` below and `types.rs::CreatureSex`.
//
// See also: `config.rs` where the species table lives as part of `GameConfig`,
// `sim/mod.rs` for the unified `Creature` type and `sim/activation.rs` for the
// activation chain that consumes this data, `types.rs` for the `Species` enum,
// `nav.rs` for `EdgeType`.
//
// **Critical constraint: determinism.** Species data is part of the game
// config and must be identical across all clients.

use crate::nav::MovementCategory;
use crate::types::TraitKind;
use serde::{Deserialize, Serialize};

/// Weapon preference for combat engagement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponPreference {
    /// Shoot at range, close to melee when out of range or no ranged option.
    PreferRanged,
    /// Close to melee distance; only shoot if no path to target exists.
    PreferMelee,
}

/// What to do when ranged ammo is exhausted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmoExhaustedBehavior {
    /// Switch to melee combat.
    SwitchToMelee,
    /// Disengage and flee.
    Flee,
}

/// How eagerly a creature initiates combat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngagementInitiative {
    /// Pursue hostiles on detection, willing to chase long distances.
    Aggressive,
    /// Counter-attack when hit and fight within ~5 voxels of the position
    /// where combat started, but don't chase beyond that tether radius.
    Defensive,
    /// Never initiate combat, never counter-attack. Completely passive.
    Passive,
}

/// Unified combat behavior struct. Replaces the former `CombatAI` enum
/// (species-level) and the former `HostileResponse` enum (military group-level).
///
/// Lives on both `SpeciesData` (species defaults for non-civ creatures)
/// and `MilitaryGroup` (player-configurable per-group overrides for civ
/// creatures). The combat decision logic (`should_flee`, `hostile_pursue`,
/// `ground_wander`, `ground_flee_step`) reads from a single resolved `EngagementStyle`.
///
/// See also: `sim/combat.rs` for the combat decision cascade,
/// `config.rs` for per-species defaults, `db.rs` for `MilitaryGroup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngagementStyle {
    /// Whether the creature prefers ranged or melee combat.
    pub weapon_preference: WeaponPreference,
    /// What to do when ranged ammo runs out.
    pub ammo_exhausted: AmmoExhaustedBehavior,
    /// How eagerly the creature initiates combat.
    pub initiative: EngagementInitiative,
    /// HP percentage (0–100) at or below which the creature rationally
    /// disengages and flees. 100 = always flee (civilian default).
    /// 0 = never disengage. Distinct from instinctual flee (panic).
    pub disengage_threshold_pct: u8,
}

impl Default for EngagementStyle {
    /// Default: passive, no combat.
    fn default() -> Self {
        Self {
            weapon_preference: WeaponPreference::PreferMelee,
            ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
            initiative: EngagementInitiative::Passive,
            disengage_threshold_pct: 0,
        }
    }
}

/// Data-driven behavioral parameters for a creature species.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeciesData {
    /// Base ticks to traverse 1.0 units of euclidean distance. Higher = slower.
    /// At 1000 ticks/sec, 500 = 2 voxels/sec. For flyers, this is the flight
    /// speed. Edge costs for non-base movement (climbing, ladders) are fixed
    /// ratios of this value determined by `movement_category`.
    pub move_ticks_per_voxel: u64,

    /// Movement mode — determines which edges the species can traverse and
    /// at what cost relative to `move_ticks_per_voxel`. Copied to the
    /// Creature DB row at spawn (per-creature, to support future modality
    /// changes). See `nav.rs` for cost ratios.
    pub movement_category: MovementCategory,

    /// Ticks between heartbeat events (mood, mana, need updates — NOT movement).
    pub heartbeat_interval_ticks: u64,

    /// Maximum food level (also the starting value). Large i64 avoids
    /// floating-point determinism concerns; UI computes percentage on the fly.
    #[serde(default = "default_food_max")]
    pub food_max: i64,

    /// Food consumed per sim tick. Batch-applied at heartbeat as
    /// `decay_per_tick * heartbeat_interval_ticks`.
    #[serde(default = "default_food_decay_per_tick")]
    pub food_decay_per_tick: i64,

    /// Food percentage below which a creature is willing to join an existing
    /// dinner party (F-dinner-party). Default 70 — hungry enough to eat
    /// socially but not urgently hungry.
    #[serde(default = "default_food_dinner_party_join_threshold_pct")]
    pub food_dinner_party_join_threshold_pct: i64,

    /// Food percentage below which a creature is willing to organize a new
    /// dinner party (F-dinner-party). Default 60 — hungrier than join
    /// threshold, since organizing takes more initiative.
    #[serde(default = "default_food_dinner_party_organize_threshold_pct")]
    pub food_dinner_party_organize_threshold_pct: i64,

    /// Food percentage below which an idle creature will seek a dining hall
    /// for solo dining. Default 40 — below the dinner party zone, so elves
    /// prefer group dining when possible.
    #[serde(default = "default_food_dining_threshold_pct")]
    pub food_dining_threshold_pct: i64,

    /// Food percentage below which an idle creature will eat carried food or
    /// forage for fruit (emergency eating). Lower than dining threshold.
    /// Default 30 — last resort before starvation.
    #[serde(default = "default_food_hunger_threshold_pct")]
    pub food_hunger_threshold_pct: i64,

    /// Percentage of food_max restored when a creature eats fruit.
    /// E.g. 40 means eating restores 40% of food_max.
    #[serde(default = "default_food_restore_pct")]
    pub food_restore_pct: i64,

    /// Percentage of food_max restored when a creature eats bread from
    /// inventory. Lower than fruit (default 30) since bread is convenient
    /// (no travel required) but less nutritious than fresh fruit.
    #[serde(default = "default_bread_restore_pct")]
    pub bread_restore_pct: i64,

    /// Spatial footprint of the creature in voxels `[width_x, height_y, depth_z]`.
    /// Default is `[1,1,1]` (single voxel). Large creatures (e.g. elephants) use
    /// `[2,2,2]` — the anchor position is the min-corner (smallest x, z) at
    /// walking level. A node at anchor `(x, y, z)` means the full footprint
    /// volume must be clear and all ground cells below must be solid.
    #[serde(default = "default_footprint")]
    pub footprint: [u8; 3],

    /// Maximum (and starting) hit points. Creature dies when HP reaches 0.
    #[serde(default = "default_hp_max")]
    pub hp_max: i64,

    /// Ticks between regenerating 1 HP. Batch-applied at heartbeat as
    /// `heartbeat_interval_ticks / ticks_per_hp_regen` (integer division),
    /// clamped to `hp_max`. 0 = no passive regeneration (default).
    #[serde(default)]
    pub ticks_per_hp_regen: u64,

    /// Flat damage dealt per melee strike. 0 means the species cannot melee.
    #[serde(default)]
    pub melee_damage: i64,

    /// Action duration / cooldown for a melee strike in ticks.
    /// At 1000 ticks/sec, 1000 = 1 second between strikes.
    #[serde(default = "default_melee_interval_ticks")]
    pub melee_interval_ticks: u64,

    /// Maximum squared distance between closest footprint points for melee.
    /// Default 2 allows face-adjacent and 2D-diagonal but not 3D-corner.
    #[serde(default = "default_melee_range_sq")]
    pub melee_range_sq: i64,

    /// Combat engagement style for this species. Determines weapon preference,
    /// initiative (aggressive/defensive/passive), ammo exhaustion behavior,
    /// and disengage threshold. For civ creatures, the military group's
    /// `engagement_style` overrides this; for non-civ creatures, this is
    /// the authoritative combat configuration.
    /// Default: passive (no combat behavior).
    #[serde(default)]
    pub engagement_style: EngagementStyle,

    /// Maximum squared 3D euclidean distance at which this creature can
    /// detect hostiles. Uses `i64` intermediates: `dx*dx + dy*dy + dz*dz`.
    /// Default 0 means no detection (passive species). For aggressive
    /// species, set to the desired detection radius squared (e.g., 225 for
    /// 15-voxel radius, 400 for 20-voxel radius).
    #[serde(default)]
    pub hostile_detection_range_sq: i64,

    /// Maximum rest level (also the starting value). Same scale as `food_max`.
    /// 0 = species never gets tired (rest system is inert).
    #[serde(default = "default_rest_max")]
    pub rest_max: i64,

    /// Rest consumed per sim tick. Batch-applied at heartbeat as
    /// `rest_decay_per_tick * heartbeat_interval_ticks`. Set to 0 for species
    /// that don't need sleep.
    #[serde(default = "default_rest_decay_per_tick")]
    pub rest_decay_per_tick: i64,

    /// Percentage of `rest_max` below which an idle creature will seek sleep.
    /// E.g. 50 means the creature gets tired at 50% rest.
    #[serde(default = "default_rest_tired_threshold_pct")]
    pub rest_tired_threshold_pct: i64,

    /// Rest restored per sim tick while sleeping. Batch-applied at each sleep
    /// activation. Higher values mean faster rest recovery per tick of sleep.
    #[serde(default = "default_rest_per_sleep_tick")]
    pub rest_per_sleep_tick: i64,

    /// Maximum mana pool for this species. 0 = nonmagical (cannot construct or
    /// perform other mana-requiring tasks). Human-scale integer (e.g., 100).
    /// WIL scales at spawn via `apply_stat_multiplier`.
    /// Current mana starts at `mp_max` on spawn.
    #[serde(default)]
    pub mp_max: i64,

    /// Ticks between regenerating 1 MP. Batch-applied at heartbeat as
    /// `heartbeat_interval / ticks_per_mp_regen` (integer division with
    /// remainder accumulation), scaled by avg(WIL, INT) via `apply_stat_divisor`.
    /// Excess beyond mp_max overflows 1:1 to the bonded tree.
    /// 0 = no natural mana generation.
    #[serde(default)]
    pub ticks_per_mp_regen: u64,

    /// Per-stat distribution parameters for rolling creature stats at spawn.
    /// Key: a stat `TraitKind` (Strength through Charisma). Value: mean and
    /// stdev for the species. Stats not present default to `(0, 50)` (human
    /// baseline with moderate variation). See `docs/drafts/creature_stats.md`.
    #[serde(default)]
    pub stat_distributions: std::collections::BTreeMap<TraitKind, StatDistribution>,

    /// Number of creatures spawned in a raid by this species. Used by the
    /// `TriggerRaid` command to size the raiding party. Default 1.
    #[serde(default = "default_raid_size")]
    pub raid_size: u32,

    /// Taming difficulty threshold (F-taming). `None` = untameable (sapient
    /// species). `Some(n)` = the tamer needs
    /// `(WIL + CHA + Beastcraft) + quasi_normal(rng, 50) >= n` to succeed
    /// on each attempt. Higher values require more stat/skill investment.
    #[serde(default)]
    pub tame_difficulty: Option<i64>,

    /// Whether this species grazes on grass. Grazers autonomously seek grassy
    /// dirt surfaces when hungry. Default false.
    #[serde(default)]
    pub is_grazer: bool,

    /// Percentage of `food_max` restored per graze action. Grazing is frequent
    /// and low-yield — a single graze restores less than eating fruit.
    /// Default 15 (15% of food_max per graze).
    #[serde(default = "default_graze_food_restore_pct")]
    pub graze_food_restore_pct: i64,

    /// Whether this species forages for wild fruit instead of grazing grass.
    /// Foragers (monkeys, squirrels) autonomously seek TreeFruit when hungry.
    /// Default false.
    #[serde(default)]
    pub is_forager: bool,

    /// Percentage of `food_max` restored when a forager eats fruit.
    /// Foraging is richer than grazing — fruit is more nutritious than grass.
    /// Default 25 (25% of food_max per fruit eaten).
    #[serde(default = "default_forage_food_restore_pct")]
    pub forage_food_restore_pct: i64,

    /// Probability weights for `CreatureSex` assignment at spawn:
    /// `[None, Male, Female]`. Ratio-based — any nonneg integers work as long
    /// as the sum is >= 1. Default `[0, 1, 1]` (equal male/female).
    #[serde(default = "default_sex_weights")]
    pub sex_weights: [u32; 3],

    /// Per-personality-axis distribution (mean, stdev), same pattern as
    /// `stat_distributions`. Axes not present default to `(0, 50)`.
    #[serde(default)]
    pub personality_distributions: PersonalityDistributions,

    /// Genome-structural config: bit widths and species-specific SNP layout.
    #[serde(default)]
    pub genome_config: SpeciesGenomeConfig,

    /// Pluralized display name for UI (e.g. "Elves", "Deer"). English
    /// pluralization is too irregular to derive from the species name.
    #[serde(default)]
    pub plural_name: String,

    /// Vertical offset in world units for centering the billboard sprite
    /// above the creature's navigation node. Species-specific because each
    /// species has a different sprite height.
    #[serde(default = "default_sprite_y_offset")]
    pub sprite_y_offset: f64,

    /// Canonical ordering for UI lists. Lower values appear first. Elf = 0.
    #[serde(default)]
    pub display_order: u32,
}

/// Species-specific distribution parameters for a single creature stat.
/// Mean is the species average (0 = human baseline); stdev controls spread.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatDistribution {
    pub mean: i32,
    pub stdev: i32,
}

/// Per-personality-axis distribution (mean, stdev), same pattern as
/// `stat_distributions`. Axes not present default to `(0, 50)`.
pub type PersonalityDistributions = std::collections::BTreeMap<PersonalityAxis, StatDistribution>;

/// The Big Five personality axes. Used as keys in personality distribution
/// config and for trait expression from the generic genome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PersonalityAxis {
    Openness,
    Conscientiousness,
    Extraversion,
    Agreeableness,
    Neuroticism,
}

/// All personality axes in genome-layout order.
pub const PERSONALITY_AXES: [PersonalityAxis; 5] = [
    PersonalityAxis::Openness,
    PersonalityAxis::Conscientiousness,
    PersonalityAxis::Extraversion,
    PersonalityAxis::Agreeableness,
    PersonalityAxis::Neuroticism,
];

/// Genome-structural config for a species. Defines bit widths and the
/// species-specific SNP layout. Stored on `SpeciesData`.
///
/// The species-specific genome layout is an ordered list of `SnpRegion`s.
/// **IMPORTANT:** ordering defines the physical bit layout. Never reorder
/// entries; only append new ones at the end.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeciesGenomeConfig {
    /// Bit-width for each ability score SNP region (default 32).
    #[serde(default = "default_stat_bits")]
    pub stat_bits: u32,

    /// Bit-width for each personality axis (default 8).
    #[serde(default = "default_personality_bits")]
    pub personality_bits: u32,

    /// Species-specific genome layout definition.
    /// IMPORTANT: ordering defines physical bit layout. Never reorder entries;
    /// only append new ones at the end.
    #[serde(default)]
    pub species_snps: Vec<SnpRegion>,
}

impl Default for SpeciesGenomeConfig {
    fn default() -> Self {
        Self {
            stat_bits: 32,
            personality_bits: 8,
            species_snps: Vec::new(),
        }
    }
}

fn default_stat_bits() -> u32 {
    32
}

fn default_personality_bits() -> u32 {
    8
}

/// A single SNP region in the species-specific genome.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnpRegion {
    /// Human-readable name, e.g. "hair_value", "hair_hue_warm".
    pub name: String,

    /// Bit-width of this region.
    pub bits: u32,

    /// How this region's bits are interpreted for trait expression.
    pub kind: SnpKind,
}

/// How a SNP region's bits are interpreted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SnpKind {
    /// Weighted-sum (ability scores) or bit-count (personality, pigmentation)
    /// scaled to species mean/stdev. The scaling model is determined by
    /// context: ability scores in the generic genome use weighted-sum;
    /// all other continuous SNP regions use bit-count.
    Continuous,

    /// Competes with other entries in the same group via max weighted sum.
    /// Categories sharing a `group` string are compared; the one with the
    /// highest weighted sum wins, with PRNG tiebreak.
    Categorical { group: String },
}

fn default_footprint() -> [u8; 3] {
    [1, 1, 1]
}

fn default_food_dinner_party_join_threshold_pct() -> i64 {
    70
}

fn default_food_dinner_party_organize_threshold_pct() -> i64 {
    60
}

fn default_food_dining_threshold_pct() -> i64 {
    40
}

fn default_food_hunger_threshold_pct() -> i64 {
    30
}

fn default_food_restore_pct() -> i64 {
    40
}

fn default_bread_restore_pct() -> i64 {
    30
}

fn default_food_max() -> i64 {
    1_000_000_000_000_000
}

fn default_food_decay_per_tick() -> i64 {
    333_333_333
}

fn default_hp_max() -> i64 {
    100
}

fn default_melee_interval_ticks() -> u64 {
    1000
}

fn default_melee_range_sq() -> i64 {
    2
}

fn default_rest_max() -> i64 {
    1_000_000_000_000_000
}

fn default_rest_decay_per_tick() -> i64 {
    333_333_333
}

fn default_rest_tired_threshold_pct() -> i64 {
    50
}

fn default_rest_per_sleep_tick() -> i64 {
    // At default heartbeat (3000 ticks), each sleep activation restores
    // rest_per_sleep_tick * 1 tick of progress. With sleep_ticks_bed = 10_000
    // activations, total restore = 10_000 * rest_per_sleep_tick.
    // We want ~10 seconds (10_000 ticks) of bed sleep to restore ~60% of rest_max.
    // 60% of 1e15 = 6e14. 6e14 / 10_000 = 6e10.
    60_000_000_000
}

fn default_raid_size() -> u32 {
    1
}

fn default_graze_food_restore_pct() -> i64 {
    15
}

fn default_forage_food_restore_pct() -> i64 {
    25
}

fn default_sex_weights() -> [u32; 3] {
    [0, 1, 1]
}

fn default_sprite_y_offset() -> f64 {
    0.48
}

/// Roll a `CreatureSex` from per-species probability weights using weighted
/// random selection. Weights are `[None, Male, Female]`.
///
/// Panics if the sum of weights is 0.
pub fn roll_creature_sex(
    weights: &[u32; 3],
    rng: &mut elven_canopy_prng::GameRng,
) -> crate::types::CreatureSex {
    use crate::types::CreatureSex;
    // Sum as u64 to avoid overflow when weights are large.
    let total: u64 = weights[0] as u64 + weights[1] as u64 + weights[2] as u64;
    assert!(total >= 1, "sex_weights sum must be >= 1");
    let roll = rng.range_u64(0, total);
    if roll < weights[0] as u64 {
        CreatureSex::None
    } else if roll < weights[0] as u64 + weights[1] as u64 {
        CreatureSex::Male
    } else {
        CreatureSex::Female
    }
}
