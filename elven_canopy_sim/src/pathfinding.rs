// Unified pathfinding for ground (nav graph) and flying (voxel grid) creatures.
//
// Ground creatures use A* on the nav graph (`astar_navgraph`) with edge-type
// filtering and species-specific traversal costs. Flying creatures use A* on
// the 3D voxel grid (`astar_fly`) with footprint clearance checks. Both return
// the same `PathResult` type.
//
// Multi-target "nearest" searches: `nearest_dijkstra_navgraph` expands outward
// from a source via Dijkstra (no heuristic) and returns the first target reached.
// `nearest_fly` runs point-to-point `astar_fly` to each candidate and returns
// the cheapest. `nearest_navgraph` is a thin wrapper that delegates to Dijkstra
// for now (F-interleaved-astar will add an A*-based strategy later).
//
// All path-finding functions take a `max_path_len` parameter — the maximum
// number of edges (hops) the resulting path may have. The search discards any
// node whose hop count from the start exceeds this limit. This is a path-length
// cutoff, not a work budget. Pass `u32::MAX` when no limit is desired.
//
// See also: `nav.rs` for the `NavGraph` being searched, `world.rs` for the
// `VoxelWorld` used by flight pathfinding, `sim/movement.rs` which consumes
// path results for step-by-step movement.
//
// **Critical constraint: determinism.** All functions are pure functions of
// their inputs — no randomness, no floats, all integer arithmetic. Nav-graph
// A* uses `VoxelCoord` tiebreaking (not `NavNodeId`) so results are independent
// of node ID assignment. Flight A* uses `BTreeMap` for visited sets (ordered by
// `VoxelCoord`) and `VoxelCoord` tiebreaking in the priority queue.

use crate::nav::{EdgeType, HEURISTIC_SCALE, NavGraph};
use crate::types::{NavEdgeId, NavNodeId, VoxelCoord};
use crate::world::VoxelWorld;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::BinaryHeap;

use crate::nav::DIST_SCALE;

/// The result of a successful pathfinding search (ground or flight).
///
/// `positions` is always populated with the voxel coordinates from start to
/// goal. For nav-graph paths, `nav_nodes` and `nav_edges` are also populated.
/// For flight paths, those fields are empty.
#[derive(Clone, Debug)]
pub struct PathResult {
    /// Voxel positions from start to goal (inclusive).
    pub positions: Vec<VoxelCoord>,
    /// Nav node IDs from start to goal (inclusive). Populated for nav-graph
    /// paths, empty for flight paths.
    pub nav_nodes: Vec<NavNodeId>,
    /// Nav edge IDs for each step (len = nav_nodes.len() - 1). Populated for
    /// nav-graph paths, empty for flight paths.
    pub nav_edges: Vec<NavEdgeId>,
    /// Total traversal cost (distance_scaled × tpv, in DIST_SCALE units).
    pub total_cost: i64,
}

/// Bundled speed parameters for nav-graph pathfinding.
///
/// Combines the four per-species ticks-per-voxel values and the optional
/// edge-type filter into a single struct to reduce argument lists. Construct
/// from `SpeciesData` (raw speeds) or `CreatureMoveSpeeds` (stat-modified).
pub struct NavGraphSpeeds<'a> {
    pub walk_tpv: u64,
    pub climb_tpv: Option<u64>,
    pub wood_ladder_tpv: Option<u64>,
    pub rope_ladder_tpv: Option<u64>,
    pub allowed_edges: Option<&'a [EdgeType]>,
}

impl<'a> NavGraphSpeeds<'a> {
    /// Build from raw species data fields.
    pub fn from_species(species_data: &'a crate::species::SpeciesData) -> Self {
        Self {
            walk_tpv: species_data.walk_ticks_per_voxel,
            climb_tpv: species_data.climb_ticks_per_voxel,
            wood_ladder_tpv: species_data.wood_ladder_tpv,
            rope_ladder_tpv: species_data.rope_ladder_tpv,
            allowed_edges: species_data.allowed_edge_types.as_deref(),
        }
    }

    /// Build from stat-modified creature move speeds plus an edge filter.
    pub fn from_move_speeds(
        speeds: &crate::stats::CreatureMoveSpeeds,
        allowed_edges: Option<&'a [EdgeType]>,
    ) -> Self {
        Self {
            walk_tpv: speeds.walk_tpv,
            climb_tpv: speeds.climb_tpv,
            wood_ladder_tpv: speeds.wood_ladder_tpv,
            rope_ladder_tpv: speeds.rope_ladder_tpv,
            allowed_edges,
        }
    }
}

// ---------------------------------------------------------------------------
// Nav-graph A* (ground creatures)
// ---------------------------------------------------------------------------

/// Entry in the A* open set (min-heap via reversed ordering).
///
/// Tiebreaker uses `VoxelCoord` ordering (not `NavNodeId`) so that
/// pathfinding results are independent of nav-graph node ID assignment.
struct OpenEntry {
    node: NavNodeId,
    f_score: i64,
    /// Cached position for deterministic tiebreaking.
    position: VoxelCoord,
}

impl PartialEq for OpenEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f_score == other.f_score && self.position == other.position
    }
}

impl Eq for OpenEntry {}

impl PartialOrd for OpenEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OpenEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed for min-heap: smallest f_score is "greatest".
        // Tiebreaker: VoxelCoord ordering (deterministic regardless of node IDs).
        other
            .f_score
            .cmp(&self.f_score)
            .then_with(|| other.position.cmp(&self.position))
    }
}

/// Find the shortest nav-graph path from `start` to `goal` using A*.
///
/// **Prefer `SimState::find_path()`** unless you are working at the nav-graph
/// level and need direct control over start/goal nodes and speed parameters.
/// `find_path` handles species dispatch, stat-modified speeds, and
/// VoxelCoord-to-NavNodeId conversion automatically.
///
/// Edge filtering is controlled by `speeds.allowed_edges` (None = all edges
/// allowed).
///
/// `max_path_len` caps the number of edges (hops) in the result path. Any
/// node whose hop count from `start` exceeds this limit is skipped. Pass
/// `u32::MAX` for no limit.
///
/// Returns `None` if no path exists, the graph is empty, or all paths exceed
/// `max_path_len`.
pub fn astar_navgraph(
    graph: &NavGraph,
    start: NavNodeId,
    goal: NavNodeId,
    speeds: &NavGraphSpeeds,
    max_path_len: u32,
) -> Option<PathResult> {
    let n = graph.node_slot_count();
    if n == 0 {
        return None;
    }
    if start == goal {
        let pos = graph.node(start).position;
        return Some(PathResult {
            positions: vec![pos],
            nav_nodes: vec![start],
            nav_edges: Vec::new(),
            total_cost: 0,
        });
    }

    let walk_tpv_i = speeds.walk_tpv as i64;

    // g_score[node] = cost of cheapest known path from start to node.
    let mut g_score = vec![i64::MAX; n];
    // came_from[node] = (previous node, edge index used to get there).
    let mut came_from: Vec<Option<(NavNodeId, NavEdgeId)>> = vec![None; n];
    let mut closed = vec![false; n];
    // depth[node] = number of edges from start to this node on the best path.
    let mut depth = vec![u32::MAX; n];

    g_score[start.0 as usize] = 0;
    depth[start.0 as usize] = 0;

    let mut open = BinaryHeap::new();
    let h_start = navgraph_heuristic(graph, start, goal, walk_tpv_i);
    open.push(OpenEntry {
        node: start,
        f_score: h_start,
        position: graph.node(start).position,
    });

    while let Some(current) = open.pop() {
        let current_id = current.node;
        let ci = current_id.0 as usize;

        if current_id == goal {
            return Some(reconstruct_navgraph_path(
                graph,
                &came_from,
                start,
                goal,
                g_score[ci],
            ));
        }

        if closed[ci] {
            continue;
        }
        closed[ci] = true;

        let current_g = g_score[ci];
        let current_depth = depth[ci];

        // If we're already at max depth, don't expand further.
        if current_depth >= max_path_len {
            continue;
        }

        for &edge_idx in graph.neighbors(current_id) {
            let edge = graph.edge(edge_idx);

            if let Some(allowed) = speeds.allowed_edges
                && !allowed.contains(&edge.edge_type)
            {
                continue;
            }

            let neighbor = edge.to;
            let ni = neighbor.0 as usize;

            if closed[ni] || !graph.is_node_alive(neighbor) {
                continue;
            }

            let tpv: i64 = match edge.edge_type {
                EdgeType::TrunkClimb | EdgeType::GroundToTrunk => match speeds.climb_tpv {
                    Some(c) => c as i64,
                    None => continue, // species cannot climb
                },
                EdgeType::WoodLadderClimb => match speeds.wood_ladder_tpv {
                    Some(c) => c as i64,
                    None => continue,
                },
                EdgeType::RopeLadderClimb => match speeds.rope_ladder_tpv {
                    Some(c) => c as i64,
                    None => continue,
                },
                _ => walk_tpv_i,
            };
            let tentative_g = current_g + edge.distance as i64 * tpv;

            if tentative_g < g_score[ni] {
                g_score[ni] = tentative_g;
                came_from[ni] = Some((current_id, edge_idx));
                depth[ni] = current_depth + 1;
                let f = tentative_g + navgraph_heuristic(graph, neighbor, goal, walk_tpv_i);
                open.push(OpenEntry {
                    node: neighbor,
                    f_score: f,
                    position: graph.node(neighbor).position,
                });
            }
        }
    }

    None // No path found.
}

/// Find the nearest reachable target from `start` using Dijkstra's algorithm.
///
/// **Prefer `SimState::find_nearest()`** unless you are working at the
/// nav-graph level and need direct control over start/target nodes and speed
/// parameters. `find_nearest` handles species dispatch, stat-modified speeds,
/// VoxelCoord-to-NavNodeId conversion, and index mapping automatically.
///
/// Expands outward from `start` by travel cost and returns the first target
/// node reached. This is a multi-target search: it stops as soon as *any*
/// target in `targets` is popped from the priority queue.
///
/// Returns `None` if no target is reachable.
pub fn nearest_dijkstra_navgraph(
    graph: &NavGraph,
    start: NavNodeId,
    targets: &[NavNodeId],
    speeds: &NavGraphSpeeds,
) -> Option<NavNodeId> {
    let n = graph.node_slot_count();
    if n == 0 || targets.is_empty() {
        return None;
    }

    // Quick check: is start already a target?
    if targets.contains(&start) {
        return Some(start);
    }

    // Build a fast lookup for targets.
    let mut is_target = vec![false; n];
    for &t in targets {
        if (t.0 as usize) < n {
            is_target[t.0 as usize] = true;
        }
    }

    let walk_tpv_i = speeds.walk_tpv as i64;

    let mut g_score = vec![i64::MAX; n];
    let mut closed = vec![false; n];

    g_score[start.0 as usize] = 0;

    let mut open = BinaryHeap::new();
    open.push(OpenEntry {
        node: start,
        f_score: 0, // Dijkstra: f = g (no heuristic).
        position: graph.node(start).position,
    });

    while let Some(current) = open.pop() {
        let current_id = current.node;
        let ci = current_id.0 as usize;

        if is_target[ci] {
            return Some(current_id);
        }

        if closed[ci] {
            continue;
        }
        closed[ci] = true;

        let current_g = g_score[ci];

        for &edge_idx in graph.neighbors(current_id) {
            let edge = graph.edge(edge_idx);

            if let Some(allowed) = speeds.allowed_edges
                && !allowed.contains(&edge.edge_type)
            {
                continue;
            }

            let neighbor = edge.to;
            let ni = neighbor.0 as usize;

            if closed[ni] || !graph.is_node_alive(neighbor) {
                continue;
            }

            let tpv: i64 = match edge.edge_type {
                EdgeType::TrunkClimb | EdgeType::GroundToTrunk => match speeds.climb_tpv {
                    Some(c) => c as i64,
                    None => continue, // species cannot climb
                },
                EdgeType::WoodLadderClimb => match speeds.wood_ladder_tpv {
                    Some(c) => c as i64,
                    None => continue,
                },
                EdgeType::RopeLadderClimb => match speeds.rope_ladder_tpv {
                    Some(c) => c as i64,
                    None => continue,
                },
                _ => walk_tpv_i,
            };
            let tentative_g = current_g + edge.distance as i64 * tpv;

            if tentative_g < g_score[ni] {
                g_score[ni] = tentative_g;
                open.push(OpenEntry {
                    node: neighbor,
                    f_score: tentative_g,
                    position: graph.node(neighbor).position,
                });
            }
        }
    }

    None // No target reachable.
}

/// Find the nearest reachable target from `start` on the nav graph.
///
/// **Prefer `SimState::find_nearest()`** unless you are working at the
/// nav-graph level and need direct control over start/target nodes and speed
/// parameters.
///
/// Currently delegates to `nearest_dijkstra_navgraph`. F-interleaved-astar
/// will later add an A*-based strategy that this wrapper can switch between.
pub fn nearest_navgraph(
    graph: &NavGraph,
    start: NavNodeId,
    targets: &[NavNodeId],
    speeds: &NavGraphSpeeds,
) -> Option<NavNodeId> {
    nearest_dijkstra_navgraph(graph, start, targets, speeds)
}

/// Admissible heuristic: Manhattan distance × walk_tpv × HEURISTIC_SCALE.
/// HEURISTIC_SCALE = DIST_SCALE / sqrt(3) ensures we never overestimate,
/// since the worst-case manhattan:euclidean ratio is sqrt(3) for 3D diagonals.
fn navgraph_heuristic(graph: &NavGraph, from: NavNodeId, to: NavNodeId, walk_tpv: i64) -> i64 {
    let a = graph.node(from).position;
    let b = graph.node(to).position;
    a.manhattan_distance(b) as i64 * walk_tpv * HEURISTIC_SCALE
}

/// Reconstruct the nav-graph path from came_from data, producing a unified
/// `PathResult` with both nav node/edge IDs and voxel positions.
fn reconstruct_navgraph_path(
    graph: &NavGraph,
    came_from: &[Option<(NavNodeId, NavEdgeId)>],
    start: NavNodeId,
    goal: NavNodeId,
    total_cost: i64,
) -> PathResult {
    let mut nav_nodes = Vec::new();
    let mut nav_edges = Vec::new();
    let mut current = goal;

    loop {
        nav_nodes.push(current);
        if current == start {
            break;
        }
        if let Some((prev, edge_idx)) = came_from[current.0 as usize] {
            nav_edges.push(edge_idx);
            current = prev;
        } else {
            break;
        }
    }

    nav_nodes.reverse();
    nav_edges.reverse();

    let positions = nav_nodes
        .iter()
        .map(|&nid| graph.node(nid).position)
        .collect();

    PathResult {
        positions,
        nav_nodes,
        nav_edges,
        total_cost,
    }
}

// ---------------------------------------------------------------------------
// Flight A* (flying creatures)
// ---------------------------------------------------------------------------

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

/// Entry in the flight A* open set (min-heap via reversed ordering).
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

/// Check whether all voxels in a footprint anchored at `anchor` are flyable.
/// The anchor is the min-corner (smallest x, y, z) of the bounding box.
pub fn footprint_flyable(world: &VoxelWorld, anchor: VoxelCoord, footprint: [u8; 3]) -> bool {
    for dx in 0..footprint[0] as i32 {
        for dy in 0..footprint[1] as i32 {
            for dz in 0..footprint[2] as i32 {
                let v = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                if !world.in_bounds(v) || !world.get(v).is_flyable() {
                    return false;
                }
            }
        }
    }
    true
}

/// Find the shortest flight path from `start` to `goal` through flyable voxels.
///
/// **Prefer `SimState::find_path()`** unless you need direct control over
/// flight parameters. `find_path` handles species dispatch, speed lookup,
/// and footprint lookup automatically.
///
/// The `footprint` `[width_x, height_y, depth_z]` specifies the creature's
/// bounding box. For 1×1×1 creatures this is `[1,1,1]`; for 2×2×2 it's
/// `[2,2,2]`. All voxels in the footprint must be flyable at every position.
///
/// `max_path_len` caps the number of voxel steps in the result path. Any
/// node whose hop count from `start` exceeds this limit is skipped. Pass
/// `u32::MAX` for no limit.
///
/// Returns `None` if no path exists (start/goal blocked, walled off, or all
/// paths exceed `max_path_len`).
pub fn astar_fly(
    world: &VoxelWorld,
    start: VoxelCoord,
    goal: VoxelCoord,
    flight_tpv: u64,
    max_path_len: u32,
    footprint: [u8; 3],
) -> Option<PathResult> {
    if !footprint_flyable(world, start, footprint) || !footprint_flyable(world, goal, footprint) {
        return None;
    }
    if start == goal {
        return Some(PathResult {
            positions: vec![start],
            nav_nodes: Vec::new(),
            nav_edges: Vec::new(),
            total_cost: 0,
        });
    }

    // g_score and came_from stored in a BTreeMap keyed by VoxelCoord
    // (deterministic iteration order, no hash-order dependence).
    let mut g_score: BTreeMap<VoxelCoord, i64> = BTreeMap::new();
    let mut came_from: BTreeMap<VoxelCoord, VoxelCoord> = BTreeMap::new();
    let mut depth: BTreeMap<VoxelCoord, u32> = BTreeMap::new();
    let mut open = BinaryHeap::new();

    g_score.insert(start, 0);
    depth.insert(start, 0);
    open.push(FlightOpenEntry {
        pos: start,
        f_score: octile_heuristic_3d(start, goal, flight_tpv),
    });

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
            return Some(PathResult {
                positions: path,
                nav_nodes: Vec::new(),
                nav_edges: Vec::new(),
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

        let current_depth = depth.get(&pos).copied().unwrap_or(0);

        // If we're already at max depth, don't expand further.
        if current_depth >= max_path_len {
            continue;
        }

        for &(dx, dy, dz, dist_scaled) in &NEIGHBOR_OFFSETS {
            let nx = pos.x + dx;
            let ny = pos.y + dy;
            let nz = pos.z + dz;
            let neighbor = VoxelCoord::new(nx, ny, nz);

            if !footprint_flyable(world, neighbor, footprint) {
                continue;
            }

            let move_cost = (dist_scaled * flight_tpv) as i64;
            let tentative_g = current_g + move_cost;

            if tentative_g < g_score.get(&neighbor).copied().unwrap_or(i64::MAX) {
                g_score.insert(neighbor, tentative_g);
                came_from.insert(neighbor, pos);
                depth.insert(neighbor, current_depth + 1);
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

/// Find the nearest reachable candidate from `start` by flight cost.
///
/// **Prefer `SimState::find_nearest()`** unless you need direct control over
/// flight parameters. `find_nearest` handles species dispatch, speed lookup,
/// footprint lookup, and index mapping automatically.
///
/// Runs point-to-point `astar_fly` to each candidate and returns the one with
/// the lowest total cost. Candidates with no reachable path are skipped.
///
/// Returns `None` if no candidate is reachable.
pub fn nearest_fly(
    world: &VoxelWorld,
    start: VoxelCoord,
    candidates: &[VoxelCoord],
    flight_tpv: u64,
    max_path_len: u32,
    footprint: [u8; 3],
) -> Option<VoxelCoord> {
    if candidates.is_empty() {
        return None;
    }

    // Quick check: is start already a candidate?
    if candidates.contains(&start) {
        return Some(start);
    }

    let mut best: Option<(VoxelCoord, i64)> = None;

    for &candidate in candidates {
        if let Some(result) =
            astar_fly(world, start, candidate, flight_tpv, max_path_len, footprint)
        {
            match best {
                None => best = Some((candidate, result.total_cost)),
                Some((_, best_cost)) if result.total_cost < best_cost => {
                    best = Some((candidate, result.total_cost));
                }
                _ => {}
            }
        }
    }

    best.map(|(coord, _)| coord)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::EdgeType;
    use crate::types::{VoxelCoord, VoxelType};
    use crate::world::VoxelWorld;

    /// Shorthand: all test nodes use Dirt surface type.
    const S: VoxelType = VoxelType::Dirt;

    /// Helper: distance in scaled units for test edges.
    const fn dist(voxels: u32) -> u32 {
        voxels * DIST_SCALE
    }

    /// Default speeds for tests: walk_tpv=1, climb_tpv=2, no ladders, all edges.
    fn test_speeds() -> NavGraphSpeeds<'static> {
        NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        }
    }

    /// Speeds with an edge filter.
    fn test_speeds_filtered(allowed: &[EdgeType]) -> NavGraphSpeeds<'_> {
        NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: Some(allowed),
        }
    }

    // -----------------------------------------------------------------------
    // astar_navgraph tests
    // -----------------------------------------------------------------------

    #[test]
    fn astar_navgraph_trivial_path() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let result = astar_navgraph(&graph, a, a, &test_speeds(), u32::MAX);
        assert!(result.is_some());
        let path = result.unwrap();
        assert_eq!(path.nav_nodes, vec![a]);
        assert_eq!(path.positions, vec![VoxelCoord::new(0, 0, 0)]);
        assert!(path.nav_edges.is_empty());
        assert_eq!(path.total_cost, 0);
    }

    #[test]
    fn astar_navgraph_simple_chain() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(5));
        graph.add_edge(b, c, EdgeType::Ground, dist(5));

        let result = astar_navgraph(&graph, a, c, &test_speeds(), u32::MAX);
        assert!(result.is_some());
        let path = result.unwrap();
        assert_eq!(path.nav_nodes, vec![a, b, c]);
        assert_eq!(path.nav_edges.len(), 2);
        assert_eq!(path.total_cost, dist(10) as i64);
        // Positions should match node positions.
        assert_eq!(
            path.positions,
            vec![
                VoxelCoord::new(0, 0, 0),
                VoxelCoord::new(5, 0, 0),
                VoxelCoord::new(10, 0, 0)
            ]
        );
    }

    #[test]
    fn astar_navgraph_chooses_shortest() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        graph.add_edge(a, c, EdgeType::Ground, dist(20));
        graph.add_edge(a, b, EdgeType::Ground, dist(3));
        graph.add_edge(b, c, EdgeType::Ground, dist(3));

        let result = astar_navgraph(&graph, a, c, &test_speeds(), u32::MAX).unwrap();
        assert_eq!(result.nav_nodes, vec![a, b, c]);
        assert_eq!(result.total_cost, dist(6) as i64);
    }

    #[test]
    fn astar_navgraph_no_path() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        let result = astar_navgraph(&graph, a, b, &test_speeds(), u32::MAX);
        assert!(result.is_none());
    }

    #[test]
    fn astar_navgraph_filtered_avoids_disallowed_edges() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(5));
        graph.add_edge(b, c, EdgeType::TrunkClimb, dist(5));

        // Only allow Ground — path a->c should fail.
        let result = astar_navgraph(
            &graph,
            a,
            c,
            &test_speeds_filtered(&[EdgeType::Ground]),
            u32::MAX,
        );
        assert!(result.is_none());

        // Allow both — should succeed.
        let result = astar_navgraph(
            &graph,
            a,
            c,
            &test_speeds_filtered(&[EdgeType::Ground, EdgeType::TrunkClimb]),
            u32::MAX,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().nav_nodes, vec![a, b, c]);
    }

    #[test]
    fn astar_navgraph_filtered_same_start_and_goal() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let result = astar_navgraph(
            &graph,
            a,
            a,
            &test_speeds_filtered(&[EdgeType::Ground]),
            u32::MAX,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().total_cost, 0);
    }

    #[test]
    fn astar_navgraph_deterministic() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(6, 0, 0), S);
        let d = graph.add_node(VoxelCoord::new(3, 3, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(3));
        graph.add_edge(b, c, EdgeType::Ground, dist(3));
        graph.add_edge(a, d, EdgeType::TrunkClimb, dist(4));
        graph.add_edge(d, c, EdgeType::TrunkClimb, dist(4));

        let speeds = NavGraphSpeeds {
            walk_tpv: 500,
            climb_tpv: Some(1250),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        let r1 = astar_navgraph(&graph, a, c, &speeds, u32::MAX).unwrap();
        let r2 = astar_navgraph(&graph, a, c, &speeds, u32::MAX).unwrap();
        assert_eq!(r1.nav_nodes, r2.nav_nodes);
        assert_eq!(r1.total_cost, r2.total_cost);
    }

    #[test]
    fn heuristic_admissible_for_3d_diagonal() {
        let tpv = 500i64;
        let actual_cost = crate::nav::scaled_distance(1, 1, 1) as i64 * tpv;
        let heuristic_cost = 3 * tpv * HEURISTIC_SCALE;
        assert!(
            heuristic_cost <= actual_cost,
            "Heuristic ({heuristic_cost}) must not exceed actual cost ({actual_cost})"
        );
    }

    #[test]
    fn astar_navgraph_tiebreaker_uses_position_not_id() {
        let pos_a = VoxelCoord::new(0, 0, 0);
        let pos_b = VoxelCoord::new(1, 0, 0);
        let pos_c = VoxelCoord::new(0, 0, 1);
        let pos_d = VoxelCoord::new(1, 0, 1);

        let mut g1 = NavGraph::new();
        let g1_a = g1.add_node(pos_a, S);
        let g1_b = g1.add_node(pos_b, S);
        let g1_c = g1.add_node(pos_c, S);
        let g1_d = g1.add_node(pos_d, S);
        g1.add_edge(g1_a, g1_b, EdgeType::Ground, dist(1));
        g1.add_edge(g1_a, g1_c, EdgeType::Ground, dist(1));
        g1.add_edge(g1_b, g1_d, EdgeType::Ground, dist(1));
        g1.add_edge(g1_c, g1_d, EdgeType::Ground, dist(1));

        let mut g2 = NavGraph::new();
        let g2_d = g2.add_node(pos_d, S);
        let g2_c = g2.add_node(pos_c, S);
        let g2_b = g2.add_node(pos_b, S);
        let g2_a = g2.add_node(pos_a, S);
        g2.add_edge(g2_a, g2_b, EdgeType::Ground, dist(1));
        g2.add_edge(g2_a, g2_c, EdgeType::Ground, dist(1));
        g2.add_edge(g2_b, g2_d, EdgeType::Ground, dist(1));
        g2.add_edge(g2_c, g2_d, EdgeType::Ground, dist(1));

        assert_ne!(g1_a, g2_a, "IDs should differ between graphs");

        let r1 = astar_navgraph(&g1, g1_a, g1_d, &test_speeds(), u32::MAX).unwrap();
        let r2 = astar_navgraph(&g2, g2_a, g2_d, &test_speeds(), u32::MAX).unwrap();

        assert_eq!(
            r1.positions, r2.positions,
            "A* paths should be identical by position regardless of node ID assignment"
        );
    }

    #[test]
    fn astar_navgraph_max_path_len_cutoff() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(1, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(2, 0, 0), S);
        let d = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(1));
        graph.add_edge(b, c, EdgeType::Ground, dist(1));
        graph.add_edge(c, d, EdgeType::Ground, dist(1));

        // Path a→d is 3 edges. max_path_len=3 should succeed.
        let result = astar_navgraph(&graph, a, d, &test_speeds(), 3);
        assert!(result.is_some());
        assert_eq!(result.unwrap().nav_nodes, vec![a, b, c, d]);

        // max_path_len=2 should fail (path requires 3 edges).
        let result = astar_navgraph(&graph, a, d, &test_speeds(), 2);
        assert!(result.is_none());

        // max_path_len=0 should fail (can't take any edges).
        let result = astar_navgraph(&graph, a, b, &test_speeds(), 0);
        assert!(result.is_none());

        // max_path_len=0 for same start/goal should succeed (0 edges needed).
        let result = astar_navgraph(&graph, a, a, &test_speeds(), 0);
        assert!(result.is_some());
    }

    // -----------------------------------------------------------------------
    // nearest_dijkstra_navgraph tests
    // -----------------------------------------------------------------------

    #[test]
    fn dijkstra_nearest_finds_closest_by_travel_cost() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(3));
        graph.add_edge(b, c, EdgeType::Ground, dist(7));

        let result = nearest_dijkstra_navgraph(&graph, a, &[b, c], &test_speeds());
        assert_eq!(result, Some(b));
    }

    #[test]
    fn dijkstra_nearest_respects_edge_filter() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(6, 0, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(3));
        graph.add_edge(b, c, EdgeType::TrunkClimb, dist(3));

        let result =
            nearest_dijkstra_navgraph(&graph, a, &[c], &test_speeds_filtered(&[EdgeType::Ground]));
        assert_eq!(result, None);

        let result =
            nearest_dijkstra_navgraph(&graph, a, &[b], &test_speeds_filtered(&[EdgeType::Ground]));
        assert_eq!(result, Some(b));
    }

    #[test]
    fn dijkstra_nearest_prefers_fast_route() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(0, 5, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(5));
        graph.add_edge(a, c, EdgeType::TrunkClimb, dist(5));

        let speeds = NavGraphSpeeds {
            walk_tpv: 500,
            climb_tpv: Some(1250),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        let result = nearest_dijkstra_navgraph(&graph, a, &[b, c], &speeds);
        assert_eq!(result, Some(b));
    }

    #[test]
    fn dijkstra_nearest_start_is_target() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let result = nearest_dijkstra_navgraph(&graph, a, &[a], &test_speeds());
        assert_eq!(result, Some(a));
    }

    #[test]
    fn dijkstra_nearest_no_targets() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let result = nearest_dijkstra_navgraph(&graph, a, &[], &test_speeds());
        assert_eq!(result, None);
    }

    #[test]
    fn dijkstra_nearest_unreachable_target() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        let result = nearest_dijkstra_navgraph(&graph, a, &[b], &test_speeds());
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // Flight pathfinding tests
    // -----------------------------------------------------------------------

    /// Create a small empty world (all air) for testing.
    fn empty_world(sx: u32, sy: u32, sz: u32) -> VoxelWorld {
        VoxelWorld::new(sx, sy, sz)
    }

    const FP1: [u8; 3] = [1, 1, 1];
    const FP2: [u8; 3] = [2, 2, 2];

    #[test]
    fn fly_same_start_and_goal() {
        let world = empty_world(16, 16, 16);
        let pos = VoxelCoord::new(5, 5, 5);
        let result = astar_fly(&world, pos, pos, 250, u32::MAX, FP1).unwrap();
        assert_eq!(result.positions, vec![pos]);
        assert!(result.nav_nodes.is_empty());
        assert!(result.nav_edges.is_empty());
        assert_eq!(result.total_cost, 0);
    }

    #[test]
    fn fly_straight_line_path() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, u32::MAX, FP1).unwrap();
        assert_eq!(result.positions.len(), 6);
        assert_eq!(result.positions[0], start);
        assert_eq!(result.positions[5], goal);
        assert_eq!(result.total_cost, 5 * 1024 * 250);
    }

    #[test]
    fn fly_diagonal_path() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 2, 2);
        let goal = VoxelCoord::new(5, 5, 5);
        let result = astar_fly(&world, start, goal, 250, u32::MAX, FP1).unwrap();
        assert_eq!(result.positions.len(), 4);
        assert_eq!(result.total_cost, 3 * 1773 * 250);
    }

    #[test]
    fn fly_blocked_by_solid() {
        let mut world = empty_world(8, 8, 8);
        for y in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(4, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(2, 4, 4);
        let goal = VoxelCoord::new(6, 4, 4);
        assert!(astar_fly(&world, start, goal, 250, u32::MAX, FP1).is_none());
    }

    #[test]
    fn fly_path_around_obstacle() {
        let mut world = empty_world(16, 16, 16);
        for y in 3..8 {
            for z in 3..8 {
                world.set(VoxelCoord::new(5, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(3, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, u32::MAX, FP1).unwrap();
        assert_eq!(*result.positions.first().unwrap(), start);
        assert_eq!(*result.positions.last().unwrap(), goal);
        assert!(result.total_cost > 0);
    }

    #[test]
    fn fly_solid_start_returns_none() {
        let mut world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        world.set(start, VoxelType::Trunk);
        let goal = VoxelCoord::new(5, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, u32::MAX, FP1).is_none());
    }

    #[test]
    fn fly_solid_goal_returns_none() {
        let mut world = empty_world(8, 8, 8);
        let goal = VoxelCoord::new(3, 3, 3);
        world.set(goal, VoxelType::Trunk);
        let start = VoxelCoord::new(5, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, u32::MAX, FP1).is_none());
    }

    #[test]
    fn fly_out_of_bounds_returns_none() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(5, 5, 5);
        let goal = VoxelCoord::new(100, 5, 5);
        assert!(astar_fly(&world, start, goal, 250, u32::MAX, FP1).is_none());
    }

    #[test]
    fn fly_max_path_len_cutoff() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);

        // Path is 5 steps. max_path_len=5 should succeed.
        let result = astar_fly(&world, start, goal, 250, 5, FP1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().positions.len(), 6);

        // max_path_len=4 should fail (path requires 5 steps).
        let result = astar_fly(&world, start, goal, 250, 4, FP1);
        assert!(result.is_none());

        // max_path_len=0 for same position should succeed.
        let result = astar_fly(&world, start, start, 250, 0, FP1);
        assert!(result.is_some());
    }

    #[test]
    fn fly_through_leaves_and_fruit() {
        let mut world = empty_world(8, 8, 8);
        world.set(VoxelCoord::new(4, 4, 4), VoxelType::Leaf);
        let start = VoxelCoord::new(3, 4, 4);
        let goal = VoxelCoord::new(5, 4, 4);
        let result = astar_fly(&world, start, goal, 250, u32::MAX, FP1).unwrap();
        assert_eq!(result.positions.len(), 3);
        assert_eq!(result.positions[1], VoxelCoord::new(4, 4, 4));
        assert_eq!(result.total_cost, 2 * 1024 * 250);

        world.set(VoxelCoord::new(4, 4, 4), VoxelType::Fruit);
        let result = astar_fly(&world, start, goal, 250, u32::MAX, FP1).unwrap();
        assert_eq!(result.positions.len(), 3);
        assert_eq!(result.positions[1], VoxelCoord::new(4, 4, 4));
    }

    #[test]
    fn fly_through_ladders() {
        let mut world = empty_world(8, 8, 8);
        world.set(VoxelCoord::new(4, 4, 4), VoxelType::WoodLadder);
        let start = VoxelCoord::new(3, 4, 4);
        let goal = VoxelCoord::new(5, 4, 4);
        let result = astar_fly(&world, start, goal, 250, u32::MAX, FP1).unwrap();
        assert_eq!(result.positions.len(), 3);
    }

    #[test]
    fn fly_footprint_2x2x2_straight_line() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(7, 5, 5);
        let result = astar_fly(&world, start, goal, 250, u32::MAX, FP2).unwrap();
        assert_eq!(result.positions.len(), 6);
        assert_eq!(result.total_cost, 5 * 1024 * 250);
    }

    #[test]
    fn fly_footprint_2x2x2_blocked_by_partial_obstruction() {
        let mut world = empty_world(16, 16, 16);
        for y in 0..16 {
            for z in 0..16 {
                world.set(VoxelCoord::new(5, y, z), VoxelType::Trunk);
            }
        }
        world.set(VoxelCoord::new(5, 5, 5), VoxelType::Air);
        let start = VoxelCoord::new(2, 5, 5);
        let goal = VoxelCoord::new(8, 5, 5);

        let result_1x1 = astar_fly(&world, start, goal, 250, u32::MAX, FP1);
        assert!(result_1x1.is_some(), "1x1x1 should find path through gap");

        assert!(
            astar_fly(&world, start, goal, 250, u32::MAX, FP2).is_none(),
            "2x2x2 should not fit through 1-voxel gap"
        );
    }

    #[test]
    fn fly_footprint_2x2x2_blocked_at_start() {
        let mut world = empty_world(8, 8, 8);
        world.set(VoxelCoord::new(3, 4, 3), VoxelType::Trunk);
        let start = VoxelCoord::new(2, 3, 2);
        let goal = VoxelCoord::new(5, 3, 5);
        assert!(astar_fly(&world, start, goal, 250, u32::MAX, FP2).is_none());
    }

    #[test]
    fn fly_footprint_2x2x2_at_world_boundary() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(7, 3, 3);
        let goal = VoxelCoord::new(5, 3, 3);
        assert!(
            astar_fly(&world, start, goal, 250, u32::MAX, FP2).is_none(),
            "2x2x2 at world boundary should not start (footprint out of bounds)"
        );
        let start2 = VoxelCoord::new(3, 3, 3);
        let goal2 = VoxelCoord::new(7, 3, 3);
        assert!(
            astar_fly(&world, start2, goal2, 250, u32::MAX, FP2).is_none(),
            "2x2x2 at world boundary should not reach goal (footprint out of bounds)"
        );
        let result = astar_fly(
            &world,
            VoxelCoord::new(3, 3, 3),
            VoxelCoord::new(7, 3, 3),
            250,
            u32::MAX,
            FP1,
        );
        assert!(result.is_some(), "1x1x1 should reach world boundary");
    }

    // -----------------------------------------------------------------------
    // nearest_fly tests
    // -----------------------------------------------------------------------

    #[test]
    fn nearest_fly_finds_closest() {
        let world = empty_world(16, 16, 16);
        let start = VoxelCoord::new(5, 5, 5);
        let near = VoxelCoord::new(7, 5, 5); // 2 steps
        let far = VoxelCoord::new(12, 5, 5); // 7 steps

        let result = nearest_fly(&world, start, &[near, far], 250, u32::MAX, FP1);
        assert_eq!(result, Some(near));
    }

    #[test]
    fn nearest_fly_start_is_candidate() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let result = nearest_fly(&world, start, &[start], 250, u32::MAX, FP1);
        assert_eq!(result, Some(start));
    }

    #[test]
    fn nearest_fly_no_candidates() {
        let world = empty_world(8, 8, 8);
        let start = VoxelCoord::new(3, 3, 3);
        let result = nearest_fly(&world, start, &[], 250, u32::MAX, FP1);
        assert_eq!(result, None);
    }

    #[test]
    fn nearest_fly_unreachable_candidate() {
        let mut world = empty_world(8, 8, 8);
        // Full wall blocking.
        for y in 0..8 {
            for z in 0..8 {
                world.set(VoxelCoord::new(4, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(2, 4, 4);
        let goal = VoxelCoord::new(6, 4, 4);
        let result = nearest_fly(&world, start, &[goal], 250, u32::MAX, FP1);
        assert_eq!(result, None);
    }

    #[test]
    fn nearest_fly_skips_unreachable_picks_reachable() {
        let mut world = empty_world(16, 16, 16);
        // Wall blocking access to one candidate.
        for y in 0..16 {
            for z in 0..16 {
                world.set(VoxelCoord::new(10, y, z), VoxelType::Trunk);
            }
        }
        let start = VoxelCoord::new(5, 5, 5);
        let blocked = VoxelCoord::new(12, 5, 5);
        let reachable = VoxelCoord::new(7, 5, 5);

        let result = nearest_fly(&world, start, &[blocked, reachable], 250, u32::MAX, FP1);
        assert_eq!(result, Some(reachable));
    }

    // -----------------------------------------------------------------------
    // nearest_navgraph tests
    // -----------------------------------------------------------------------

    #[test]
    fn nearest_navgraph_delegates_to_dijkstra() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        graph.add_edge(a, b, EdgeType::Ground, dist(3));
        graph.add_edge(b, c, EdgeType::Ground, dist(7));

        // nearest_navgraph should find closest target (b).
        let result = nearest_navgraph(&graph, a, &[b, c], &test_speeds());
        assert_eq!(result, Some(b));
    }

    // -----------------------------------------------------------------------
    // Edge type / TPV tests
    // -----------------------------------------------------------------------

    #[test]
    fn astar_navgraph_climb_tpv_none_blocks_climb_edges() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(0, 5, 0), S);
        graph.add_edge(a, b, EdgeType::TrunkClimb, dist(5));

        // climb_tpv = None → climb edges impassable.
        let no_climb = NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: None,
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        assert!(astar_navgraph(&graph, a, b, &no_climb, u32::MAX).is_none());

        // climb_tpv = Some → should find path.
        let with_climb = NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        assert!(astar_navgraph(&graph, a, b, &with_climb, u32::MAX).is_some());
    }

    #[test]
    fn astar_navgraph_ground_to_trunk_blocked_by_climb_none() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(0, 1, 0), S);
        graph.add_edge(a, b, EdgeType::GroundToTrunk, dist(1));

        let no_climb = NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: None,
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        assert!(astar_navgraph(&graph, a, b, &no_climb, u32::MAX).is_none());
    }

    #[test]
    fn astar_navgraph_wood_ladder_tpv() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(0, 5, 0), S);
        graph.add_edge(a, b, EdgeType::WoodLadderClimb, dist(5));

        // wood_ladder_tpv = None → ladder impassable.
        let no_ladder = NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        assert!(astar_navgraph(&graph, a, b, &no_ladder, u32::MAX).is_none());

        // wood_ladder_tpv = Some(3) → should find path with cost 5 * 3 * DIST_SCALE.
        let with_ladder = NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: Some(3),
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        let result = astar_navgraph(&graph, a, b, &with_ladder, u32::MAX).unwrap();
        assert_eq!(result.total_cost, dist(5) as i64 * 3);
    }

    #[test]
    fn astar_navgraph_rope_ladder_tpv() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(0, 5, 0), S);
        graph.add_edge(a, b, EdgeType::RopeLadderClimb, dist(5));

        // rope_ladder_tpv = None → impassable.
        let no_ladder = NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: None,
            allowed_edges: None,
        };
        assert!(astar_navgraph(&graph, a, b, &no_ladder, u32::MAX).is_none());

        // rope_ladder_tpv = Some(4) → should find path.
        let with_ladder = NavGraphSpeeds {
            walk_tpv: 1,
            climb_tpv: Some(2),
            wood_ladder_tpv: None,
            rope_ladder_tpv: Some(4),
            allowed_edges: None,
        };
        let result = astar_navgraph(&graph, a, b, &with_ladder, u32::MAX).unwrap();
        assert_eq!(result.total_cost, dist(5) as i64 * 4);
    }
}
