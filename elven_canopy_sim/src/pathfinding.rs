// A* pathfinding over the navigation graph.
//
// Implements standard A* search using a `BinaryHeap` (min-heap via reversed
// ordering, same pattern as `EventQueue`). Node scores and came-from data
// are stored in `Vec`s indexed by `NavNodeId` for O(1) access and
// deterministic behavior (no `HashMap`).
//
// Edge costs are species-dependent: `edge.distance * ticks_per_voxel`, where
// `ticks_per_voxel` is `walk_ticks_per_voxel` for flat edges and
// `climb_ticks_per_voxel` for TrunkClimb/GroundToTrunk edges (from
// `species.rs`). The heuristic uses `manhattan_distance * walk_ticks_per_voxel`
// — the fastest speed — to remain admissible.
//
// See also: `nav.rs` for the `NavGraph` being searched, `sim.rs` which
// calls pathfinding during creature AI decisions.
//
// **Critical constraint: determinism.** A* is a pure function of graph
// state and start/goal nodes. No randomness, no floating-point ambiguity
// beyond basic f32 arithmetic with `total_cmp` for ordering.

use crate::nav::{EdgeType, NavGraph};
use crate::types::NavNodeId;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// The result of a successful A* search.
#[derive(Clone, Debug)]
pub struct PathResult {
    /// Sequence of node IDs from start to goal (inclusive).
    pub nodes: Vec<NavNodeId>,
    /// Indices into `NavGraph.edges` for each step (len = nodes.len() - 1).
    pub edge_indices: Vec<usize>,
    /// Total traversal cost.
    pub total_cost: f32,
}

/// Entry in the A* open set (min-heap via reversed ordering).
struct OpenEntry {
    node: NavNodeId,
    f_score: f32,
}

impl PartialEq for OpenEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f_score.total_cmp(&other.f_score) == Ordering::Equal && self.node == other.node
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
        other
            .f_score
            .total_cmp(&self.f_score)
            .then_with(|| other.node.0.cmp(&self.node.0))
    }
}

/// Find the shortest path from `start` to `goal` using A*.
///
/// Returns `None` if no path exists or if the graph is empty.
/// `walk_tpv` and `climb_tpv` are the species' ticks-per-voxel for flat and
/// climb edges respectively. `climb_tpv = None` means climb edges are
/// impassable (species cannot climb).
pub fn astar(
    graph: &NavGraph,
    start: NavNodeId,
    goal: NavNodeId,
    walk_tpv: u64,
    climb_tpv: Option<u64>,
) -> Option<PathResult> {
    let n = graph.node_slot_count();
    if n == 0 {
        return None;
    }
    if start == goal {
        return Some(PathResult {
            nodes: vec![start],
            edge_indices: Vec::new(),
            total_cost: 0.0,
        });
    }

    let walk_tpv_f = walk_tpv as f32;

    // g_score[node] = cost of cheapest known path from start to node.
    let mut g_score = vec![f32::INFINITY; n];
    // came_from[node] = (previous node, edge index used to get there).
    let mut came_from: Vec<Option<(NavNodeId, usize)>> = vec![None; n];
    let mut closed = vec![false; n];

    g_score[start.0 as usize] = 0.0;

    let mut open = BinaryHeap::new();
    let h_start = heuristic(graph, start, goal, walk_tpv_f);
    open.push(OpenEntry {
        node: start,
        f_score: h_start,
    });

    while let Some(current) = open.pop() {
        let current_id = current.node;
        let ci = current_id.0 as usize;

        if current_id == goal {
            // Reconstruct path.
            return Some(reconstruct_path(&came_from, start, goal, g_score[ci]));
        }

        if closed[ci] {
            continue;
        }
        closed[ci] = true;

        let current_g = g_score[ci];

        for &edge_idx in graph.neighbors(current_id) {
            let edge = graph.edge(edge_idx);
            let neighbor = edge.to;
            let ni = neighbor.0 as usize;

            if closed[ni] {
                continue;
            }

            let tpv = match edge.edge_type {
                EdgeType::TrunkClimb | EdgeType::GroundToTrunk => match climb_tpv {
                    Some(c) => c as f32,
                    None => continue, // species cannot climb
                },
                _ => walk_tpv_f,
            };
            let tentative_g = current_g + edge.distance * tpv;

            if tentative_g < g_score[ni] {
                g_score[ni] = tentative_g;
                came_from[ni] = Some((current_id, edge_idx));
                let f = tentative_g + heuristic(graph, neighbor, goal, walk_tpv_f);
                open.push(OpenEntry {
                    node: neighbor,
                    f_score: f,
                });
            }
        }
    }

    None // No path found.
}

/// Like `astar`, but only traverses edges whose `edge_type` is in
/// `allowed_edges`. Returns `None` if no path exists using only those types.
pub fn astar_filtered(
    graph: &NavGraph,
    start: NavNodeId,
    goal: NavNodeId,
    walk_tpv: u64,
    climb_tpv: Option<u64>,
    allowed_edges: &[EdgeType],
) -> Option<PathResult> {
    let n = graph.node_slot_count();
    if n == 0 {
        return None;
    }
    if start == goal {
        return Some(PathResult {
            nodes: vec![start],
            edge_indices: Vec::new(),
            total_cost: 0.0,
        });
    }

    let walk_tpv_f = walk_tpv as f32;

    let mut g_score = vec![f32::INFINITY; n];
    let mut came_from: Vec<Option<(NavNodeId, usize)>> = vec![None; n];
    let mut closed = vec![false; n];

    g_score[start.0 as usize] = 0.0;

    let mut open = BinaryHeap::new();
    let h_start = heuristic(graph, start, goal, walk_tpv_f);
    open.push(OpenEntry {
        node: start,
        f_score: h_start,
    });

    while let Some(current) = open.pop() {
        let current_id = current.node;
        let ci = current_id.0 as usize;

        if current_id == goal {
            return Some(reconstruct_path(&came_from, start, goal, g_score[ci]));
        }

        if closed[ci] {
            continue;
        }
        closed[ci] = true;

        let current_g = g_score[ci];

        for &edge_idx in graph.neighbors(current_id) {
            let edge = graph.edge(edge_idx);

            if !allowed_edges.contains(&edge.edge_type) {
                continue;
            }

            let neighbor = edge.to;
            let ni = neighbor.0 as usize;

            if closed[ni] {
                continue;
            }

            let tpv = match edge.edge_type {
                EdgeType::TrunkClimb | EdgeType::GroundToTrunk => match climb_tpv {
                    Some(c) => c as f32,
                    None => continue,
                },
                _ => walk_tpv_f,
            };
            let tentative_g = current_g + edge.distance * tpv;

            if tentative_g < g_score[ni] {
                g_score[ni] = tentative_g;
                came_from[ni] = Some((current_id, edge_idx));
                let f = tentative_g + heuristic(graph, neighbor, goal, walk_tpv_f);
                open.push(OpenEntry {
                    node: neighbor,
                    f_score: f,
                });
            }
        }
    }

    None
}

/// Find the nearest reachable target from `start` using Dijkstra's algorithm.
///
/// Expands outward from `start` by travel cost and returns the first target
/// node reached. This is a multi-target search: it stops as soon as *any*
/// target in `targets` is popped from the priority queue.
///
/// `allowed_edges`: if `Some`, only edges of the listed types are traversed
/// (e.g. capybaras restricted to `ForestFloor`). If `None`, all edges allowed.
///
/// Returns `None` if no target is reachable.
pub fn dijkstra_nearest(
    graph: &NavGraph,
    start: NavNodeId,
    targets: &[NavNodeId],
    walk_tpv: u64,
    climb_tpv: Option<u64>,
    allowed_edges: Option<&[EdgeType]>,
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

    let walk_tpv_f = walk_tpv as f32;

    let mut g_score = vec![f32::INFINITY; n];
    let mut closed = vec![false; n];

    g_score[start.0 as usize] = 0.0;

    let mut open = BinaryHeap::new();
    open.push(OpenEntry {
        node: start,
        f_score: 0.0, // Dijkstra: f = g (no heuristic).
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

            if let Some(allowed) = allowed_edges
                && !allowed.contains(&edge.edge_type)
            {
                continue;
            }

            let neighbor = edge.to;
            let ni = neighbor.0 as usize;

            if closed[ni] {
                continue;
            }

            let tpv = match edge.edge_type {
                EdgeType::TrunkClimb | EdgeType::GroundToTrunk => match climb_tpv {
                    Some(c) => c as f32,
                    None => continue, // species cannot climb
                },
                _ => walk_tpv_f,
            };
            let tentative_g = current_g + edge.distance * tpv;

            if tentative_g < g_score[ni] {
                g_score[ni] = tentative_g;
                open.push(OpenEntry {
                    node: neighbor,
                    f_score: tentative_g,
                });
            }
        }
    }

    None // No target reachable.
}

/// Admissible heuristic: Manhattan distance * walk_ticks_per_voxel.
/// Uses the fastest speed (flat walk) to ensure the heuristic never
/// overestimates.
fn heuristic(graph: &NavGraph, from: NavNodeId, to: NavNodeId, walk_tpv: f32) -> f32 {
    let a = graph.node(from).position;
    let b = graph.node(to).position;
    a.manhattan_distance(b) as f32 * walk_tpv
}

/// Reconstruct the path from came_from data.
fn reconstruct_path(
    came_from: &[Option<(NavNodeId, usize)>],
    start: NavNodeId,
    goal: NavNodeId,
    total_cost: f32,
) -> PathResult {
    let mut nodes = Vec::new();
    let mut edge_indices = Vec::new();
    let mut current = goal;

    loop {
        nodes.push(current);
        if current == start {
            break;
        }
        if let Some((prev, edge_idx)) = came_from[current.0 as usize] {
            edge_indices.push(edge_idx);
            current = prev;
        } else {
            break;
        }
    }

    nodes.reverse();
    edge_indices.reverse();

    PathResult {
        nodes,
        edge_indices,
        total_cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::EdgeType;
    use crate::types::{VoxelCoord, VoxelType};

    /// Shorthand: all test nodes use ForestFloor surface type.
    const S: VoxelType = VoxelType::ForestFloor;

    #[test]
    fn astar_trivial_path() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        // Path from a to a.
        let result = astar(&graph, a, a, 1, Some(2));
        assert!(result.is_some());
        let path = result.unwrap();
        assert_eq!(path.nodes, vec![a]);
        assert!(path.edge_indices.is_empty());
        assert_eq!(path.total_cost, 0.0);
    }

    #[test]
    fn astar_simple_chain() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        // Edges store euclidean distance.
        graph.add_edge(a, b, EdgeType::ForestFloor, 5.0);
        graph.add_edge(b, c, EdgeType::ForestFloor, 5.0);

        // walk_tpv=1 → cost = distance * 1 = distance.
        let result = astar(&graph, a, c, 1, Some(2));
        assert!(result.is_some());
        let path = result.unwrap();
        assert_eq!(path.nodes, vec![a, b, c]);
        assert_eq!(path.edge_indices.len(), 2);
        assert_eq!(path.total_cost, 10.0);
    }

    #[test]
    fn astar_chooses_shortest() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        // Direct long-distance edge a->c.
        graph.add_edge(a, c, EdgeType::ForestFloor, 20.0);
        // Shorter via b.
        graph.add_edge(a, b, EdgeType::ForestFloor, 3.0);
        graph.add_edge(b, c, EdgeType::ForestFloor, 3.0);

        let result = astar(&graph, a, c, 1, Some(2)).unwrap();
        assert_eq!(result.nodes, vec![a, b, c]);
        assert_eq!(result.total_cost, 6.0);
    }

    #[test]
    fn astar_no_path() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        // No edges — no path.
        let result = astar(&graph, a, b, 1, Some(2));
        assert!(result.is_none());
    }

    #[test]
    fn astar_filtered_avoids_disallowed_edges() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        // a->b via ForestFloor, b->c via TrunkClimb.
        graph.add_edge(a, b, EdgeType::ForestFloor, 5.0);
        graph.add_edge(b, c, EdgeType::TrunkClimb, 5.0);

        // Only allow ForestFloor — path a->c should fail (can't cross TrunkClimb).
        let result = astar_filtered(&graph, a, c, 1, Some(2), &[EdgeType::ForestFloor]);
        assert!(result.is_none());

        // Allow both — should succeed.
        let result = astar_filtered(
            &graph,
            a,
            c,
            1,
            Some(2),
            &[EdgeType::ForestFloor, EdgeType::TrunkClimb],
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().nodes, vec![a, b, c]);
    }

    #[test]
    fn astar_filtered_same_start_and_goal() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let result = astar_filtered(&graph, a, a, 1, Some(2), &[EdgeType::ForestFloor]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().total_cost, 0.0);
    }

    #[test]
    fn astar_deterministic() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(6, 0, 0), S);
        let d = graph.add_node(VoxelCoord::new(3, 3, 0), S);
        graph.add_edge(a, b, EdgeType::ForestFloor, 3.0);
        graph.add_edge(b, c, EdgeType::ForestFloor, 3.0);
        graph.add_edge(a, d, EdgeType::TrunkClimb, 4.0);
        graph.add_edge(d, c, EdgeType::TrunkClimb, 4.0);

        let r1 = astar(&graph, a, c, 500, Some(1250)).unwrap();
        let r2 = astar(&graph, a, c, 500, Some(1250)).unwrap();
        assert_eq!(r1.nodes, r2.nodes);
        assert_eq!(r1.total_cost, r2.total_cost);
    }

    // -------------------------------------------------------------------
    // dijkstra_nearest tests
    // -------------------------------------------------------------------

    #[test]
    fn dijkstra_nearest_finds_closest_by_travel_cost() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        // a->b short, a->c long (but c is spatially further).
        graph.add_edge(a, b, EdgeType::ForestFloor, 3.0);
        graph.add_edge(b, c, EdgeType::ForestFloor, 7.0);

        // Both b and c are targets — b should win (closer by travel cost).
        let result = dijkstra_nearest(&graph, a, &[b, c], 1, Some(1), None);
        assert_eq!(result, Some(b));
    }

    #[test]
    fn dijkstra_nearest_respects_edge_filter() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(3, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(6, 0, 0), S);
        // a->b via ForestFloor (short), b->c via TrunkClimb.
        graph.add_edge(a, b, EdgeType::ForestFloor, 3.0);
        graph.add_edge(b, c, EdgeType::TrunkClimb, 3.0);

        // Only ForestFloor allowed — c is unreachable.
        let result = dijkstra_nearest(&graph, a, &[c], 1, Some(1), Some(&[EdgeType::ForestFloor]));
        assert_eq!(result, None);

        // b is reachable via ForestFloor.
        let result = dijkstra_nearest(&graph, a, &[b], 1, Some(1), Some(&[EdgeType::ForestFloor]));
        assert_eq!(result, Some(b));
    }

    #[test]
    fn dijkstra_nearest_prefers_fast_route() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), S);
        let c = graph.add_node(VoxelCoord::new(0, 5, 0), S);
        // a->b: short distance via ForestFloor (walk speed).
        // a->c: short distance via TrunkClimb (climb speed — slower).
        graph.add_edge(a, b, EdgeType::ForestFloor, 5.0);
        graph.add_edge(a, c, EdgeType::TrunkClimb, 5.0);

        // walk_tpv=500, climb_tpv=1250.
        // Cost to b: 5 * 500 = 2500. Cost to c: 5 * 1250 = 6250.
        // b should win.
        let result = dijkstra_nearest(&graph, a, &[b, c], 500, Some(1250), None);
        assert_eq!(result, Some(b));
    }

    #[test]
    fn dijkstra_nearest_start_is_target() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let result = dijkstra_nearest(&graph, a, &[a], 1, Some(1), None);
        assert_eq!(result, Some(a));
    }

    #[test]
    fn dijkstra_nearest_no_targets() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let result = dijkstra_nearest(&graph, a, &[], 1, Some(1), None);
        assert_eq!(result, None);
    }

    #[test]
    fn dijkstra_nearest_unreachable_target() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), S);
        let b = graph.add_node(VoxelCoord::new(10, 0, 0), S);
        // No edges — b is unreachable.
        let result = dijkstra_nearest(&graph, a, &[b], 1, Some(1), None);
        assert_eq!(result, None);
    }
}
