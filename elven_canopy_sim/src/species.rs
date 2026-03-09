// Species data — Dwarf Fortress-style data-driven creature configuration.
//
// All behavioral differences between creature species (elves, capybaras, etc.)
// are expressed as data in `SpeciesData`, keyed by `Species` in the game config.
// The sim code uses a single `Creature` type and reads species-specific values
// from the species table at runtime — no code branching per species.
//
// Current parameters:
// - `walk_ticks_per_voxel` — ticks to traverse 1.0 units of euclidean distance
//   on flat ground. Higher = slower. At 1000 ticks/sec, a value of 500 means
//   the creature walks 2 voxels per second.
// - `climb_ticks_per_voxel` — ticks per 1.0 units on TrunkClimb/GroundToTrunk
//   edges. `None` means the species cannot climb (e.g. capybara). At 1000
//   ticks/sec, a value of 1250 means 0.8 voxels per second climbing.
// - `heartbeat_interval_ticks` — interval for `CreatureHeartbeat` events.
//   Note: heartbeats do NOT drive movement (that's the activation chain in
//   `sim.rs`); they handle periodic non-movement checks like mood and mana.
// - `allowed_edge_types` — restricts which nav graph edges the species can
//   traverse. `None` = all edges (elves can climb trunks and walk branches).
//   `Some(vec)` = only listed types (capybaras are ground-only).
// - `ground_only` — if true, spawning and wandering are restricted to
//   ground-level nav nodes (`ForestFloor` surface type).
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
//
// See also: `config.rs` where the species table lives as part of `GameConfig`,
// `sim.rs` for the unified `Creature` type and activation chain that consumes
// this data, `types.rs` for the `Species` enum, `nav.rs` for `EdgeType`.
//
// **Critical constraint: determinism.** Species data is part of the game
// config and must be identical across all clients.

use crate::nav::EdgeType;
use serde::{Deserialize, Serialize};

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
    3_333_333_333
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
    3_333_333_333
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
