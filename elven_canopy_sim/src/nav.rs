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

/// Heuristic scale for A*: `DIST_SCALE / sqrt(3)` rounded down.
/// Ensures Manhattan-based heuristic remains admissible when edge costs
/// are scaled by `DIST_SCALE`.
pub const HEURISTIC_SCALE: i64 = 591;

/// Compute a scaled integer Euclidean distance from coordinate deltas.
/// Returns `floor(sqrt(dx² + dy² + dz²) * DIST_SCALE)`, computed with
/// integer square root for determinism (no floats).
pub fn scaled_distance(dx: i32, dy: i32, dz: i32) -> u32 {
    let sq = (dx as i64 * dx as i64 + dy as i64 * dy as i64 + dz as i64 * dz as i64) as u64;
    let scaled_sq = sq * (DIST_SCALE as u64 * DIST_SCALE as u64);
    scaled_sq.isqrt() as u32
}

// ---------------------------------------------------------------------------
// Large-creature surface helpers (used by creature gravity)
// ---------------------------------------------------------------------------

use crate::world::VoxelWorld;

/// Find the topmost solid Y in a column using RLE spans.
/// Returns `None` if the column has no solid voxels.
fn top_solid_y_from_spans(world: &VoxelWorld, x: u32, z: u32) -> Option<u8> {
    let mut top: Option<u8> = None;
    for (vt, _y_start, y_end) in world.column_spans(x, z) {
        if vt.is_solid() {
            top = Some(y_end);
        }
    }
    top
}

/// Compute the surface Y (air voxel above ground) for a 2x2 footprint
/// anchored at `(ax, az)`. Returns `None` if any column lacks solid ground
/// or if height variation across the 4 columns exceeds 1 voxel.
///
/// Used by creature gravity (`creature.rs`) for large creatures.
pub(crate) fn large_node_surface_y(world: &VoxelWorld, ax: i32, az: i32) -> Option<i32> {
    let mut min_surface = i32::MAX;
    let mut max_surface = i32::MIN;
    for dz in 0..2 {
        for dx in 0..2 {
            let top_solid = top_solid_y_from_spans(world, (ax + dx) as u32, (az + dz) as u32);
            match top_solid {
                Some(y) => {
                    min_surface = min_surface.min(y as i32);
                    max_surface = max_surface.max(y as i32);
                }
                None => return None,
            }
        }
    }
    if max_surface - min_surface > 1 {
        return None;
    }
    Some(max_surface + 1)
}
