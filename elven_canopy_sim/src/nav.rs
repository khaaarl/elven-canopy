// Navigation constants and edge types shared by pathfinding and walkability.
//
// This module was formerly the full navigation graph implementation. After the
// migration to voxel-direct A* (F-remove-navgraph), the graph structures were
// removed and only the shared types remain: `EdgeType` for edge classification,
// distance constants, and `scaled_distance()` for integer-only Euclidean
// distance computation.
//
// See also: `pathfinding.rs` for A* search, `walkability.rs` for walkable
// position queries.

/// The type of connection between two nav nodes.
/// Serializable because it appears in species config (`allowed_edge_types`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EdgeType {
    /// Walking on the ground (dirt terrain) around the trunk base.
    #[serde(alias = "ForestFloor")]
    Ground,
    /// Climbing up/down the raw trunk surface.
    TrunkClimb,
    /// Walking along a branch.
    BranchWalk,
    /// Circumferential movement around the trunk at one y-level.
    TrunkCircumference,
    /// Connecting ground-level nodes to trunk surface nodes.
    GroundToTrunk,
    /// Climbing a wood ladder.
    WoodLadderClimb,
    /// Climbing a rope ladder.
    RopeLadderClimb,
}

/// Scale factor for integer edge distances. Euclidean voxel distances are
/// multiplied by this value so that irrational lengths (sqrt(2), sqrt(3))
/// are represented without floats. A power of two for cheap multiply/divide.
pub const DIST_SCALE: u32 = 1024;

/// Compute a scaled integer Euclidean distance from coordinate deltas.
/// Returns `floor(sqrt(dx² + dy² + dz²) * DIST_SCALE)`, computed with
/// integer square root for determinism (no floats).
pub fn scaled_distance(dx: i32, dy: i32, dz: i32) -> u32 {
    let sq = (dx as i64 * dx as i64 + dy as i64 * dy as i64 + dz as i64 * dz as i64) as u64;
    let scaled_sq = sq * (DIST_SCALE as u64 * DIST_SCALE as u64);
    scaled_sq.isqrt() as u32
}

// Large-creature surface helpers have been moved to `walkability.rs`
// (`large_node_surface_y`, `top_solid_y_from_spans`) where they are used
// by `ground_neighbors` and `creature.rs`.
