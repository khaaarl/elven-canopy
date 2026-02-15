// Navigation graph for creature pathfinding.
//
// The nav graph is a set of `NavNode`s (positions) connected by `NavEdge`s
// (typed, weighted connections). It is built from tree geometry by
// `build_nav_graph()` and used by `pathfinding.rs` for A* search.
//
// All storage uses `Vec` indexed by `NavNodeId`/`NavEdgeId` for O(1) lookup
// and deterministic iteration order. No `HashMap`.
//
// See also: `tree_gen.rs` for the tree geometry that feeds graph construction,
// `pathfinding.rs` for A* search over this graph, `sim.rs` which owns the
// `NavGraph` as part of `SimState`.
//
// **Critical constraint: determinism.** The graph is built from deterministic
// tree geometry. Node/edge IDs are sequential integers assigned in fixed order.

use crate::config::GameConfig;
use crate::sim::Tree;
use crate::types::{NavEdgeId, NavNodeId, VoxelCoord};
use serde::{Deserialize, Serialize};

/// A node in the navigation graph — a position an elf can stand on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavNode {
    pub id: NavNodeId,
    pub position: VoxelCoord,
    /// Indices into `NavGraph.edges` for edges that originate from this node.
    pub edge_indices: Vec<usize>,
}

/// The type of connection between two nav nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeType {
    /// Walking on the forest floor around the trunk base.
    ForestFloor,
    /// Climbing up/down the raw trunk surface.
    TrunkClimb,
    /// Walking along a branch.
    BranchWalk,
    /// Circumferential movement around the trunk at one y-level.
    TrunkCircumference,
    /// Connecting ground ring to lowest trunk surface nodes.
    GroundToTrunk,
}

/// A directed edge in the navigation graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavEdge {
    pub id: NavEdgeId,
    pub from: NavNodeId,
    pub to: NavNodeId,
    pub edge_type: EdgeType,
    /// Traversal cost in ticks (based on distance and speed multiplier).
    pub cost: f32,
}

/// The navigation graph container.
#[derive(Clone, Debug, Default)]
pub struct NavGraph {
    pub nodes: Vec<NavNode>,
    pub edges: Vec<NavEdge>,
}

impl NavGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node at the given position. Returns its ID.
    pub fn add_node(&mut self, position: VoxelCoord) -> NavNodeId {
        let id = NavNodeId(self.nodes.len() as u32);
        self.nodes.push(NavNode {
            id,
            position,
            edge_indices: Vec::new(),
        });
        id
    }

    /// Add a bidirectional edge between two nodes. Returns the edge ID of the
    /// forward (from -> to) edge.
    pub fn add_edge(
        &mut self,
        from: NavNodeId,
        to: NavNodeId,
        edge_type: EdgeType,
        cost: f32,
    ) -> NavEdgeId {
        let forward_id = NavEdgeId(self.edges.len() as u32);
        let reverse_id = NavEdgeId(self.edges.len() as u32 + 1);

        let forward_idx = self.edges.len();
        self.edges.push(NavEdge {
            id: forward_id,
            from,
            to,
            edge_type,
            cost,
        });

        let reverse_idx = self.edges.len();
        self.edges.push(NavEdge {
            id: reverse_id,
            from: to,
            to: from,
            edge_type,
            cost,
        });

        self.nodes[from.0 as usize].edge_indices.push(forward_idx);
        self.nodes[to.0 as usize].edge_indices.push(reverse_idx);

        forward_id
    }

    /// Get all edges originating from a node.
    pub fn neighbors(&self, node: NavNodeId) -> &[usize] {
        &self.nodes[node.0 as usize].edge_indices
    }

    /// Get a node by ID.
    pub fn node(&self, id: NavNodeId) -> &NavNode {
        &self.nodes[id.0 as usize]
    }

    /// Get an edge by index.
    pub fn edge(&self, idx: usize) -> &NavEdge {
        &self.edges[idx]
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Find the nearest node to a given position (by Manhattan distance).
    /// Returns `None` if the graph is empty.
    pub fn find_nearest_node(&self, pos: VoxelCoord) -> Option<NavNodeId> {
        self.nodes
            .iter()
            .min_by_key(|n| n.position.manhattan_distance(pos))
            .map(|n| n.id)
    }

    /// Find the nearest ground-level (y=0) node to the given position.
    /// Returns `None` if no ground nodes exist.
    pub fn find_nearest_ground_node(&self, pos: VoxelCoord) -> Option<NavNodeId> {
        self.nodes
            .iter()
            .filter(|n| n.position.y == 0)
            .min_by_key(|n| n.position.manhattan_distance(pos))
            .map(|n| n.id)
    }

    /// Return all ground-level (y=0) node IDs.
    pub fn ground_node_ids(&self) -> Vec<NavNodeId> {
        self.nodes
            .iter()
            .filter(|n| n.position.y == 0)
            .map(|n| n.id)
            .collect()
    }
}

/// Build a navigation graph from tree geometry.
///
/// Layout:
/// 1. **Ground rings**: `ground_ring_count` concentric rings of 8 nodes each
///    (4 cardinals N/E/S/W + 4 intercardinals NE/SE/SW/NW) at y=0. The inner
///    ring sits at `trunk_radius + 2`, each subsequent ring is
///    `ground_ring_spacing` voxels farther out. Rings are connected
///    circumferentially with `ForestFloor` edges, and adjacent rings are
///    connected radially (same direction, 8 pairs per ring pair).
/// 2. **Trunk surface**: Every `nav_node_vertical_spacing` y-levels from y=1
///    up to trunk height, place 4 nodes (N/S/E/W at radius+1). Connected
///    vertically with `TrunkClimb` edges, horizontally with
///    `TrunkCircumference` edges.
/// 3. **Branches**: Nav nodes placed along each branch's centerline path
///    (root, every 3 steps, tip), connected with `BranchWalk` edges. Primary
///    branch roots connect to the nearest trunk surface node via `TrunkClimb`.
///    Sub-branch roots connect to the nearest nav node on their parent branch
///    via `BranchWalk`. The `per_branch_nav_nodes` vec tracks nav node IDs
///    per branch path, enabling parent lookups for sub-branches.
/// 4. **Ground-to-trunk**: Inner ring's 4 cardinal nodes connect to the
///    lowest trunk surface ring.
pub fn build_nav_graph(tree: &Tree, config: &GameConfig) -> NavGraph {
    let mut graph = NavGraph::new();

    let cx = tree.position.x;
    let cz = tree.position.z;
    let trunk_radius = config.tree_trunk_radius as i32;

    // Cardinal offsets: N(+Z), E(+X), S(-Z), W(-X)
    let cardinal_offsets: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];

    // 8 direction offsets for ground rings: 4 cardinals + 4 intercardinals.
    // Order: N, NE, E, SE, S, SW, W, NW — so indices 0,2,4,6 are cardinal.
    let ground_dir_offsets: [(i32, i32); 8] = [
        (0, 1),   // N (+Z)
        (1, 1),   // NE
        (1, 0),   // E (+X)
        (1, -1),  // SE
        (0, -1),  // S (-Z)
        (-1, -1), // SW
        (-1, 0),  // W (-X)
        (-1, 1),  // NW
    ];

    // --- 1. Ground rings ---
    // Build concentric rings of 8 nodes each around the trunk base.
    let ring_count = config.ground_ring_count.max(1) as usize;
    let ring_spacing = config.ground_ring_spacing.max(1) as i32;
    let inner_radius = trunk_radius + 2;
    let base_speed = config.nav_base_speed;

    // ground_rings[ring_idx][dir_idx] = NavNodeId
    let mut ground_rings: Vec<Vec<NavNodeId>> = Vec::new();

    for ring_idx in 0..ring_count {
        let radius = inner_radius + ring_idx as i32 * ring_spacing;
        let mut ring = Vec::new();
        for &(dx, dz) in &ground_dir_offsets {
            let id = graph.add_node(VoxelCoord::new(
                cx + dx * radius,
                0,
                cz + dz * radius,
            ));
            ring.push(id);
        }
        // Connect circumferentially within this ring.
        for i in 0..ring.len() {
            let next = (i + 1) % ring.len();
            let a_pos = graph.node(ring[i]).position;
            let b_pos = graph.node(ring[next]).position;
            let dist = a_pos.manhattan_distance(b_pos) as f32;
            let cost = dist / base_speed;
            graph.add_edge(ring[i], ring[next], EdgeType::ForestFloor, cost);
        }
        ground_rings.push(ring);
    }

    // Connect adjacent rings radially (same direction index, 8 pairs per ring pair).
    for r in 1..ground_rings.len() {
        for dir in 0..8 {
            let inner = ground_rings[r - 1][dir];
            let outer = ground_rings[r][dir];
            let i_pos = graph.node(inner).position;
            let o_pos = graph.node(outer).position;
            let dist = i_pos.manhattan_distance(o_pos) as f32;
            let cost = dist / base_speed;
            graph.add_edge(inner, outer, EdgeType::ForestFloor, cost);
        }
    }

    // --- 2. Trunk surface rings ---
    let spacing = config.nav_node_vertical_spacing.max(1);
    let trunk_height = config.tree_trunk_height;
    let trunk_surface_offset = trunk_radius + 1;

    // Collect trunk rings for vertical connections.
    let mut trunk_rings: Vec<Vec<NavNodeId>> = Vec::new();

    let mut y = spacing;
    while y <= trunk_height {
        let mut ring = Vec::new();
        for &(dx, dz) in &cardinal_offsets {
            let id = graph.add_node(VoxelCoord::new(
                cx + dx * trunk_surface_offset,
                y as i32,
                cz + dz * trunk_surface_offset,
            ));
            ring.push(id);
        }
        // Connect circumferentially.
        for i in 0..ring.len() {
            let next = (i + 1) % ring.len();
            let a_pos = graph.node(ring[i]).position;
            let b_pos = graph.node(ring[next]).position;
            let dist = a_pos.manhattan_distance(b_pos) as f32;
            let cost = dist / base_speed;
            graph.add_edge(ring[i], ring[next], EdgeType::TrunkCircumference, cost);
        }
        trunk_rings.push(ring);
        y += spacing;
    }

    // Connect rings vertically with TrunkClimb edges.
    let climb_speed = base_speed * config.climb_speed_multiplier;
    for i in 1..trunk_rings.len() {
        for j in 0..4 {
            let lower = trunk_rings[i - 1][j];
            let upper = trunk_rings[i][j];
            let l_pos = graph.node(lower).position;
            let u_pos = graph.node(upper).position;
            let dist = l_pos.manhattan_distance(u_pos) as f32;
            let cost = dist / climb_speed;
            graph.add_edge(lower, upper, EdgeType::TrunkClimb, cost);
        }
    }

    // --- 3. Ground-to-trunk connections ---
    // Connect inner ring's 4 cardinal nodes (indices 0,2,4,6 = N,E,S,W)
    // to the lowest trunk surface ring's 4 cardinal nodes (indices 0,1,2,3).
    if let Some(first_trunk_ring) = trunk_rings.first() {
        if let Some(inner_ground_ring) = ground_rings.first() {
            // Cardinal ground indices [0,2,4,6] map to trunk indices [0,1,2,3].
            let cardinal_ground_indices = [0usize, 2, 4, 6];
            for (trunk_i, &ground_i) in cardinal_ground_indices.iter().enumerate() {
                let ground = inner_ground_ring[ground_i];
                let trunk_node = first_trunk_ring[trunk_i];
                let g_pos = graph.node(ground).position;
                let t_pos = graph.node(trunk_node).position;
                let dist = g_pos.manhattan_distance(t_pos) as f32;
                let cost = dist / climb_speed;
                graph.add_edge(ground, trunk_node, EdgeType::GroundToTrunk, cost);
            }
        }
    }

    // --- 4. Branch nodes ---
    // Place nav nodes along each branch path (root, every 3 steps, tip).
    // Track per-branch nav nodes so sub-branches can connect to parents.
    let mut per_branch_nav_nodes: Vec<Vec<NavNodeId>> = Vec::new();

    for (path_idx, path) in tree.branch_paths.iter().enumerate() {
        if path.is_empty() {
            per_branch_nav_nodes.push(Vec::new());
            continue;
        }

        // Place nav nodes at root (0), every 3 steps, and tip.
        let mut branch_nav_ids: Vec<NavNodeId> = Vec::new();
        for (step, pos) in path.iter().enumerate() {
            if step == 0 || step == path.len() - 1 || step % 3 == 0 {
                branch_nav_ids.push(graph.add_node(*pos));
            }
        }

        // Connect consecutive nav nodes along this branch.
        for i in 1..branch_nav_ids.len() {
            let prev = branch_nav_ids[i - 1];
            let curr = branch_nav_ids[i];
            let p_pos = graph.node(prev).position;
            let c_pos = graph.node(curr).position;
            let dist = p_pos.manhattan_distance(c_pos) as f32;
            let cost = dist / base_speed;
            if dist > 0.0 {
                graph.add_edge(prev, curr, EdgeType::BranchWalk, cost);
            }
        }

        // Connect this branch's root to the tree structure.
        let root_node = branch_nav_ids[0];
        let root_pos = graph.node(root_node).position;

        match &tree.branch_parents[path_idx] {
            None => {
                // Primary branch: connect root to nearest trunk surface node.
                let nearest_trunk = trunk_rings
                    .iter()
                    .flat_map(|ring| ring.iter())
                    .min_by_key(|&&node_id| {
                        graph.node(node_id).position.manhattan_distance(root_pos)
                    })
                    .copied();

                if let Some(trunk_node) = nearest_trunk {
                    let t_pos = graph.node(trunk_node).position;
                    let d = root_pos.manhattan_distance(t_pos) as f32;
                    let c = d / climb_speed;
                    graph.add_edge(root_node, trunk_node, EdgeType::TrunkClimb, c);
                }
            }
            Some(parent) => {
                // Sub-branch: connect root to nearest nav node on parent branch.
                let parent_nav = &per_branch_nav_nodes[parent.parent_path_idx];
                if let Some(&closest) = parent_nav
                    .iter()
                    .min_by_key(|&&nid| graph.node(nid).position.manhattan_distance(root_pos))
                {
                    let p_pos = graph.node(closest).position;
                    let d = root_pos.manhattan_distance(p_pos) as f32;
                    // Use max(1.0, ...) so overlapping positions still get connected.
                    let c = if d > 0.0 { d / base_speed } else { 1.0 };
                    graph.add_edge(root_node, closest, EdgeType::BranchWalk, c);
                }
            }
        }

        per_branch_nav_nodes.push(branch_nav_ids);
    }

    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_node_assigns_sequential_ids() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let b = graph.add_node(VoxelCoord::new(1, 0, 0));
        let c = graph.add_node(VoxelCoord::new(2, 0, 0));
        assert_eq!(a, NavNodeId(0));
        assert_eq!(b, NavNodeId(1));
        assert_eq!(c, NavNodeId(2));
        assert_eq!(graph.node_count(), 3);
    }

    #[test]
    fn add_edge_creates_bidirectional() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0));
        let b = graph.add_node(VoxelCoord::new(5, 0, 0));
        graph.add_edge(a, b, EdgeType::ForestFloor, 5.0);

        // Node a should have an edge to b.
        let a_edges: Vec<_> = graph
            .neighbors(a)
            .iter()
            .map(|&idx| graph.edge(idx).to)
            .collect();
        assert_eq!(a_edges, vec![b]);

        // Node b should have an edge back to a.
        let b_edges: Vec<_> = graph
            .neighbors(b)
            .iter()
            .map(|&idx| graph.edge(idx).to)
            .collect();
        assert_eq!(b_edges, vec![a]);
    }

    #[test]
    fn find_nearest_node_works() {
        let mut graph = NavGraph::new();
        graph.add_node(VoxelCoord::new(0, 0, 0));
        graph.add_node(VoxelCoord::new(10, 0, 0));
        graph.add_node(VoxelCoord::new(5, 5, 0));

        let nearest = graph.find_nearest_node(VoxelCoord::new(4, 4, 0));
        assert_eq!(nearest, Some(NavNodeId(2))); // (5,5,0) is closest
    }

    #[test]
    fn find_nearest_node_empty_graph() {
        let graph = NavGraph::new();
        assert_eq!(graph.find_nearest_node(VoxelCoord::new(0, 0, 0)), None);
    }

    #[test]
    fn build_nav_graph_has_ground_ring() {
        let tree = test_tree();
        let config = test_config();
        let graph = build_nav_graph(&tree, &config);

        // Should have at least 4 ground nodes at y=0.
        let ground_nodes: Vec<_> = graph
            .nodes
            .iter()
            .filter(|n| n.position.y == 0)
            .collect();
        assert!(ground_nodes.len() >= 4);
    }

    #[test]
    fn build_nav_graph_has_trunk_nodes() {
        let tree = test_tree();
        let config = test_config();
        let graph = build_nav_graph(&tree, &config);

        // Should have trunk surface nodes above y=0.
        let trunk_nodes: Vec<_> = graph
            .nodes
            .iter()
            .filter(|n| n.position.y > 0)
            .collect();
        assert!(!trunk_nodes.is_empty());
    }

    #[test]
    fn build_nav_graph_is_connected() {
        use crate::pathfinding;

        let tree = test_tree();
        let config = test_config();
        let graph = build_nav_graph(&tree, &config);

        // Every node should be reachable from node 0.
        let start = NavNodeId(0);
        for i in 1..graph.node_count() {
            let goal = NavNodeId(i as u32);
            let path = pathfinding::astar(&graph, start, goal, 1.0);
            assert!(
                path.is_some(),
                "No path from node 0 to node {i} (pos {:?})",
                graph.node(goal).position,
            );
        }
    }

    fn test_tree() -> Tree {
        use crate::prng::GameRng;
        use crate::tree_gen;
        use crate::world::VoxelWorld;

        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = tree_gen::generate_tree(&mut world, &config, &mut rng);

        Tree {
            id: crate::types::TreeId(crate::types::SimUuid::new_v4(&mut rng)),
            position: VoxelCoord::new(32, 0, 32),
            health: 100.0,
            growth_level: 1,
            mana_stored: 100.0,
            mana_capacity: 500.0,
            fruit_production_rate: 0.5,
            carrying_capacity: 20.0,
            current_load: 0.0,
            owner: None,
            trunk_voxels: result.trunk_voxels,
            branch_voxels: result.branch_voxels,
            branch_paths: result.branch_paths,
            branch_parents: result.branch_parents,
        }
    }

    #[test]
    fn build_nav_graph_is_connected_with_forks() {
        use crate::pathfinding;

        let config = GameConfig {
            world_size: (64, 64, 64),
            tree_trunk_radius: 3,
            tree_trunk_height: 30,
            tree_branch_start_y: 10,
            tree_branch_interval: 5,
            tree_branch_count: 4,
            tree_branch_length: 6,
            tree_branch_radius: 1,
            tree_branch_fork_chance: 1.0,
            tree_branch_fork_min_step: 2,
            tree_branch_fork_max_depth: 2,
            nav_node_vertical_spacing: 4,
            ..GameConfig::default()
        };

        let mut world = crate::world::VoxelWorld::new(64, 64, 64);
        let mut rng = crate::prng::GameRng::new(42);
        let result = crate::tree_gen::generate_tree(&mut world, &config, &mut rng);

        let tree = Tree {
            id: crate::types::TreeId(crate::types::SimUuid::new_v4(&mut rng)),
            position: VoxelCoord::new(32, 0, 32),
            health: 100.0,
            growth_level: 1,
            mana_stored: 100.0,
            mana_capacity: 500.0,
            fruit_production_rate: 0.5,
            carrying_capacity: 20.0,
            current_load: 0.0,
            owner: None,
            trunk_voxels: result.trunk_voxels,
            branch_voxels: result.branch_voxels,
            branch_paths: result.branch_paths,
            branch_parents: result.branch_parents,
        };

        assert!(
            tree.branch_paths.len() > 4,
            "Expected forks to produce more than 4 branch paths, got {}",
            tree.branch_paths.len(),
        );

        let graph = build_nav_graph(&tree, &config);

        // Every node should be reachable from node 0.
        let start = NavNodeId(0);
        for i in 1..graph.node_count() {
            let goal = NavNodeId(i as u32);
            let path = pathfinding::astar(&graph, start, goal, 1.0);
            assert!(
                path.is_some(),
                "No path from node 0 to node {i} (pos {:?})",
                graph.node(goal).position,
            );
        }
    }

    fn test_config() -> GameConfig {
        GameConfig {
            world_size: (64, 64, 64),
            tree_trunk_radius: 3,
            tree_trunk_height: 30,
            tree_branch_start_y: 10,
            tree_branch_interval: 5,
            tree_branch_count: 4,
            tree_branch_length: 6,
            tree_branch_radius: 1,
            tree_branch_fork_max_depth: 0,
            nav_node_vertical_spacing: 4,
            ..GameConfig::default()
        }
    }
}
