// Species data — Dwarf Fortress-style data-driven creature configuration.
//
// All behavioral differences between creature species (elves, capybaras, etc.)
// are expressed as data in `SpeciesData`, keyed by `Species` in the game config.
// The sim code uses a single `Creature` type and reads species-specific values
// from the species table at runtime — no code branching per species.
//
// See also: `config.rs` where the species table lives, `sim.rs` for the
// unified `Creature` type that consumes this data, `types.rs` for the
// `Species` enum.
//
// **Critical constraint: determinism.** Species data is part of the game
// config and must be identical across all clients.

use crate::nav::EdgeType;
use serde::{Deserialize, Serialize};

/// Data-driven behavioral parameters for a creature species.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeciesData {
    /// Movement speed in voxels per tick on flat surfaces.
    pub base_speed: f32,

    /// Ticks between heartbeat events (wander decisions, need updates).
    pub heartbeat_interval_ticks: u64,

    /// Edge types this species can traverse. `None` means all edges (e.g.
    /// elves can climb). `Some(vec)` restricts pathfinding to listed types
    /// (e.g. capybaras only walk on forest floor).
    pub allowed_edge_types: Option<Vec<EdgeType>>,

    /// If true, spawn at ground-level nodes and only pick ground destinations.
    pub ground_only: bool,
}
