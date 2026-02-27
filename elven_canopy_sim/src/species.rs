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
}

fn default_food_max() -> i64 {
    1_000_000_000_000_000
}

fn default_food_decay_per_tick() -> i64 {
    3_333_333_333
}
