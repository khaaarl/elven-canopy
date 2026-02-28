// Navigation graph for creature pathfinding.
//
// The nav graph is a set of `NavNode`s (positions) connected by `NavEdge`s
// (typed connections with euclidean distance). It is built from the voxel
// world by `build_nav_graph()` and used by `pathfinding.rs` for A* search.
//
// **Edges store distance, not time-cost.** Each edge records the euclidean
// distance between its endpoints and the edge type (ForestFloor, TrunkClimb,
// etc.). The pathfinder computes actual traversal time at search time using
// per-species speed parameters (walk_ticks_per_voxel, climb_ticks_per_voxel
// from `species.rs`). This means graph construction needs only the voxel
// world — no speed config required.
//
// **Voxel-derived construction:** Every air voxel that is face-adjacent to at
// least one solid voxel becomes a nav node. Edges connect 26-neighbors among
// nav nodes. This means the nav graph reflects actual world geometry —
// construction changes the navigable topology via incremental updates.
//
// Each nav node carries a `surface_type` derived from the solid voxel it
// touches (see `derive_surface_type()`). Edge types are derived from the
// surface types of the two endpoints (see `derive_edge_type()`). Root voxels
// are treated as walkable surfaces (BranchWalk), with ForestFloor and
// TrunkClimb transitions at boundaries.
//
// **Stable node IDs and incremental updates.** Nodes are stored as
// `Vec<Option<NavNode>>` — `Some` for live nodes, `None` for removed (dead)
// slots. This allows `update_after_voxel_solidified()` to add/remove nodes
// without shifting IDs, so creatures' `current_node` references remain valid
// unless their specific node was removed. Dead slots are recycled via
// `free_slots`. A persistent `spatial_index` (flat voxel index → node slot)
// enables O(1) coord→node lookup for both incremental updates and
// `has_node_at()` queries.
//
// The full `build_nav_graph()` is used at startup and save/load. During
// gameplay, `materialize_next_build_voxel()` in `sim.rs` calls
// `update_after_voxel_solidified()` which touches only ~7 positions and their
// 26-neighbor edges — O(1) instead of O(world_size).
//
// All storage uses `Vec` indexed by `NavNodeId`/`NavEdgeId` for O(1) lookup
// and deterministic iteration order. No `HashMap`.
//
// See also: `world.rs` for the voxel grid, `tree_gen.rs` for tree geometry,
// `pathfinding.rs` for A* search over this graph, `sim.rs` which owns the
// `NavGraph` as part of `SimState`.
//
// **Critical constraint: determinism.** The graph is built by iterating voxels
// in fixed order (matching the flat index of `VoxelWorld`). Node/edge IDs are
// sequential integers assigned in that order. Incremental updates are also
// deterministic — they process affected positions in a fixed order.

use crate::types::{NavEdgeId, NavNodeId, VoxelCoord, VoxelType};
use crate::world::VoxelWorld;
use serde::{Deserialize, Serialize};

/// A node in the navigation graph — a position a creature can occupy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavNode {
    pub id: NavNodeId,
    pub position: VoxelCoord,
    /// The type of solid surface this node is adjacent to. Determines what
    /// kind of creature movement is valid here (e.g. `ForestFloor` for ground
    /// walking, `Trunk` for climbing).
    pub surface_type: VoxelType,
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
    /// Connecting ground-level nodes to trunk surface nodes.
    GroundToTrunk,
}

/// A directed edge in the navigation graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavEdge {
    pub id: NavEdgeId,
    pub from: NavNodeId,
    pub to: NavNodeId,
    pub edge_type: EdgeType,
    /// Euclidean distance between the two endpoints (in voxel units).
    pub distance: f32,
}

/// The navigation graph container.
///
/// Nodes are stored as `Option<NavNode>` slots — `Some` for live nodes, `None`
/// for removed nodes. This allows incremental updates (removing/adding nodes)
/// without shifting IDs. The `spatial_index` maps flat voxel indices to node
/// slots for O(1) coord→node lookup; `free_slots` tracks recyclable slots.
#[derive(Clone, Debug, Default)]
pub struct NavGraph {
    nodes: Vec<Option<NavNode>>,
    pub edges: Vec<NavEdge>,
    spatial_index: Vec<u32>,
    free_slots: Vec<usize>,
    world_size: (usize, usize, usize),
}

impl NavGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node at the given position with the given surface type. Returns
    /// its ID.
    pub fn add_node(&mut self, position: VoxelCoord, surface_type: VoxelType) -> NavNodeId {
        let id = NavNodeId(self.nodes.len() as u32);
        self.nodes.push(Some(NavNode {
            id,
            position,
            surface_type,
            edge_indices: Vec::new(),
        }));
        id
    }

    /// Add a bidirectional edge between two nodes. Returns the edge ID of the
    /// forward (from -> to) edge.
    pub fn add_edge(
        &mut self,
        from: NavNodeId,
        to: NavNodeId,
        edge_type: EdgeType,
        distance: f32,
    ) -> NavEdgeId {
        let forward_id = NavEdgeId(self.edges.len() as u32);
        let reverse_id = NavEdgeId(self.edges.len() as u32 + 1);

        let forward_idx = self.edges.len();
        self.edges.push(NavEdge {
            id: forward_id,
            from,
            to,
            edge_type,
            distance,
        });

        let reverse_idx = self.edges.len();
        self.edges.push(NavEdge {
            id: reverse_id,
            from: to,
            to: from,
            edge_type,
            distance,
        });

        self.nodes[from.0 as usize]
            .as_mut()
            .unwrap()
            .edge_indices
            .push(forward_idx);
        self.nodes[to.0 as usize]
            .as_mut()
            .unwrap()
            .edge_indices
            .push(reverse_idx);

        forward_id
    }

    /// Get all edges originating from a node.
    pub fn neighbors(&self, node: NavNodeId) -> &[usize] {
        &self.nodes[node.0 as usize].as_ref().unwrap().edge_indices
    }

    /// Get a node by ID. Panics if the slot is dead (`None`).
    pub fn node(&self, id: NavNodeId) -> &NavNode {
        self.nodes[id.0 as usize].as_ref().unwrap()
    }

    /// Get an edge by index.
    pub fn edge(&self, idx: usize) -> &NavEdge {
        &self.edges[idx]
    }

    /// Number of live nodes (excludes dead slots).
    pub fn node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_some()).count()
    }

    /// Total node slots including dead ones (for A* array sizing).
    pub fn node_slot_count(&self) -> usize {
        self.nodes.len()
    }

    /// Iterate over all live nodes (skips dead `None` slots).
    pub fn live_nodes(&self) -> impl Iterator<Item = &NavNode> {
        self.nodes.iter().filter_map(|n| n.as_ref())
    }

    /// Find the nearest node to a given position (by Manhattan distance).
    /// Returns `None` if the graph is empty.
    pub fn find_nearest_node(&self, pos: VoxelCoord) -> Option<NavNodeId> {
        self.live_nodes()
            .min_by_key(|n| n.position.manhattan_distance(pos))
            .map(|n| n.id)
    }

    /// Find the nearest ground-level node (surface type `ForestFloor`) to the
    /// given position. Returns `None` if no ground nodes exist.
    pub fn find_nearest_ground_node(&self, pos: VoxelCoord) -> Option<NavNodeId> {
        self.live_nodes()
            .filter(|n| n.surface_type == VoxelType::ForestFloor)
            .min_by_key(|n| n.position.manhattan_distance(pos))
            .map(|n| n.id)
    }

    /// Return all ground-level node IDs (surface type `ForestFloor`).
    pub fn ground_node_ids(&self) -> Vec<NavNodeId> {
        self.live_nodes()
            .filter(|n| n.surface_type == VoxelType::ForestFloor)
            .map(|n| n.id)
            .collect()
    }

    /// Number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// O(1) check whether a coordinate has a live nav node.
    pub fn has_node_at(&self, coord: VoxelCoord) -> bool {
        let (sx, sy, sz) = self.world_size;
        if sx == 0 {
            return false;
        }
        let x = coord.x as usize;
        let y = coord.y as usize;
        let z = coord.z as usize;
        if x >= sx || y >= sy || z >= sz {
            return false;
        }
        let flat = x + z * sx + y * sx * sz;
        flat < self.spatial_index.len() && self.spatial_index[flat] != u32::MAX
    }

    /// Compute the flat voxel index for a coordinate within this graph's
    /// world. Returns `None` if out of bounds.
    fn flat_index(&self, coord: VoxelCoord) -> Option<usize> {
        let (sx, sy, sz) = self.world_size;
        let x = coord.x as usize;
        let y = coord.y as usize;
        let z = coord.z as usize;
        if coord.x < 0 || coord.y < 0 || coord.z < 0 || x >= sx || y >= sy || z >= sz {
            return None;
        }
        Some(x + z * sx + y * sx * sz)
    }

    /// Incrementally update the nav graph after a single voxel changed from
    /// Air to solid (e.g. construction materialization).
    ///
    /// Only touches the changed coord + 6 face neighbors (7 positions total)
    /// and their 26-neighbor edges. Returns the IDs of nodes that were removed
    /// (callers should resnap any creatures on those nodes).
    ///
    /// **Algorithm:**
    /// 1. For each of the 7 affected positions, determine whether a nav node
    ///    should exist (Air + solid face neighbor + y≥1). Add/remove/update
    ///    nodes accordingly.
    /// 2. Collect a "dirty set" of all live nodes at affected positions plus
    ///    their live 26-neighbors.
    /// 3. Clear all edges touching dirty nodes (both directions).
    /// 4. Recompute edges between dirty nodes and their 26-neighbors.
    pub fn update_after_voxel_solidified(
        &mut self,
        world: &VoxelWorld,
        coord: VoxelCoord,
    ) -> Vec<NavNodeId> {
        let mut removed_ids = Vec::new();

        // Step 1: Determine the 7 affected positions (changed + 6 face neighbors).
        let mut affected: Vec<VoxelCoord> = Vec::with_capacity(7);
        affected.push(coord);
        for &(dx, dy, dz) in &FACE_OFFSETS {
            let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
            if self.flat_index(neighbor).is_some() {
                affected.push(neighbor);
            }
        }

        // Step 2: For each affected position, add/remove/update nav node.
        for &pos in &affected {
            let should_be_node = pos.y >= 1
                && world.get(pos) == VoxelType::Air
                && FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
                    world
                        .get(VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz))
                        .is_solid()
                });
            let flat = match self.flat_index(pos) {
                Some(f) => f,
                None => continue,
            };
            let current_slot = self.spatial_index[flat];
            let is_node = current_slot != u32::MAX;

            if should_be_node && !is_node {
                // Add new node.
                let surface = derive_surface_type(world, pos);
                let slot = if let Some(free) = self.free_slots.pop() {
                    let id = NavNodeId(free as u32);
                    self.nodes[free] = Some(NavNode {
                        id,
                        position: pos,
                        surface_type: surface,
                        edge_indices: Vec::new(),
                    });
                    free
                } else {
                    let slot = self.nodes.len();
                    let id = NavNodeId(slot as u32);
                    self.nodes.push(Some(NavNode {
                        id,
                        position: pos,
                        surface_type: surface,
                        edge_indices: Vec::new(),
                    }));
                    slot
                };
                self.spatial_index[flat] = slot as u32;
            } else if !should_be_node && is_node {
                // Remove node.
                let slot = current_slot as usize;
                let id = NavNodeId(current_slot);
                removed_ids.push(id);
                self.nodes[slot] = None;
                self.spatial_index[flat] = u32::MAX;
                self.free_slots.push(slot);
            } else if should_be_node && is_node {
                // Update surface type (solid below may have changed).
                let surface = derive_surface_type(world, pos);
                if let Some(node) = self.nodes[current_slot as usize].as_mut() {
                    node.surface_type = surface;
                }
            }
        }

        // Step 3: Collect dirty set — all live nodes at affected positions
        // plus their live 26-neighbors.
        let mut dirty_set: Vec<usize> = Vec::new();
        let mut is_dirty = vec![false; self.nodes.len()];
        for &pos in &affected {
            // Add the node at this position (if live).
            if let Some(flat) = self.flat_index(pos) {
                let slot = self.spatial_index[flat];
                if slot != u32::MAX {
                    let s = slot as usize;
                    if !is_dirty[s] {
                        is_dirty[s] = true;
                        dirty_set.push(s);
                    }
                }
            }
            // Add live 26-neighbors of this position.
            for dy in -1i32..=1 {
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let np = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
                        if let Some(nflat) = self.flat_index(np) {
                            let nslot = self.spatial_index[nflat];
                            if nslot != u32::MAX {
                                let ns = nslot as usize;
                                if ns < is_dirty.len() && !is_dirty[ns] {
                                    is_dirty[ns] = true;
                                    dirty_set.push(ns);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Step 4: Clear all edges touching dirty nodes.
        for &slot in &dirty_set {
            if let Some(node) = &self.nodes[slot] {
                let edge_indices: Vec<usize> = node.edge_indices.clone();
                for &eidx in &edge_indices {
                    let edge = &self.edges[eidx];
                    let other_slot = edge.to.0 as usize;
                    // Remove the reverse edge from the other endpoint.
                    if let Some(other_node) = self.nodes[other_slot].as_mut() {
                        other_node
                            .edge_indices
                            .retain(|&rev_idx| self.edges[rev_idx].to != NavNodeId(slot as u32));
                    }
                }
                // Clear this node's edge list.
                if let Some(node) = self.nodes[slot].as_mut() {
                    node.edge_indices.clear();
                }
            }
        }

        // Step 5: Recompute edges for dirty nodes.
        // For each dirty node, check 26 neighbors. If the neighbor is live,
        // add a bidirectional edge. To avoid duplicate edges between two dirty
        // nodes, only create the pair when processing the smaller slot index.
        for &slot in &dirty_set {
            let node = match &self.nodes[slot] {
                Some(n) => n,
                None => continue,
            };
            let pos = node.position;

            for dy in -1i32..=1 {
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let np = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
                        let nflat = match self.flat_index(np) {
                            Some(f) => f,
                            None => continue,
                        };
                        let nslot = self.spatial_index[nflat];
                        if nslot == u32::MAX {
                            continue;
                        }
                        let ns = nslot as usize;

                        // If both are dirty, only create edge from smaller slot.
                        if is_dirty[ns] && ns < slot {
                            continue;
                        }

                        let from_id = NavNodeId(slot as u32);
                        let to_id = NavNodeId(ns as u32);
                        let from_node = self.nodes[slot].as_ref().unwrap();
                        let to_node = self.nodes[ns].as_ref().unwrap();

                        let edge_type = derive_edge_type(
                            from_node.surface_type,
                            to_node.surface_type,
                            from_node.position,
                            to_node.position,
                        );
                        let dist = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                        self.add_edge(from_id, to_id, edge_type, dist);
                    }
                }
            }
        }

        removed_ids
    }
}

// ---------------------------------------------------------------------------
// Surface and edge type derivation
// ---------------------------------------------------------------------------

/// 6 face-neighbor offsets (±x, ±y, ±z).
const FACE_OFFSETS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// Determine what surface a creature at `pos` is touching.
///
/// Priority: the voxel directly below takes precedence (creature standing on
/// it). Otherwise check horizontal neighbors and above in a fixed order and
/// return the first solid type found (creature clinging to it).
///
/// This means ground-level nodes at y=1 above `ForestFloor` get surface type
/// `ForestFloor` even when they're also adjacent to the trunk — capybaras can
/// walk near the trunk base.
fn derive_surface_type(world: &VoxelWorld, pos: VoxelCoord) -> VoxelType {
    // Check below first (creature standing on this surface).
    let below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
    let below_type = world.get(below);
    if below_type.is_solid() {
        return below_type;
    }

    // Check horizontal neighbors and above in fixed order.
    let side_offsets: [(i32, i32, i32); 5] =
        [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)];
    for (dx, dy, dz) in side_offsets {
        let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
        let ntype = world.get(neighbor);
        if ntype.is_solid() {
            return ntype;
        }
    }

    // Shouldn't happen — only called for air voxels with solid face neighbors.
    VoxelType::ForestFloor
}

/// Determine the edge type for a connection between two nav nodes based on
/// their surface types and positions.
fn derive_edge_type(
    from_surface: VoxelType,
    to_surface: VoxelType,
    from_pos: VoxelCoord,
    to_pos: VoxelCoord,
) -> EdgeType {
    use VoxelType::*;

    // Same surface type on both sides.
    if from_surface == to_surface {
        return match from_surface {
            ForestFloor => EdgeType::ForestFloor,
            Trunk => {
                if from_pos.y != to_pos.y {
                    EdgeType::TrunkClimb
                } else {
                    EdgeType::TrunkCircumference
                }
            }
            Branch | Leaf | Fruit | GrownPlatform | Bridge | Root => EdgeType::BranchWalk,
            GrownStairs | GrownWall => EdgeType::TrunkClimb,
            Air => EdgeType::BranchWalk, // shouldn't happen
        };
    }

    // Mixed surface types.
    match (from_surface, to_surface) {
        (ForestFloor, Trunk) | (Trunk, ForestFloor) => EdgeType::GroundToTrunk,
        (ForestFloor, Root) | (Root, ForestFloor) => EdgeType::ForestFloor,
        (Trunk, Root) | (Root, Trunk) => EdgeType::TrunkClimb,
        (Trunk, Branch) | (Branch, Trunk) | (Trunk, Leaf) | (Leaf, Trunk) => EdgeType::TrunkClimb,
        _ => {
            // GrownStairs / GrownWall → climb-like; everything else → walk-like.
            if matches!(from_surface, GrownStairs | GrownWall)
                || matches!(to_surface, GrownStairs | GrownWall)
            {
                EdgeType::TrunkClimb
            } else {
                EdgeType::BranchWalk
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Graph construction
// ---------------------------------------------------------------------------

/// Build a navigation graph by scanning the voxel world.
///
/// **Algorithm:**
/// 1. Allocate a spatial index (`Vec<u32>`, same size as the voxel grid,
///    filled with `u32::MAX` as sentinel). This is freed after construction.
/// 2. **Pass 1 — nodes:** Iterate all voxels in flat-index order (y outer,
///    z middle, x inner). Each Air voxel with at least one solid face
///    neighbor becomes a nav node. Its surface type is derived from the
///    adjacent solid voxels.
/// 3. **Pass 2 — edges:** Iterate again. For each nav node, check the 13
///    positive-half neighbors (26-connectivity) to avoid duplicate
///    bidirectional edges. If the neighbor is also a nav node, derive the
///    edge type, compute euclidean distance, and add a bidirectional edge.
///    26-connectivity (vs face-only) is needed because the air shell around
///    thin geometry (radius-1 branches) would be disconnected with
///    face-only edges.
///
/// With the default 256×128×256 world, expect ~3000–5000 nav nodes (air
/// voxels touching the tree surface and forest floor). The spatial index
/// is ~32 MB and freed after construction.
pub fn build_nav_graph(world: &VoxelWorld) -> NavGraph {
    let mut graph = NavGraph::new();

    let sx = world.size_x as usize;
    let sy = world.size_y as usize;
    let sz = world.size_z as usize;
    let total = sx * sy * sz;

    if total == 0 {
        return graph;
    }

    graph.world_size = (sx, sy, sz);
    graph.spatial_index = vec![u32::MAX; total];

    // --- Pass 1: create nav nodes ---
    // Start at y=1: y=0 is the floor layer (ForestFloor), so air at y=0
    // only exists at the floor boundary and creates disconnected artifacts.
    // Creatures walk ON the floor (y=1), not beside it.
    for y in 1..sy {
        for z in 0..sz {
            for x in 0..sx {
                let coord = VoxelCoord::new(x as i32, y as i32, z as i32);
                if world.get(coord) != VoxelType::Air {
                    continue;
                }

                // Check if any face neighbor is solid.
                let has_solid_neighbor = FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
                    let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                    world.get(neighbor).is_solid()
                });

                if has_solid_neighbor {
                    let surface = derive_surface_type(world, coord);
                    let node_id = graph.add_node(coord, surface);
                    let flat_idx = x + z * sx + y * sx * sz;
                    graph.spatial_index[flat_idx] = node_id.0;
                }
            }
        }
    }

    // --- Pass 2: create edges ---
    // Use 26-connectivity (13 positive-half neighbors) to ensure the air
    // shell around thin geometry (radius-1 branches) stays connected.
    // The "positive half" is the set of 13 offsets where the first nonzero
    // component (checking x, then y, then z) is positive. Each generates a
    // bidirectional edge, covering all 26 directions without duplicates.
    #[rustfmt::skip]
    let positive_half: [(i32, i32, i32); 13] = [
        // dx > 0 (9 offsets)
        ( 1, -1, -1), ( 1, -1,  0), ( 1, -1,  1),
        ( 1,  0, -1), ( 1,  0,  0), ( 1,  0,  1),
        ( 1,  1, -1), ( 1,  1,  0), ( 1,  1,  1),
        // dx == 0, dy > 0 (3 offsets)
        ( 0,  1, -1), ( 0,  1,  0), ( 0,  1,  1),
        // dx == 0, dy == 0, dz > 0 (1 offset)
        ( 0,  0,  1),
    ];
    for y in 1..sy {
        for z in 0..sz {
            for x in 0..sx {
                let flat_idx = x + z * sx + y * sx * sz;
                let from_id = graph.spatial_index[flat_idx];
                if from_id == u32::MAX {
                    continue;
                }

                for &(dx, dy, dz) in &positive_half {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    let nz = z as i32 + dz;

                    if nx < 0 || ny < 0 || nz < 0 {
                        continue;
                    }
                    let (nxu, nyu, nzu) = (nx as usize, ny as usize, nz as usize);
                    if nxu >= sx || nyu >= sy || nzu >= sz {
                        continue;
                    }

                    let n_flat = nxu + nzu * sx + nyu * sx * sz;
                    let to_id = graph.spatial_index[n_flat];
                    if to_id == u32::MAX {
                        continue;
                    }

                    let from = NavNodeId(from_id);
                    let to = NavNodeId(to_id);
                    let from_node = graph.node(from);
                    let to_node = graph.node(to);

                    let edge_type = derive_edge_type(
                        from_node.surface_type,
                        to_node.surface_type,
                        from_node.position,
                        to_node.position,
                    );

                    let dist = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                    graph.add_edge(from, to, edge_type, dist);
                }
            }
        }
    }

    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- NavGraph unit tests ---

    #[test]
    fn add_node_assigns_sequential_ids() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        let b = graph.add_node(VoxelCoord::new(1, 0, 0), VoxelType::ForestFloor);
        let c = graph.add_node(VoxelCoord::new(2, 0, 0), VoxelType::Trunk);
        assert_eq!(a, NavNodeId(0));
        assert_eq!(b, NavNodeId(1));
        assert_eq!(c, NavNodeId(2));
        assert_eq!(graph.node_count(), 3);
    }

    #[test]
    fn add_edge_creates_bidirectional() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        let b = graph.add_node(VoxelCoord::new(5, 0, 0), VoxelType::ForestFloor);
        graph.add_edge(a, b, EdgeType::ForestFloor, 5.0);

        let a_edges: Vec<_> = graph
            .neighbors(a)
            .iter()
            .map(|&idx| graph.edge(idx).to)
            .collect();
        assert_eq!(a_edges, vec![b]);

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
        graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        graph.add_node(VoxelCoord::new(10, 0, 0), VoxelType::ForestFloor);
        graph.add_node(VoxelCoord::new(5, 5, 0), VoxelType::Trunk);

        let nearest = graph.find_nearest_node(VoxelCoord::new(4, 4, 0));
        assert_eq!(nearest, Some(NavNodeId(2))); // (5,5,0) is closest
    }

    #[test]
    fn find_nearest_node_empty_graph() {
        let graph = NavGraph::new();
        assert_eq!(graph.find_nearest_node(VoxelCoord::new(0, 0, 0)), None);
    }

    #[test]
    fn ground_node_ids_filters_by_surface_type() {
        let mut graph = NavGraph::new();
        graph.add_node(VoxelCoord::new(0, 1, 0), VoxelType::ForestFloor);
        graph.add_node(VoxelCoord::new(1, 5, 0), VoxelType::Trunk);
        graph.add_node(VoxelCoord::new(2, 1, 0), VoxelType::ForestFloor);

        let ground = graph.ground_node_ids();
        assert_eq!(ground.len(), 2);
        assert_eq!(ground[0], NavNodeId(0));
        assert_eq!(ground[1], NavNodeId(2));
    }

    #[test]
    fn find_nearest_ground_node_filters_by_surface_type() {
        let mut graph = NavGraph::new();
        graph.add_node(VoxelCoord::new(0, 1, 0), VoxelType::ForestFloor);
        graph.add_node(VoxelCoord::new(1, 1, 0), VoxelType::Trunk);
        graph.add_node(VoxelCoord::new(5, 1, 0), VoxelType::ForestFloor);

        // Closest overall is the Trunk node, but ground search skips it.
        let nearest = graph.find_nearest_ground_node(VoxelCoord::new(1, 1, 0));
        assert_eq!(nearest, Some(NavNodeId(0)));
    }

    // --- Surface type derivation tests ---

    #[test]
    fn surface_type_standing_on_floor() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(4, 0, 4), VoxelType::ForestFloor);
        // Air at y=1 is above ForestFloor.
        let surface = derive_surface_type(&world, VoxelCoord::new(4, 1, 4));
        assert_eq!(surface, VoxelType::ForestFloor);
    }

    #[test]
    fn surface_type_standing_on_trunk() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(4, 5, 4), VoxelType::Trunk);
        // Air at y=6 is above trunk.
        let surface = derive_surface_type(&world, VoxelCoord::new(4, 6, 4));
        assert_eq!(surface, VoxelType::Trunk);
    }

    #[test]
    fn surface_type_clinging_to_trunk() {
        let mut world = VoxelWorld::new(8, 8, 8);
        // Trunk at x=3, air at x=4 clinging to trunk side.
        world.set(VoxelCoord::new(3, 5, 4), VoxelType::Trunk);
        let surface = derive_surface_type(&world, VoxelCoord::new(4, 5, 4));
        // Nothing below, but trunk to the -x side.
        assert_eq!(surface, VoxelType::Trunk);
    }

    #[test]
    fn surface_type_floor_takes_priority_over_trunk() {
        // Node at y=1 with ForestFloor below and Trunk to the side — should
        // be ForestFloor (standing on it takes priority).
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(4, 0, 4), VoxelType::ForestFloor);
        world.set(VoxelCoord::new(3, 1, 4), VoxelType::Trunk);
        let surface = derive_surface_type(&world, VoxelCoord::new(4, 1, 4));
        assert_eq!(surface, VoxelType::ForestFloor);
    }

    // --- Edge type derivation tests ---

    #[test]
    fn edge_type_forest_floor() {
        let et = derive_edge_type(
            VoxelType::ForestFloor,
            VoxelType::ForestFloor,
            VoxelCoord::new(0, 1, 0),
            VoxelCoord::new(1, 1, 0),
        );
        assert_eq!(et, EdgeType::ForestFloor);
    }

    #[test]
    fn edge_type_trunk_climb() {
        let et = derive_edge_type(
            VoxelType::Trunk,
            VoxelType::Trunk,
            VoxelCoord::new(4, 5, 4),
            VoxelCoord::new(4, 6, 4),
        );
        assert_eq!(et, EdgeType::TrunkClimb);
    }

    #[test]
    fn edge_type_trunk_circumference() {
        let et = derive_edge_type(
            VoxelType::Trunk,
            VoxelType::Trunk,
            VoxelCoord::new(4, 5, 4),
            VoxelCoord::new(4, 5, 5),
        );
        assert_eq!(et, EdgeType::TrunkCircumference);
    }

    #[test]
    fn edge_type_ground_to_trunk() {
        let et = derive_edge_type(
            VoxelType::ForestFloor,
            VoxelType::Trunk,
            VoxelCoord::new(4, 1, 4),
            VoxelCoord::new(4, 2, 4),
        );
        assert_eq!(et, EdgeType::GroundToTrunk);
    }

    #[test]
    fn edge_type_trunk_to_branch() {
        let et = derive_edge_type(
            VoxelType::Trunk,
            VoxelType::Branch,
            VoxelCoord::new(4, 10, 4),
            VoxelCoord::new(5, 10, 4),
        );
        assert_eq!(et, EdgeType::TrunkClimb);
    }

    #[test]
    fn edge_type_branch_walk() {
        let et = derive_edge_type(
            VoxelType::Branch,
            VoxelType::Branch,
            VoxelCoord::new(10, 20, 4),
            VoxelCoord::new(11, 20, 4),
        );
        assert_eq!(et, EdgeType::BranchWalk);
    }

    // --- build_nav_graph integration tests ---

    /// Helper: create a VoxelWorld with a generated tree.
    /// Uses the default fantasy_mega profile with leaves disabled and
    /// a small 64^3 world.
    fn test_world() -> VoxelWorld {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        // Disable leaves for basic nav tests.
        config.tree_profile.leaves.canopy_density = 0.0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

        world
    }

    #[test]
    fn nav_nodes_are_air_voxels() {
        let world = test_world();
        let graph = build_nav_graph(&world);

        for node in graph.live_nodes() {
            assert_eq!(
                world.get(node.position),
                VoxelType::Air,
                "Nav node at {:?} should be air, got {:?}",
                node.position,
                world.get(node.position),
            );
        }
    }

    #[test]
    fn nav_nodes_adjacent_to_solid() {
        let world = test_world();
        let graph = build_nav_graph(&world);

        for node in graph.live_nodes() {
            let has_solid = FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
                let n = VoxelCoord::new(
                    node.position.x + dx,
                    node.position.y + dy,
                    node.position.z + dz,
                );
                world.get(n).is_solid()
            });
            assert!(
                has_solid,
                "Nav node at {:?} has no solid face neighbor",
                node.position,
            );
        }
    }

    #[test]
    fn build_nav_graph_has_ground_nodes() {
        let world = test_world();
        let graph = build_nav_graph(&world);

        let ground_nodes: Vec<_> = graph
            .live_nodes()
            .filter(|n| n.surface_type == VoxelType::ForestFloor)
            .collect();
        assert!(
            ground_nodes.len() >= 4,
            "Expected at least 4 ground nodes, got {}",
            ground_nodes.len(),
        );
        // Ground nodes should be at y=1 (air above ForestFloor at y=0).
        for n in &ground_nodes {
            assert_eq!(
                n.position.y, 1,
                "Ground node should be at y=1, got y={}",
                n.position.y,
            );
        }
    }

    #[test]
    fn build_nav_graph_has_trunk_nodes() {
        let world = test_world();
        let graph = build_nav_graph(&world);

        let trunk_nodes: Vec<_> = graph
            .live_nodes()
            .filter(|n| n.surface_type == VoxelType::Trunk)
            .collect();
        assert!(!trunk_nodes.is_empty(), "Should have trunk surface nodes");
    }

    #[test]
    fn build_nav_graph_is_connected() {
        let world = test_world();
        let graph = build_nav_graph(&world);

        assert!(graph.node_count() > 0, "Graph should have nodes");

        // BFS flood fill from node 0 — all nodes should be reachable.
        let n = graph.node_count();
        let mut visited = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        visited[0] = true;
        queue.push_back(NavNodeId(0));

        while let Some(current) = queue.pop_front() {
            for &edge_idx in graph.neighbors(current) {
                let neighbor = graph.edge(edge_idx).to;
                let ni = neighbor.0 as usize;
                if !visited[ni] {
                    visited[ni] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        let unreachable: Vec<_> = visited
            .iter()
            .enumerate()
            .filter(|&(_, v)| !v)
            .map(|(i, _)| {
                let node = graph.node(NavNodeId(i as u32));
                (i, node.position, node.surface_type)
            })
            .collect();

        assert!(
            unreachable.is_empty(),
            "Found {} unreachable nodes (out of {}). First 10: {:?}",
            unreachable.len(),
            n,
            &unreachable[..unreachable.len().min(10)],
        );
    }

    #[test]
    fn build_nav_graph_is_connected_with_splits() {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        // High split chance to test connectivity with many branches.
        config.tree_profile.split.split_chance_base = 1.0;
        config.tree_profile.split.min_progress_for_split = 0.05;
        config.tree_profile.leaves.canopy_density = 0.0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

        let graph = build_nav_graph(&world);
        assert!(graph.node_count() > 0);

        // BFS flood fill from node 0.
        let n = graph.node_count();
        let mut visited = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        visited[0] = true;
        queue.push_back(NavNodeId(0));

        while let Some(current) = queue.pop_front() {
            for &edge_idx in graph.neighbors(current) {
                let neighbor = graph.edge(edge_idx).to;
                let ni = neighbor.0 as usize;
                if !visited[ni] {
                    visited[ni] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        let unreachable_count = visited.iter().filter(|&&v| !v).count();
        assert!(
            unreachable_count == 0,
            "Found {unreachable_count} unreachable nodes (out of {n})",
        );
    }

    #[test]
    fn voxel_nav_determinism() {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };

        // Build two graphs from the same seed.
        let mut world_a = VoxelWorld::new(64, 64, 64);
        let mut rng_a = GameRng::new(42);
        tree_gen::generate_tree(&mut world_a, &config, &mut rng_a);
        let graph_a = build_nav_graph(&world_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(42);
        tree_gen::generate_tree(&mut world_b, &config, &mut rng_b);
        let graph_b = build_nav_graph(&world_b);

        assert_eq!(graph_a.node_count(), graph_b.node_count());
        assert_eq!(graph_a.edge_count(), graph_b.edge_count());

        for i in 0..graph_a.node_count() {
            let na = graph_a.node(NavNodeId(i as u32));
            let nb = graph_b.node(NavNodeId(i as u32));
            assert_eq!(na.position, nb.position);
            assert_eq!(na.surface_type, nb.surface_type);
        }
    }

    #[test]
    fn nav_graph_connected_with_leaves() {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

        let graph = build_nav_graph(&world);
        assert!(graph.node_count() > 0);

        // BFS flood fill from node 0.
        let n = graph.node_count();
        let mut visited = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        visited[0] = true;
        queue.push_back(NavNodeId(0));

        while let Some(current) = queue.pop_front() {
            for &edge_idx in graph.neighbors(current) {
                let neighbor = graph.edge(edge_idx).to;
                let ni = neighbor.0 as usize;
                if !visited[ni] {
                    visited[ni] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        let unreachable_count = visited.iter().filter(|&&v| !v).count();
        assert!(
            unreachable_count == 0,
            "Found {unreachable_count} unreachable nodes (out of {n}) with leaves enabled",
        );
    }

    #[test]
    fn nav_graph_connected_with_roots() {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        config.tree_profile.roots.root_energy_fraction = 0.2;
        config.tree_profile.roots.root_initial_count = 5;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

        let graph = build_nav_graph(&world);
        assert!(graph.node_count() > 0);

        // BFS flood fill from node 0.
        let n = graph.node_count();
        let mut visited = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        visited[0] = true;
        queue.push_back(NavNodeId(0));

        while let Some(current) = queue.pop_front() {
            for &edge_idx in graph.neighbors(current) {
                let neighbor = graph.edge(edge_idx).to;
                let ni = neighbor.0 as usize;
                if !visited[ni] {
                    visited[ni] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        let unreachable_count = visited.iter().filter(|&&v| !v).count();
        assert!(
            unreachable_count == 0,
            "Found {unreachable_count} unreachable nodes (out of {n}) with roots enabled",
        );
    }

    // --- Incremental update tests ---

    /// Helper: build a small world with a 3x1 platform at y=0, producing nav
    /// nodes at y=1. World size 8x8x8 keeps tests fast.
    fn platform_world() -> VoxelWorld {
        let mut world = VoxelWorld::new(8, 8, 8);
        // Solid floor at y=0: (3,0,3), (4,0,3), (5,0,3)
        world.set(VoxelCoord::new(3, 0, 3), VoxelType::ForestFloor);
        world.set(VoxelCoord::new(4, 0, 3), VoxelType::ForestFloor);
        world.set(VoxelCoord::new(5, 0, 3), VoxelType::ForestFloor);
        world
    }

    #[test]
    fn incremental_update_removes_solidified_node() {
        let mut world = platform_world();
        let mut graph = build_nav_graph(&world);

        // There should be a nav node at (4,1,3) — air above floor.
        assert!(
            graph.has_node_at(VoxelCoord::new(4, 1, 3)),
            "Expected nav node at (4,1,3) before solidification",
        );

        // Solidify (4,1,3) — it should no longer be a nav node.
        world.set(VoxelCoord::new(4, 1, 3), VoxelType::GrownPlatform);
        let removed = graph.update_after_voxel_solidified(&world, VoxelCoord::new(4, 1, 3));

        assert!(
            !graph.has_node_at(VoxelCoord::new(4, 1, 3)),
            "Nav node at (4,1,3) should be removed after solidification",
        );
        // The solidified position was a nav node, so it must be in the removed list.
        assert!(!removed.is_empty(), "Should have removed at least one node");
    }

    #[test]
    fn incremental_update_adds_new_neighbor_nodes() {
        let mut world = platform_world();
        let mut graph = build_nav_graph(&world);

        // (4,1,3) is a nav node (air above floor). Solidifying it should
        // create a new nav node at (4,2,3) — air above the new solid.
        assert!(
            !graph.has_node_at(VoxelCoord::new(4, 2, 3)),
            "No nav node at (4,2,3) before solidification",
        );

        world.set(VoxelCoord::new(4, 1, 3), VoxelType::GrownPlatform);
        graph.update_after_voxel_solidified(&world, VoxelCoord::new(4, 1, 3));

        assert!(
            graph.has_node_at(VoxelCoord::new(4, 2, 3)),
            "Should have a nav node at (4,2,3) after solidification — air above new solid",
        );
    }

    #[test]
    fn incremental_update_matches_full_rebuild() {
        let mut world = platform_world();
        let mut graph = build_nav_graph(&world);

        // Solidify (4,1,3).
        world.set(VoxelCoord::new(4, 1, 3), VoxelType::GrownPlatform);
        graph.update_after_voxel_solidified(&world, VoxelCoord::new(4, 1, 3));

        // Full rebuild on the same world state.
        let rebuilt = build_nav_graph(&world);

        // Compare node positions (order-independent).
        let mut inc_positions: Vec<VoxelCoord> = graph.live_nodes().map(|n| n.position).collect();
        let mut full_positions: Vec<VoxelCoord> =
            rebuilt.live_nodes().map(|n| n.position).collect();
        inc_positions.sort();
        full_positions.sort();
        assert_eq!(
            inc_positions, full_positions,
            "Incremental and full rebuild should produce the same node positions",
        );

        // Compare edge connectivity (by position pairs, order-independent).
        let mut inc_edges: Vec<(VoxelCoord, VoxelCoord)> = Vec::new();
        for node in graph.live_nodes() {
            for &edge_idx in &node.edge_indices {
                let edge = graph.edge(edge_idx);
                let from_pos = graph.node(edge.from).position;
                let to_pos = graph.node(edge.to).position;
                inc_edges.push((from_pos, to_pos));
            }
        }
        let mut full_edges: Vec<(VoxelCoord, VoxelCoord)> = Vec::new();
        for node in rebuilt.live_nodes() {
            for &edge_idx in &node.edge_indices {
                let edge = rebuilt.edge(edge_idx);
                let from_pos = rebuilt.node(edge.from).position;
                let to_pos = rebuilt.node(edge.to).position;
                full_edges.push((from_pos, to_pos));
            }
        }
        inc_edges.sort();
        full_edges.sort();
        assert_eq!(
            inc_edges, full_edges,
            "Incremental and full rebuild should produce the same edges",
        );
    }
}
