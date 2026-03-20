// Vanilla A* pathfinding for flying creatures over the voxel grid.
//
// Flying creatures move through any non-solid voxel in 3D space. This is
// completely separate from the nav graph used by ground creatures — the sky
// is mostly open, so a simple grid-based A* is sufficient.
//
// 26-connected neighbors (all 3D diagonals allowed). Costs use the same
// DIST_SCALE (1024) integer scaling as `nav.rs` / `pathfinding.rs` for
// consistency. Heuristic: 3D octile distance (admissible, consistent).
//
// See also: `pathfinding.rs` (nav-graph A* for ground creatures),
// `world.rs` (VoxelWorld providing `get()` and `in_bounds()`),
// `sim/movement.rs` which consumes flight paths for step-by-step movement.
//
// **Critical constraint: determinism.** Integer-only arithmetic, BTreeMap for
// visited set (ordered by VoxelCoord), BinaryHeap with VoxelCoord tiebreaking.

use crate::nav::DIST_SCALE;
use crate::types::VoxelCoord;
use crate::world::VoxelWorld;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::BinaryHeap;

/// Precomputed scaled distances for the 26 neighbor offsets.
/// Face-adjacent (6): distance = DIST_SCALE (1024)
/// Edge-diagonal (12): distance = floor(sqrt(2) * 1024) = 1448
/// Corner-diagonal (8): distance = floor(sqrt(3) * 1024) = 1773
const NEIGHBOR_OFFSETS: [(i32, i32, i32, u64); 26] = [
    // Face-adjacent (6)
    (-1, 0, 0, 1024),
    (1, 0, 0, 1024),
    (0, -1, 0, 1024),
    (0, 1, 0, 1024),
    (0, 0, -1, 1024),
    (0, 0, 1, 1024),
    // Edge-diagonal (12)
    (-1, -1, 0, 1448),
    (-1, 1, 0, 1448),
    (1, -1, 0, 1448),
    (1, 1, 0, 1448),
    (-1, 0, -1, 1448),
    (-1, 0, 1, 1448),
    (1, 0, -1, 1448),
    (1, 0, 1, 1448),
    (0, -1, -1, 1448),
    (0, -1, 1, 1448),
    (0, 1, -1, 1448),
    (0, 1, 1, 1448),
    // Corner-diagonal (8)
    (-1, -1, -1, 1773),
    (-1, -1, 1, 1773),
    (-1, 1, -1, 1773),
    (-1, 1, 1, 1773),
    (1, -1, -1, 1773),
    (1, -1, 1, 1773),
    (1, 1, -1, 1773),
    (1, 1, 1, 1773),
];

/// Result of a successful flight A* search.
#[derive(Clone, Debug)]
pub struct FlightPathResult {
    /// Sequence of voxel coordinates from start to goal (inclusive).
    pub waypoints: Vec<VoxelCoord>,
    /// Total traversal cost (distance_scaled × flight_tpv).
    pub total_cost: i64,
}

/// Entry in the A* open set (min-heap via reversed ordering).
struct FlightOpenEntry {
    pos: VoxelCoord,
    f_score: i64,
}

impl PartialEq for FlightOpenEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f_score == other.f_score && self.pos == other.pos
    }
}

impl Eq for FlightOpenEntry {}

impl PartialOrd for FlightOpenEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FlightOpenEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed for min-heap: smallest f_score is "greatest".
        // Tiebreaker: VoxelCoord ordering for determinism.
        other
            .f_score
            .cmp(&self.f_score)
            .then_with(|| other.pos.cmp(&self.pos))
    }
}

/// 3D octile heuristic (admissible and consistent for 26-connected grids).
///
/// Octile distance in 3D: let d1 ≤ d2 ≤ d3 be the sorted absolute deltas.
/// Cost = d1 * corner_dist + (d2 - d1) * edge_dist + (d3 - d2) * face_dist,
/// where face_dist = DIST_SCALE, edge_dist = 1448, corner_dist = 1773.
///
/// Multiplied by `flight_tpv` to match the edge cost scale.
fn octile_heuristic_3d(a: VoxelCoord, b: VoxelCoord, flight_tpv: u64) -> i64 {
    let dx = (a.x - b.x).unsigned_abs();
    let dy = (a.y - b.y).unsigned_abs();
    let dz = (a.z - b.z).unsigned_abs();

    // Sort deltas: d1 ≤ d2 ≤ d3.
    let (mut d1, mut d2, mut d3) = (dx, dy, dz);
    if d1 > d2 {
        std::mem::swap(&mut d1, &mut d2);
    }
    if d2 > d3 {
        std::mem::swap(&mut d2, &mut d3);
    }
    if d1 > d2 {
        std::mem::swap(&mut d1, &mut d2);
    }

    let face: u64 = DIST_SCALE as u64; // 1024
    let edge: u64 = 1448; // floor(sqrt(2) * 1024)
    let corner: u64 = 1773; // floor(sqrt(3) * 1024)

    let h = d1 as u64 * corner + (d2 - d1) as u64 * edge + (d3 - d2) as u64 * face;

    (h * flight_tpv) as i64
}

/// Find the shortest flight path from `start` to `goal` through non-solid voxels.
///
/// Returns `None` if no path exists (start/goal are solid, or walled off).
/// `flight_tpv` is the species' flight ticks-per-voxel from `SpeciesData`.
///
/// The search limit `max_nodes` caps the number of nodes expanded to prevent
/// runaway searches in very open worlds. 0 means unlimited.
pub fn astar_fly(
    world: &VoxelWorld,
    start: VoxelCoord,
    goal: VoxelCoord,
    flight_tpv: u64,
    max_nodes: u32,
) -> Option<FlightPathResult> {
    if !world.in_bounds(start) || !world.in_bounds(goal) {
        return None;
    }
    if !world.get(start).is_flyable() || !world.get(goal).is_flyable() {
        return None;
    }
    if start == goal {
        return Some(FlightPathResult {
            waypoints: vec![start],
            total_cost: 0,
        });
    }

    // g_score and came_from stored in a BTreeMap keyed by VoxelCoord
    // (deterministic iteration order, no hash-order dependence).
    let mut g_score: BTreeMap<VoxelCoord, i64> = BTreeMap::new();
    let mut came_from: BTreeMap<VoxelCoord, VoxelCoord> = BTreeMap::new();
    let mut open = BinaryHeap::new();

    g_score.insert(start, 0);
    open.push(FlightOpenEntry {
        pos: start,
        f_score: octile_heuristic_3d(start, goal, flight_tpv),
    });

    let mut expanded: u32 = 0;

    while let Some(FlightOpenEntry { pos, f_score }) = open.pop() {
        if pos == goal {
            // Reconstruct path.
            let mut path = vec![goal];
            let mut current = goal;
            while let Some(&prev) = came_from.get(&current) {
                path.push(prev);
                current = prev;
            }
            path.reverse();
            let total_cost = g_score[&goal];
            return Some(FlightPathResult {
                waypoints: path,
                total_cost,
            });
        }

        let current_g = match g_score.get(&pos) {
            Some(&g) => g,
            None => continue,
        };

        // Skip stale entries: this node was re-pushed with a better g_score,
        // and the better entry has already been (or will be) expanded.
        let h = octile_heuristic_3d(pos, goal, flight_tpv);
        if current_g + h < f_score {
            continue;
        }

        expanded += 1;
        if max_nodes > 0 && expanded > max_nodes {
            return None; // search budget exhausted
        }

        for &(dx, dy, dz, dist_scaled) in &NEIGHBOR_OFFSETS {
            let nx = pos.x + dx;
            let ny = pos.y + dy;
            let nz = pos.z + dz;
            let neighbor = VoxelCoord::new(nx, ny, nz);

            if !world.in_bounds(neighbor) {
                continue;
            }
            if !world.get(neighbor).is_flyable() {
                continue;
            }

            let move_cost = (dist_scaled * flight_tpv) as i64;
            let tentative_g = current_g + move_cost;

            if tentative_g < g_score.get(&neighbor).copied().unwrap_or(i64::MAX) {
                g_score.insert(neighbor, tentative_g);
                came_from.insert(neighbor, pos);
                let h = octile_heuristic_3d(neighbor, goal, flight_tpv);
                open.push(FlightOpenEntry {
                    pos: neighbor,
                    f_score: tentative_g + h,
                });
            }
        }
    }

    None // no path found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::VoxelCoord;
    use crate::world::VoxelWorld;

    /// Create a small empty world (all air) for testing.
    fn empty_world(sx: u32, sy: u32, sz: u32) -> VoxelWorld {
        VoxelWorld::new(sx, sy, sz)
    }

    #[test]
    fn same_start_and_goal() {
        let world = empty_world(16, 16, 16);
        let pos = VoxelCoord::new(5, 5, 5);
        let result = astar_fly(&world, pos, pos, 250, 0).unwrap();
        assert_eq!(result.waypoints, vec![pos]);
        assert_eq!(result.total_cost, 0);
    }

    #[test]
    fn straight_line_path() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, 0).unwrap();
        // Straight line: 5 steps, each face-adjacent (1024 * 250 = 256000 cost per step).
        assert_eq!(result.waypoints.len(), 6); // 5 edges + start
        assert_eq!(result.waypoints[0], start);
        assert_eq!(result.waypoints[5], goal);
        assert_eq!(result.total_cost, 5 * 1024 * 250);
    }

    #[test]
    fn diagonal_path() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 2, 2);
        let goal = VoxelCoord::new(5, 5, 5);
        let result = astar_fly(&world, start, goal, 250, 0).unwrap();
        // Pure 3D diagonal: 3 corner steps (distance 1773 each).
        assert_eq!(result.waypoints.len(), 4);
        assert_eq!(result.total_cost, 3 * 1773 * 250);
    }

    #[test]
    fn blocked_by_solid() {
        let mut world = empty_world(8, 8, 8);
        // Build a solid wall at x=4 blocking all y,z in range.
        for y in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(4, y, z), crate::types::VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(2, 4, 4);
        let goal = VoxelCoord::new(6, 4, 4);
        // Fully walled off — no path.
        assert!(astar_fly(&world, start, goal, 250, 0).is_none());
    }

    #[test]
    fn path_around_obstacle() {
        let mut world = empty_world(16, 16, 16);
        // Partial wall at x=5 blocking y=3..7, z=3..7 — leaves gap at y=8.
        for y in 3..8 {
            for z in 3..8 {
                world.set(VoxelCoord::new(5, y, z), crate::types::VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(3, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, 0).unwrap();
        // Path exists (goes around the wall).
        assert_eq!(*result.waypoints.first().unwrap(), start);
        assert_eq!(*result.waypoints.last().unwrap(), goal);
        assert!(result.total_cost > 0);
    }

    #[test]
    fn solid_start_returns_none() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        world.set(start, crate::types::VoxelType::Trunk);
        let goal = VoxelCoord::new(5, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, 0).is_none());
    }

    #[test]
    fn solid_goal_returns_none() {
        let mut world = empty_world(8, 8, 8);
        let goal = VoxelCoord::new(3, 3, 3);
        world.set(goal, crate::types::VoxelType::Trunk);
        let start = VoxelCoord::new(5, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, 0).is_none());
    }

    #[test]
    fn out_of_bounds_returns_none() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(5, 5, 5);
        let goal = VoxelCoord::new(100, 5, 5); // out of bounds
        assert!(astar_fly(&world, start, goal, 250, 0).is_none());
    }

    #[test]
    fn max_nodes_budget() {
        let world = empty_world(64, 64, 64);
        let start = VoxelCoord::new(0, 32, 0);
        let goal = VoxelCoord::new(63, 32, 63);
        // Very tight budget — should fail.
        assert!(astar_fly(&world, start, goal, 250, 5).is_none());
        // Generous budget — should succeed.
        let result = astar_fly(&world, start, goal, 250, 100_000);
        assert!(result.is_some());
    }

    #[test]
    fn flyable_through_leaves_and_fruit() {
        let mut world = empty_world(8, 8, 8);
        // Place a Leaf voxel on the direct path. Flying creatures should
        // route straight through it (Leaf is solid but flyable).
        world.set(VoxelCoord::new(4, 4, 4), crate::types::VoxelType::Leaf);
        let start = VoxelCoord::new(3, 4, 4);
        let goal = VoxelCoord::new(5, 4, 4);
        let result = astar_fly(&world, start, goal, 250, 0).unwrap();
        assert_eq!(result.waypoints.len(), 3);
        assert_eq!(result.waypoints[1], VoxelCoord::new(4, 4, 4));
        assert_eq!(result.total_cost, 2 * 1024 * 250);

        // Same for Fruit.
        world.set(VoxelCoord::new(4, 4, 4), crate::types::VoxelType::Fruit);
        let result = astar_fly(&world, start, goal, 250, 0).unwrap();
        assert_eq!(result.waypoints.len(), 3);
        assert_eq!(result.waypoints[1], VoxelCoord::new(4, 4, 4));
    }

    #[test]
    fn flyable_through_ladders() {
        let mut world = empty_world(8, 8, 8);
        // WoodLadder and RopeLadder are non-solid and flyable.
        world.set(
            VoxelCoord::new(4, 4, 4),
            crate::types::VoxelType::WoodLadder,
        );
        let start = VoxelCoord::new(3, 4, 4);
        let goal = VoxelCoord::new(5, 4, 4);
        let result = astar_fly(&world, start, goal, 250, 0).unwrap();
        assert_eq!(result.waypoints.len(), 3);
    }
}
