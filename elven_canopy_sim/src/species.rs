// Species data — Dwarf Fortress-style data-driven creature configuration.
//
// Most behavioral differences between creature species (elves, capybaras, etc.)
// are expressed as data in `SpeciesData`, keyed by `Species` in the game config.
// The sim code uses a single `Creature` type and reads species-specific values
// from the species table at runtime, including `engagement_style` for combat
// behavior and `hostile_detection_range_sq` for detection range.
//
// Current parameters:
// - `walk_ticks_per_voxel` — ticks to traverse 1.0 units of euclidean distance
//   on flat ground. Higher = slower. At 1000 ticks/sec, a value of 500 means
//   the creature walks 2 voxels per second.
// - `climb_ticks_per_voxel` — ticks per 1.0 units on TrunkClimb/GroundToTrunk
//   edges. `None` means the species cannot climb (e.g. capybara). At 1000
//   ticks/sec, a value of 1250 means 0.8 voxels per second climbing.
// - `flight_ticks_per_voxel` — ticks per 1.0 units of 3D flight. `None`
//   means the species cannot fly. Flying creatures use vanilla A* on the
//   voxel grid (`flight_pathfinding.rs`) instead of the nav graph.
// - `heartbeat_interval_ticks` — interval for `CreatureHeartbeat` events.
//   Note: heartbeats do NOT drive movement (that's the activation chain in
//   `sim/activation.rs`); they handle periodic non-movement checks like mood and mana.
// - `allowed_edge_types` — restricts which nav graph edges the species can
//   traverse. `None` = all edges (elves can climb trunks and walk branches).
//   `Some(vec)` = only listed types (capybaras are ground-only).
// - `ground_only` — if true, spawning and wandering are restricted to
//   ground-level nav nodes (`Dirt` surface type).
// - `food_max` — maximum (and starting) food level. Large i64 to avoid
//   floating-point determinism issues.
// - `food_decay_per_tick` — food subtracted per sim tick, batch-applied at
//   heartbeat time as `decay_per_tick * heartbeat_interval_ticks`.
// - `food_hunger_threshold_pct` — percentage of `food_max` below which an
//   idle creature will seek fruit (default 50).
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
//   mana-requiring tasks). Uses the same large-integer scale as `food_max`.
// - `ticks_per_hp_regen` — ticks between regenerating 1 HP. Batch-applied at
//   heartbeat as `heartbeat_interval_ticks / ticks_per_hp_regen` HP (integer
//   division), clamped to `hp_max`. 0 = no passive regen (default). Trolls use
//   this for their signature regeneration.
// - `mana_per_tick` — mana generated per sim tick. Batch-applied at heartbeat.
//   Excess overflows to the bonded tree.
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
//
// See also: `config.rs` where the species table lives as part of `GameConfig`,
// `sim/mod.rs` for the unified `Creature` type and `sim/activation.rs` for the
// activation chain that consumes this data, `types.rs` for the `Species` enum,
// `nav.rs` for `EdgeType`.
//
// **Critical constraint: determinism.** Species data is part of the game
// config and must be identical across all clients.

use crate::nav::EdgeType;
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
    /// Ticks to traverse 1.0 units of euclidean distance on flat ground.
    /// Higher = slower. At 1000 ticks/sec, 500 = 2 voxels/sec.
    pub walk_ticks_per_voxel: u64,

    /// Ticks per 1.0 units on TrunkClimb/GroundToTrunk edges. `None` means
    /// the species cannot climb (e.g. capybara). At 1000 ticks/sec, 1250 =
    /// 0.8 voxels/sec climbing.
    pub climb_ticks_per_voxel: Option<u64>,

    /// Ticks per 1.0 units of 3D flight movement. `None` means the species
    /// cannot fly. Flying creatures use vanilla A* on the voxel grid instead
    /// of the nav graph. At 1000 ticks/sec, 250 = 4 voxels/sec flying.
    #[serde(default)]
    pub flight_ticks_per_voxel: Option<u64>,

    /// Ticks between heartbeat events (mood, mana, need updates — NOT movement).
    pub heartbeat_interval_ticks: u64,

    /// Edge types this species can traverse. `None` means all edges (e.g.
    /// elves can climb). `Some(vec)` restricts pathfinding to listed types
    /// (e.g. capybaras only walk on forest floor).
    pub allowed_edge_types: Option<Vec<EdgeType>>,

    /// If true, spawn at ground-level nodes and only pick ground destinations.
    pub ground_only: bool,

    /// Maximum food level (also the starting value). Large i64 avoids
    /// floating-point determinism concerns; UI computes percentage on the fly.
    #[serde(default = "default_food_max")]
    pub food_max: i64,

    /// Food consumed per sim tick. Batch-applied at heartbeat as
    /// `decay_per_tick * heartbeat_interval_ticks`.
    #[serde(default = "default_food_decay_per_tick")]
    pub food_decay_per_tick: i64,

    /// Food percentage below which an idle creature will seek fruit.
    /// E.g. 50 means the creature gets hungry at 50% food.
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

    /// Ticks per 1.0 units on WoodLadderClimb edges. `None` means the species
    /// cannot use wood ladders (e.g. capybara, elephant).
    #[serde(default)]
    pub wood_ladder_tpv: Option<u64>,

    /// Ticks per 1.0 units on RopeLadderClimb edges. `None` means the species
    /// cannot use rope ladders.
    #[serde(default)]
    pub rope_ladder_tpv: Option<u64>,

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
    /// perform other mana-requiring tasks). Uses the same large-integer scale
    /// as `food_max` / `rest_max` to avoid floating-point determinism issues.
    /// Current mana starts at `mp_max` on spawn.
    #[serde(default)]
    pub mp_max: i64,

    /// Mana generated per sim tick. Batch-applied at heartbeat as
    /// `mana_per_tick * heartbeat_interval_ticks`, then excess overflows to the
    /// bonded tree. 0 = no natural mana generation.
    #[serde(default)]
    pub mana_per_tick: i64,

    /// Per-stat distribution parameters for rolling creature stats at spawn.
    /// Key: a stat `TraitKind` (Strength through Charisma). Value: mean and
    /// stdev for the species. Stats not present default to `(0, 5)` (human
    /// baseline with moderate variation). See `docs/drafts/creature_stats.md`.
    #[serde(default)]
    pub stat_distributions: std::collections::BTreeMap<TraitKind, StatDistribution>,

    /// Number of creatures spawned in a raid by this species. Used by the
    /// `TriggerRaid` command to size the raiding party. Default 1.
    #[serde(default = "default_raid_size")]
    pub raid_size: u32,
}

/// Species-specific distribution parameters for a single creature stat.
/// Mean is the species average (0 = human baseline); stdev controls spread.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatDistribution {
    pub mean: i32,
    pub stdev: i32,
}

fn default_footprint() -> [u8; 3] {
    [1, 1, 1]
}

fn default_food_hunger_threshold_pct() -> i64 {
    50
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
