// A* pathfinding over the navigation graph.
//
// Implements standard A* search using a `BinaryHeap` (min-heap via reversed
// ordering, same pattern as `EventQueue`). Node scores and came-from data
// are stored in `Vec`s indexed by `NavNodeId` for O(1) access and
// deterministic behavior (no `HashMap`).
//
// The heuristic is Manhattan distance divided by max speed, which is
// admissible (never overestimates).
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
        self.f_score.total_cmp(&other.f_score) == Ordering::Equal
            && self.node == other.node
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
/// The `max_speed` parameter is used for the heuristic (Manhattan / max_speed).
pub fn astar(
    graph: &NavGraph,
    start: NavNodeId,
    goal: NavNodeId,
    max_speed: f32,
) -> Option<PathResult> {
    let n = graph.node_count();
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

    // g_score[node] = cost of cheapest known path from start to node.
    let mut g_score = vec![f32::INFINITY; n];
    // came_from[node] = (previous node, edge index used to get there).
    let mut came_from: Vec<Option<(NavNodeId, usize)>> = vec![None; n];
    let mut closed = vec![false; n];

    g_score[start.0 as usize] = 0.0;

    let mut open = BinaryHeap::new();
    let h_start = heuristic(graph, start, goal, max_speed);
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

            let tentative_g = current_g + edge.cost;

            if tentative_g < g_score[ni] {
                g_score[ni] = tentative_g;
                came_from[ni] = Some((current_id, edge_idx));
                let f = tentative_g + heuristic(graph, neighbor, goal, max_speed);
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
    max_speed: f32,
    allowed_edges: &[EdgeType],
) -> Option<PathResult> {
    let n = graph.node_count();
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

    let mut g_score = vec![f32::INFINITY; n];
    let mut came_from: Vec<Option<(NavNodeId, usize)>> = vec![None; n];
    let mut closed = vec![false; n];

    g_score[start.0 as usize] = 0.0;

    let mut open = BinaryHeap::new();
    let h_start = heuristic(graph, start, goal, max_speed);
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

            let tentative_g = current_g + edge.cost;

            if tentative_g < g_score[ni] {
                g_score[ni] = tentative_g;
                came_from[ni] = Some((current_id, edge_idx));
                let f = tentative_g + heuristic(graph, neighbor, goal, max_speed);
                open.push(OpenEntry {
                    node: neighbor,
                    f_score: f,
                });
            }
        }
    }

    None
}

/// Admissible heuristic: Manhattan distance / max_speed.
fn heuristic(graph: &NavGraph, from: NavNodeId, to: NavNodeId, max_speed: f32) -> f32 {
    let a = graph.node(from).position;
    let b = graph.node(to).position;
    a.manhattan_distance(b) as f32 / max_speed
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
    use crate::types::VoxelCoord;

    #[test]
    fn astar_trivial_path() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        // Path from a to a.
        let result = astar(&graph, a, a, 1.0);
        assert!(result.is_some());
        let path = result.unwrap();
        assert_eq!(path.nodes, vec![a]);
        assert!(path.edge_indices.is_empty());
        assert_eq!(path.total_cost, 0.0);
    }

    #[test]
    fn astar_simple_chain() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let b = graph.add_node(VoxelCoord::new(5, 0, 0));
        let c = graph.add_node(VoxelCoord::new(10, 0, 0));
        graph.add_edge(a, b, EdgeType::ForestFloor, 5.0);
        graph.add_edge(b, c, EdgeType::ForestFloor, 5.0);

        let result = astar(&graph, a, c, 1.0);
        assert!(result.is_some());
        let path = result.unwrap();
        assert_eq!(path.nodes, vec![a, b, c]);
        assert_eq!(path.edge_indices.len(), 2);
        assert_eq!(path.total_cost, 10.0);
    }

    #[test]
    fn astar_chooses_shortest() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let b = graph.add_node(VoxelCoord::new(5, 0, 0));
        let c = graph.add_node(VoxelCoord::new(10, 0, 0));
        // Direct expensive edge a->c.
        graph.add_edge(a, c, EdgeType::ForestFloor, 20.0);
        // Cheaper via b.
        graph.add_edge(a, b, EdgeType::ForestFloor, 3.0);
        graph.add_edge(b, c, EdgeType::ForestFloor, 3.0);

        let result = astar(&graph, a, c, 1.0).unwrap();
        assert_eq!(result.nodes, vec![a, b, c]);
        assert_eq!(result.total_cost, 6.0);
    }

    #[test]
    fn astar_no_path() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let b = graph.add_node(VoxelCoord::new(10, 0, 0));
        // No edges — no path.
        let result = astar(&graph, a, b, 1.0);
        assert!(result.is_none());
    }

    #[test]
    fn astar_filtered_avoids_disallowed_edges() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let b = graph.add_node(VoxelCoord::new(5, 0, 0));
        let c = graph.add_node(VoxelCoord::new(10, 0, 0));
        // a->b via ForestFloor, b->c via TrunkClimb.
        graph.add_edge(a, b, EdgeType::ForestFloor, 5.0);
        graph.add_edge(b, c, EdgeType::TrunkClimb, 5.0);

        // Only allow ForestFloor — path a->c should fail (can't cross TrunkClimb).
        let result = astar_filtered(&graph, a, c, 1.0, &[EdgeType::ForestFloor]);
        assert!(result.is_none());

        // Allow both — should succeed.
        let result = astar_filtered(
            &graph,
            a,
            c,
            1.0,
            &[EdgeType::ForestFloor, EdgeType::TrunkClimb],
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().nodes, vec![a, b, c]);
    }

    #[test]
    fn astar_filtered_same_start_and_goal() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let result = astar_filtered(&graph, a, a, 1.0, &[EdgeType::ForestFloor]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().total_cost, 0.0);
    }

    #[test]
    fn astar_deterministic() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let b = graph.add_node(VoxelCoord::new(3, 0, 0));
        let c = graph.add_node(VoxelCoord::new(6, 0, 0));
        let d = graph.add_node(VoxelCoord::new(3, 3, 0));
        graph.add_edge(a, b, EdgeType::ForestFloor, 3.0);
        graph.add_edge(b, c, EdgeType::ForestFloor, 3.0);
        graph.add_edge(a, d, EdgeType::TrunkClimb, 4.0);
        graph.add_edge(d, c, EdgeType::TrunkClimb, 4.0);

        let r1 = astar(&graph, a, c, 1.0).unwrap();
        let r2 = astar(&graph, a, c, 1.0).unwrap();
        assert_eq!(r1.nodes, r2.nodes);
        assert_eq!(r1.total_cost, r2.total_cost);
    }
}
