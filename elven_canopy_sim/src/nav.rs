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
// **Span-scan + BFS construction:** Instead of scanning every voxel in the
// world, `build_nav_graph()` uses a unified BFS approach:
// (1) Span-scan: for each (x, z) column, reads RLE spans and extracts seed
//     positions from span boundaries — where air starts after solid, and
//     non-solid non-air spans (BuildingInterior, ladders) which emit every
//     voxel as a seed. NO horizontal neighbor seeds are generated — the BFS
//     discovers them. At NO POINT does it iterate through Y ranges of solid
//     material (dirt, trunk, etc.).
// (2) Seeds are filtered through `should_be_nav_node()` and inserted as
//     nodes in the graph.
// (3) BFS from seeds with 26-connectivity: discovers additional nav nodes
//     AND creates edges in a single pass. For each node popped, all 26
//     neighbors are checked. Existing graph nodes get edges; new valid
//     positions are inserted and pushed onto the queue.
// This reduces node discovery from O(world_volume) to O(number_of_spans +
// BFS_frontier). Every air voxel that is face-adjacent to at least one
// solid voxel becomes a nav node. `BuildingInterior` voxels are always nav
// nodes (face data provides surfaces). Edges connect 26-neighbors among
// nav nodes, subject to face-blocking checks (`is_edge_blocked_by_faces()`).
// This means the nav graph reflects actual world geometry — construction
// changes the navigable topology via incremental updates.
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
// `free_slots`. A persistent `spatial_index` (`LookupMap<VoxelCoord, u32>`)
// enables O(1) coord→node lookup for both incremental updates and
// `has_node_at()` queries.
//
// The full `build_nav_graph()` is used at startup and save/load. During
// gameplay, `materialize_next_build_voxel()` in `sim/construction.rs` calls
// `update_after_voxel_solidified()` which touches only ~7 positions and their
// 26-neighbor edges — O(1) instead of O(world_size).
//
// **Dual-graph pattern for large creatures.** `build_large_nav_graph()` builds
// a separate `NavGraph` for 2x2x2 footprint creatures (e.g. elephants). Nodes
// exist at anchor `(x, y, z)` where the 2x2 ground footprint has solid voxels
// within 1 voxel of height variation and 2 voxels of air clearance above the
// highest ground point. Edges connect 8-neighbors with union-footprint
// clearance checks, allowing up to 1 voxel of height change between adjacent
// nodes. The large graph uses the same `NavGraph` struct so all existing
// pathfinding code works unchanged. `sim/mod.rs` stores both graphs and dispatches
// via `graph_for_species()` based on the species' footprint.
//
// Nodes and edges use `Vec` indexed by `NavNodeId`/`NavEdgeId` for O(1) lookup
// and deterministic iteration order. The spatial index uses `LookupMap` (a
// non-iterable `HashMap` wrapper) for O(1) coord→node lookup without
// allocating 4 bytes per voxel in the entire world volume.
//
// See also: `world.rs` for the voxel grid, `tree_gen.rs` for tree geometry,
// `pathfinding.rs` for A* search over this graph, `sim/mod.rs` which owns the
// `NavGraph` as part of `SimState`, `species.rs` for the `footprint` field.
//
// **Critical constraint: determinism.** The graph is built deterministically:
// seeds are produced in (z, x) column order, BFS uses a FIFO queue with a
// fixed 26-direction array, so node IDs are assigned in deterministic BFS
// discovery order. Incremental updates are also deterministic — they process
// affected positions in a fixed order.

use crate::lookup_map::LookupMap;
use crate::types::{
    FaceData, FaceDirection, FaceType, NavEdgeId, NavNodeId, VoxelCoord, VoxelType,
};
use crate::world::VoxelWorld;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// A directed edge in the navigation graph.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavEdge {
    pub id: NavEdgeId,
    pub from: NavNodeId,
    pub to: NavNodeId,
    pub edge_type: EdgeType,
    /// Euclidean distance × DIST_SCALE (1024). Integer-only, deterministic.
    /// For adjacent horizontal (1,0,0): 1024.
    /// For diagonal (1,1,0): 1448 (≈ sqrt(2) × 1024).
    /// For 3D diagonal (1,1,1): 1774 (≈ sqrt(3) × 1024).
    pub distance: u32,
}

/// Compute a scaled integer Euclidean distance from coordinate deltas.
/// Returns `floor(sqrt(dx² + dy² + dz²) * DIST_SCALE)`, computed with
/// integer square root for determinism (no floats).
pub fn scaled_distance(dx: i32, dy: i32, dz: i32) -> u32 {
    let sq = (dx as i64 * dx as i64 + dy as i64 * dy as i64 + dz as i64 * dz as i64) as u64;
    let scaled_sq = sq * (DIST_SCALE as u64 * DIST_SCALE as u64);
    scaled_sq.isqrt() as u32
}

/// The navigation graph container.
///
/// Nodes are stored as `Option<NavNode>` slots — `Some` for live nodes, `None`
/// for removed nodes. This allows incremental updates (removing/adding nodes)
/// without shifting IDs. The `spatial_index` maps `VoxelCoord → node slot`
/// via `LookupMap` for O(1) coord→node lookup; `free_slots` tracks
/// recyclable slots. Using `LookupMap` (non-iterable `HashMap` wrapper)
/// instead of a flat `Vec<u32>` avoids allocating 4 bytes per voxel in the
/// entire world volume — only actual nav nodes consume space.
#[derive(Clone, Debug, Default)]
pub struct NavGraph {
    nodes: Vec<Option<NavNode>>,
    pub edges: Vec<NavEdge>,
    spatial_index: LookupMap<VoxelCoord, u32>,
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
        distance: u32,
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

    /// Check if a node ID refers to a live (non-removed) node.
    pub fn is_node_alive(&self, id: NavNodeId) -> bool {
        let idx = id.0 as usize;
        idx < self.nodes.len() && self.nodes[idx].is_some()
    }

    /// Kill a node slot (set it to `None`). Used in tests to simulate
    /// incremental updates that leave dead slots without recycling them.
    #[cfg(test)]
    pub fn kill_node(&mut self, id: NavNodeId) {
        let idx = id.0 as usize;
        if idx < self.nodes.len() {
            self.nodes[idx] = None;
        }
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

    /// Find up to `count` distinct nav nodes near `center`, expanding outward
    /// via BFS. The center node is always the first result. Used by group move
    /// commands to spread creatures across nearby positions instead of stacking
    /// them all on the same voxel.
    ///
    /// Returns at most `count` node IDs. If the graph has fewer reachable nodes
    /// than requested, returns as many as it can find.
    pub fn spread_destinations(&self, center: NavNodeId, count: usize) -> Vec<NavNodeId> {
        if count == 0 || !self.is_node_alive(center) {
            return Vec::new();
        }
        let mut result = Vec::with_capacity(count);
        result.push(center);
        if count == 1 {
            return result;
        }

        // BFS outward from center.
        let mut visited = vec![false; self.nodes.len()];
        visited[center.0 as usize] = true;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(center);

        while let Some(node_id) = queue.pop_front() {
            for &edge_idx in self.neighbors(node_id) {
                let neighbor = self.edges[edge_idx].to;
                let slot = neighbor.0 as usize;
                if !visited[slot] && self.is_node_alive(neighbor) {
                    visited[slot] = true;
                    result.push(neighbor);
                    if result.len() >= count {
                        return result;
                    }
                    queue.push_back(neighbor);
                }
            }
        }

        result
    }

    /// Number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// O(1) check whether a coordinate has a live nav node.
    pub fn has_node_at(&self, coord: VoxelCoord) -> bool {
        self.spatial_index.contains_key(&coord)
    }

    /// O(1) lookup: return the `NavNodeId` at a coordinate, or `None`.
    pub fn node_at(&self, coord: VoxelCoord) -> Option<NavNodeId> {
        self.spatial_index.get(&coord).map(|&slot| NavNodeId(slot))
    }

    /// Find the edge index from `from` to `to` (linear scan of `from`'s
    /// neighbor list). Returns `None` if no such edge exists.
    pub fn find_edge_to(&self, from: NavNodeId, to: NavNodeId) -> Option<usize> {
        self.neighbors(from)
            .iter()
            .copied()
            .find(|&idx| self.edges[idx].to == to)
    }

    /// Check whether a coordinate is within this graph's world bounds.
    fn in_bounds(&self, coord: VoxelCoord) -> bool {
        let (sx, sy, sz) = self.world_size;
        coord.x >= 0
            && coord.y >= 0
            && coord.z >= 0
            && (coord.x as usize) < sx
            && (coord.y as usize) < sy
            && (coord.z as usize) < sz
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
        face_data: &BTreeMap<VoxelCoord, FaceData>,
        coord: VoxelCoord,
    ) -> Vec<NavNodeId> {
        let mut removed_ids = Vec::new();

        // Step 1: Determine the 7 affected positions (changed + 6 face neighbors).
        let mut affected: Vec<VoxelCoord> = Vec::with_capacity(7);
        affected.push(coord);
        for &(dx, dy, dz) in &FACE_OFFSETS {
            let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
            if self.in_bounds(neighbor) {
                affected.push(neighbor);
            }
        }

        // Step 2: For each affected position, add/remove/update nav node.
        for &pos in &affected {
            let should_be_node = should_be_nav_node(world, face_data, pos);
            if !self.in_bounds(pos) {
                continue;
            }
            let current_slot = self.spatial_index.get(&pos).copied();
            let is_node = current_slot.is_some();

            if should_be_node && !is_node {
                // Add new node.
                let surface = derive_surface_type(world, face_data, pos);
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
                self.spatial_index.insert(pos, slot as u32);
            } else if !should_be_node && is_node {
                // Remove node.
                let slot_val = current_slot.unwrap();
                let slot = slot_val as usize;
                removed_ids.push(NavNodeId(slot_val));
                self.nodes[slot] = None;
                self.spatial_index.remove(&pos);
                self.free_slots.push(slot);
            } else if should_be_node && is_node {
                // Update surface type (solid below may have changed).
                let surface = derive_surface_type(world, face_data, pos);
                let slot_val = current_slot.unwrap();
                if let Some(node) = self.nodes[slot_val as usize].as_mut() {
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
            if let Some(&slot) = self.spatial_index.get(&pos) {
                let s = slot as usize;
                if !is_dirty[s] {
                    is_dirty[s] = true;
                    dirty_set.push(s);
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
                        if let Some(&nslot) = self.spatial_index.get(&np) {
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
            let (pos, from_surface) = match &self.nodes[slot] {
                Some(n) => (n.position, n.surface_type),
                None => continue,
            };

            for dy in -1i32..=1 {
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let np = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
                        let nslot = match self.spatial_index.get(&np).copied() {
                            Some(s) => s,
                            None => continue,
                        };
                        let ns = nslot as usize;

                        // If both are dirty, only create edge from smaller slot.
                        if is_dirty[ns] && ns < slot {
                            continue;
                        }

                        // Check if face data blocks this edge.
                        if is_edge_blocked_by_faces(face_data, pos, np) {
                            continue;
                        }

                        let to_node = self.nodes[ns].as_ref().unwrap();
                        let edge_type = derive_edge_type(
                            from_surface,
                            to_node.surface_type,
                            pos,
                            to_node.position,
                        );
                        let dist = scaled_distance(dx, dy, dz);
                        self.add_edge(NavNodeId(slot as u32), NavNodeId(nslot), edge_type, dist);
                    }
                }
            }
        }

        removed_ids
    }

    /// Incrementally update the nav graph after a `BuildingInterior` voxel
    /// is placed at `coord`.
    ///
    /// Unlike `update_after_voxel_solidified` (which handles solid voxels that
    /// remove their own node), building interior voxels are passable — so this
    /// method creates/updates the node at `coord` rather than removing it.
    /// Same 7-position + dirty-set + edge recomputation structure, but uses
    /// `should_be_nav_node` and `is_edge_blocked_by_faces`.
    ///
    /// Returns the IDs of nodes that were removed (callers should resnap any
    /// creatures on those nodes).
    pub fn update_after_building_voxel_set(
        &mut self,
        world: &VoxelWorld,
        face_data: &BTreeMap<VoxelCoord, FaceData>,
        coord: VoxelCoord,
    ) -> Vec<NavNodeId> {
        let mut removed_ids = Vec::new();

        // Step 1: Determine the 7 affected positions (changed + 6 face neighbors).
        let mut affected: Vec<VoxelCoord> = Vec::with_capacity(7);
        affected.push(coord);
        for &(dx, dy, dz) in &FACE_OFFSETS {
            let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
            if self.in_bounds(neighbor) {
                affected.push(neighbor);
            }
        }

        // Step 2: For each affected position, add/remove/update nav node.
        for &pos in &affected {
            let should_exist = should_be_nav_node(world, face_data, pos);
            if !self.in_bounds(pos) {
                continue;
            }
            let current_slot = self.spatial_index.get(&pos).copied();
            let is_node = current_slot.is_some();

            if should_exist && !is_node {
                let surface = derive_surface_type(world, face_data, pos);
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
                self.spatial_index.insert(pos, slot as u32);
            } else if !should_exist && is_node {
                let slot_val = current_slot.unwrap();
                let slot = slot_val as usize;
                removed_ids.push(NavNodeId(slot_val));
                self.nodes[slot] = None;
                self.spatial_index.remove(&pos);
                self.free_slots.push(slot);
            } else if should_exist && is_node {
                let surface = derive_surface_type(world, face_data, pos);
                let slot_val = current_slot.unwrap();
                if let Some(node) = self.nodes[slot_val as usize].as_mut() {
                    node.surface_type = surface;
                }
            }
        }

        // Steps 3-5: Same dirty-set + edge recomputation as
        // update_after_voxel_solidified.
        let mut dirty_set: Vec<usize> = Vec::new();
        let mut is_dirty = vec![false; self.nodes.len()];
        for &pos in &affected {
            if let Some(&slot) = self.spatial_index.get(&pos) {
                let s = slot as usize;
                if !is_dirty[s] {
                    is_dirty[s] = true;
                    dirty_set.push(s);
                }
            }
            for dy in -1i32..=1 {
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let np = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
                        if let Some(&nslot) = self.spatial_index.get(&np) {
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

        for &slot in &dirty_set {
            if let Some(node) = &self.nodes[slot] {
                let edge_indices: Vec<usize> = node.edge_indices.clone();
                for &eidx in &edge_indices {
                    let edge = &self.edges[eidx];
                    let other_slot = edge.to.0 as usize;
                    if let Some(other_node) = self.nodes[other_slot].as_mut() {
                        other_node
                            .edge_indices
                            .retain(|&rev_idx| self.edges[rev_idx].to != NavNodeId(slot as u32));
                    }
                }
                if let Some(node) = self.nodes[slot].as_mut() {
                    node.edge_indices.clear();
                }
            }
        }

        for &slot in &dirty_set {
            let (pos, from_surface) = match &self.nodes[slot] {
                Some(n) => (n.position, n.surface_type),
                None => continue,
            };

            for dy in -1i32..=1 {
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 && dz == 0 {
                            continue;
                        }
                        let np = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
                        let nslot = match self.spatial_index.get(&np).copied() {
                            Some(s) => s,
                            None => continue,
                        };
                        let ns = nslot as usize;

                        if is_dirty[ns] && ns < slot {
                            continue;
                        }

                        if is_edge_blocked_by_faces(face_data, pos, np) {
                            continue;
                        }

                        let to_node = self.nodes[ns].as_ref().unwrap();
                        let edge_type = derive_edge_type(
                            from_surface,
                            to_node.surface_type,
                            pos,
                            to_node.position,
                        );
                        let dist = scaled_distance(dx, dy, dz);
                        self.add_edge(NavNodeId(slot as u32), NavNodeId(nslot), edge_type, dist);
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

/// Determine whether a voxel at `pos` should be a nav node.
///
/// Rules:
/// - y < 1 or solid → false
/// - `BuildingInterior` → always true (face data provides surfaces)
/// - Air with a solid face neighbor → true (existing behavior)
/// - Air next to a `BuildingInterior` neighbor whose blocking face points
///   toward `pos` → true (the face acts as a virtual solid surface)
/// - Otherwise → false
fn should_be_nav_node(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
) -> bool {
    if pos.y < 1 {
        return false;
    }
    let voxel = world.get(pos);
    if voxel.is_solid() {
        return false;
    }
    if voxel == VoxelType::BuildingInterior || voxel.is_ladder() {
        return true;
    }
    // Air voxel: check face neighbors for solid or blocking building faces.
    FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
        let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
        let ntype = world.get(neighbor);
        if ntype.is_solid() {
            return true;
        }
        // Check if neighbor is BuildingInterior with a blocking face toward us.
        if ntype == VoxelType::BuildingInterior
            && let Some(fd) = face_data.get(&neighbor)
        {
            // The face on the neighbor pointing toward pos.
            let dir = FaceDirection::from_offset(-dx, -dy, -dz);
            if let Some(d) = dir {
                return fd.get(d).blocks_movement();
            }
        }
        false
    })
}

/// Determine what surface a creature at `pos` is touching.
///
/// Priority: the voxel directly below takes precedence (creature standing on
/// it). Otherwise check horizontal neighbors and above in a fixed order and
/// return the first solid type found (creature clinging to it).
///
/// For `BuildingInterior` voxels, face data determines the surface type:
/// - Floor face → `GrownPlatform` (walkable)
/// - Wall/Window side → `GrownWall` (climbable)
/// - Ceiling face → `GrownPlatform` (walkable on top)
///
/// For Air voxels next to `BuildingInterior` with blocking faces, the face
/// type determines the surface similarly.
fn derive_surface_type(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
) -> VoxelType {
    let voxel = world.get(pos);

    // Ladder voxels: surface type is the ladder type itself.
    if voxel.is_ladder() {
        return voxel;
    }

    // BuildingInterior voxels derive surface from their own face data.
    if voxel == VoxelType::BuildingInterior
        && let Some(fd) = face_data.get(&pos)
    {
        // Check Floor first (standing on it).
        if fd.get(FaceDirection::NegY).blocks_movement() {
            return VoxelType::GrownPlatform;
        }
        // Check side faces for walls.
        for &dir in &[
            FaceDirection::PosX,
            FaceDirection::NegX,
            FaceDirection::PosZ,
            FaceDirection::NegZ,
        ] {
            if fd.get(dir).blocks_movement() {
                return VoxelType::GrownWall;
            }
        }
        // Check ceiling.
        if fd.get(FaceDirection::PosY).blocks_movement() {
            return VoxelType::GrownPlatform;
        }
        // Fallback: check solid neighbors like normal Air.
    }

    // Check below first (creature standing on this surface).
    let below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
    let below_type = world.get(below);
    if below_type.is_solid() {
        // Dirt behaves like ForestFloor for navigation — ground-only creatures
        // can walk on hilly dirt terrain.
        if below_type == VoxelType::Dirt {
            return VoxelType::ForestFloor;
        }
        return below_type;
    }
    // Check if below is BuildingInterior with a Ceiling face pointing up.
    if below_type == VoxelType::BuildingInterior
        && let Some(fd) = face_data.get(&below)
        && fd.get(FaceDirection::PosY).blocks_movement()
    {
        return VoxelType::GrownPlatform;
    }

    // Check horizontal neighbors and above in fixed order.
    let side_offsets: [(i32, i32, i32); 5] =
        [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)];
    for (dx, dy, dz) in side_offsets {
        let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
        let ntype = world.get(neighbor);
        if ntype.is_solid() {
            if ntype == VoxelType::Dirt {
                return VoxelType::ForestFloor;
            }
            return ntype;
        }
        // Check if neighbor is BuildingInterior with blocking face toward pos.
        if ntype == VoxelType::BuildingInterior
            && let Some(fd) = face_data.get(&neighbor)
        {
            let dir = FaceDirection::from_offset(-dx, -dy, -dz);
            if let Some(d) = dir {
                let ft = fd.get(d);
                if ft.blocks_movement() {
                    return match ft {
                        FaceType::Floor | FaceType::Ceiling => VoxelType::GrownPlatform,
                        _ => VoxelType::GrownWall,
                    };
                }
            }
        }
    }

    // Shouldn't happen — only called for voxels with solid face neighbors.
    VoxelType::ForestFloor
}

/// Check whether face data blocks movement from `from` to `to`.
///
/// For each nonzero component of the offset (dx, dy, dz):
/// - Check the source voxel's face in that component direction
/// - Check the dest voxel's face in the opposite direction
/// - If any checked face blocks movement → edge is blocked
///
/// For diagonals: if ANY component direction is blocked, the whole diagonal
/// is blocked (prevents corner-cutting through walls).
fn is_edge_blocked_by_faces(
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    from: VoxelCoord,
    to: VoxelCoord,
) -> bool {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;

    // Check each nonzero component direction.
    let components: [(i32, i32, i32); 3] = [
        (dx.signum(), 0, 0),
        (0, dy.signum(), 0),
        (0, 0, dz.signum()),
    ];

    for (cx, cy, cz) in components {
        if cx == 0 && cy == 0 && cz == 0 {
            continue;
        }
        // Check source voxel's face in this direction.
        if let Some(fd) = face_data.get(&from)
            && let Some(dir) = FaceDirection::from_offset(cx, cy, cz)
            && fd.get(dir).blocks_movement()
        {
            return true;
        }
        // Check dest voxel's face in the opposite direction.
        if let Some(fd) = face_data.get(&to)
            && let Some(dir) = FaceDirection::from_offset(-cx, -cy, -cz)
            && fd.get(dir).blocks_movement()
        {
            return true;
        }
    }
    false
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
            ForestFloor | Dirt => EdgeType::ForestFloor,
            Trunk => {
                if from_pos.y != to_pos.y {
                    EdgeType::TrunkClimb
                } else {
                    EdgeType::TrunkCircumference
                }
            }
            Branch | Leaf | Fruit | GrownPlatform | Bridge | Root | BuildingInterior | Strut => {
                EdgeType::BranchWalk
            }
            GrownStairs | GrownWall => EdgeType::TrunkClimb,
            WoodLadder => EdgeType::WoodLadderClimb,
            RopeLadder => EdgeType::RopeLadderClimb,
            Air => EdgeType::BranchWalk, // shouldn't happen
        };
    }

    // Mixed surface types — one side is ladder, other is not → BranchWalk
    // (stepping on/off the ladder).
    if matches!(from_surface, WoodLadder | RopeLadder)
        || matches!(to_surface, WoodLadder | RopeLadder)
    {
        return EdgeType::BranchWalk;
    }

    // Mixed surface types.
    match (from_surface, to_surface) {
        (ForestFloor, Trunk) | (Trunk, ForestFloor) => EdgeType::GroundToTrunk,
        (Dirt, Trunk) | (Trunk, Dirt) => EdgeType::GroundToTrunk,
        (ForestFloor, Dirt) | (Dirt, ForestFloor) => EdgeType::ForestFloor,
        (ForestFloor, Root) | (Root, ForestFloor) => EdgeType::ForestFloor,
        (Dirt, Root) | (Root, Dirt) => EdgeType::ForestFloor,
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

/// All 26 neighbor offsets in a fixed deterministic order.
/// Used by the BFS in `build_nav_graph` and related functions.
#[rustfmt::skip]
const ALL_26_NEIGHBORS: [(i32, i32, i32); 26] = [
    // dy = -1 (9 offsets)
    (-1, -1, -1), ( 0, -1, -1), ( 1, -1, -1),
    (-1, -1,  0), ( 0, -1,  0), ( 1, -1,  0),
    (-1, -1,  1), ( 0, -1,  1), ( 1, -1,  1),
    // dy = 0 (8 offsets, skipping (0,0,0))
    (-1,  0, -1), ( 0,  0, -1), ( 1,  0, -1),
    (-1,  0,  0),               ( 1,  0,  0),
    (-1,  0,  1), ( 0,  0,  1), ( 1,  0,  1),
    // dy = +1 (9 offsets)
    (-1,  1, -1), ( 0,  1, -1), ( 1,  1, -1),
    (-1,  1,  0), ( 0,  1,  0), ( 1,  1,  0),
    (-1,  1,  1), ( 0,  1,  1), ( 1,  1,  1),
];

/// Build a navigation graph by scanning the voxel world.
///
/// **Algorithm (span-scan + BFS):**
/// 1. **Span-scan seed extraction:** For each (x, z) column, read RLE spans
///    via `column_spans()`. Extract seed nav candidates from span boundaries:
///    - Solid→Air transition: the first air Y (top_y + 1) is a standing
///      candidate. No horizontal neighbor seeds — the BFS discovers them.
///    - Non-solid non-air spans (BuildingInterior, ladders): every Y in the
///      span is a candidate (these spans are typically 1-3 voxels tall).
///
/// 2. **Filter seeds:** Remove any seed that doesn't pass
///    `should_be_nav_node()`.
///
/// 3. **Insert valid seeds as nodes** in the nav graph.
///
/// 4. **BFS from seeds:** For each node popped from the queue, check all 26
///    neighbors. If a neighbor is already in the graph, create an edge if
///    one doesn't exist. If a neighbor passes `should_be_nav_node`, insert
///    it as a new node, create an edge, and push it onto the queue.
///    This discovers all reachable nav nodes and creates all edges in a
///    single pass.
///
/// Node IDs are assigned in BFS discovery order (deterministic from seed
/// order + FIFO queue + fixed 26-direction order). No sorting step needed.
///
/// This is O(number_of_spans + BFS_frontier) instead of O(world_volume).
/// At no point does it iterate through Y ranges of solid material.
pub fn build_nav_graph(world: &VoxelWorld, face_data: &BTreeMap<VoxelCoord, FaceData>) -> NavGraph {
    let mut graph = NavGraph::new();

    let sx = world.size_x as usize;
    let sy = world.size_y as usize;
    let sz = world.size_z as usize;

    if sx == 0 || sy == 0 || sz == 0 {
        return graph;
    }

    graph.world_size = (sx, sy, sz);

    // --- Step 1: Span-scan to extract seed candidates ---
    // For each (x, z) column (in z, x order for determinism), examine span
    // boundaries. Where an air span starts after a solid span, top_y + 1 is
    // a seed. BuildingInterior and ladder spans emit every voxel.
    // No horizontal neighbor seeds — the BFS discovers those.
    let mut seed_set: LookupMap<VoxelCoord, ()> = LookupMap::new();
    let mut seed_list: Vec<VoxelCoord> = Vec::new();

    let add_seed = |coord: VoxelCoord,
                    seed_set: &mut LookupMap<VoxelCoord, ()>,
                    seed_list: &mut Vec<VoxelCoord>| {
        if !seed_set.contains_key(&coord) {
            seed_set.insert(coord, ());
            seed_list.push(coord);
        }
    };

    for z in 0..sz {
        for x in 0..sx {
            let spans: Vec<(VoxelType, u8, u8)> = world.column_spans(x as u32, z as u32).collect();

            for (span_idx, &(vt, y_start, y_end)) in spans.iter().enumerate() {
                if vt.is_solid() {
                    // The air voxel just above this solid span is a standing
                    // candidate (in the same column).
                    let above_y = y_end as i32 + 1;
                    if above_y >= 1 && above_y < sy as i32 {
                        add_seed(
                            VoxelCoord::new(x as i32, above_y, z as i32),
                            &mut seed_set,
                            &mut seed_list,
                        );
                    }
                    // The air voxel just below this solid span is face-adjacent
                    // to solid above.
                    if span_idx > 0 {
                        let below_y = y_start as i32 - 1;
                        if below_y >= 1 {
                            add_seed(
                                VoxelCoord::new(x as i32, below_y, z as i32),
                                &mut seed_set,
                                &mut seed_list,
                            );
                        }
                    }
                } else if vt == VoxelType::BuildingInterior || vt.is_ladder() {
                    // Non-solid non-air: every voxel in the span is a candidate.
                    // These spans are typically 1-3 voxels tall.
                    for y in y_start..=y_end {
                        if y >= 1 {
                            add_seed(
                                VoxelCoord::new(x as i32, y as i32, z as i32),
                                &mut seed_set,
                                &mut seed_list,
                            );
                        }
                    }
                }
                // Air spans: skip entirely (no seeds from pure air).
            }
        }
    }

    // --- Step 2: Filter seeds through should_be_nav_node ---
    // --- Step 3: Insert valid seeds as nodes in the graph ---
    let mut bfs_queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();

    for &coord in &seed_list {
        if !should_be_nav_node(world, face_data, coord) {
            continue;
        }
        // Already inserted by a previous seed (dedup).
        if graph.spatial_index.contains_key(&coord) {
            continue;
        }
        let surface = derive_surface_type(world, face_data, coord);
        let node_id = graph.add_node(coord, surface);
        graph.spatial_index.insert(coord, node_id.0);
        bfs_queue.push_back(node_id.0);
    }

    // --- Step 4: BFS — discover neighbors, create nodes AND edges ---
    // For each node popped, check all 26 neighbors:
    // - If neighbor is already in the graph: create an edge if one doesn't
    //   already exist between current and neighbor.
    // - If neighbor is NOT in the graph: check should_be_nav_node. If valid,
    //   insert as a new node, create an edge, push onto the BFS queue.
    // - If neighbor fails should_be_nav_node: skip.
    while let Some(slot) = bfs_queue.pop_front() {
        let (pos, from_surface) = {
            let node = graph.nodes[slot as usize].as_ref().unwrap();
            (node.position, node.surface_type)
        };

        for &(dx, dy, dz) in &ALL_26_NEIGHBORS {
            let np = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);

            // Bounds check.
            if np.x < 0
                || np.y < 1
                || np.z < 0
                || np.x >= sx as i32
                || np.y >= sy as i32
                || np.z >= sz as i32
            {
                continue;
            }

            // Check if face data blocks this edge.
            if is_edge_blocked_by_faces(face_data, pos, np) {
                continue;
            }

            if let Some(&neighbor_slot) = graph.spatial_index.get(&np) {
                // Neighbor already in graph — create edge if not already connected.
                let already_connected = graph.nodes[slot as usize]
                    .as_ref()
                    .unwrap()
                    .edge_indices
                    .iter()
                    .any(|&eidx| graph.edges[eidx].to == NavNodeId(neighbor_slot));
                if !already_connected {
                    let to_node = graph.nodes[neighbor_slot as usize].as_ref().unwrap();
                    let edge_type = derive_edge_type(from_surface, to_node.surface_type, pos, np);
                    let dist = scaled_distance(dx, dy, dz);
                    graph.add_edge(NavNodeId(slot), NavNodeId(neighbor_slot), edge_type, dist);
                }
            } else {
                // Neighbor not in graph — check if it should be a nav node.
                if should_be_nav_node(world, face_data, np) {
                    let surface = derive_surface_type(world, face_data, np);
                    let new_id = graph.add_node(np, surface);
                    graph.spatial_index.insert(np, new_id.0);

                    let edge_type = derive_edge_type(from_surface, surface, pos, np);
                    let dist = scaled_distance(dx, dy, dz);
                    graph.add_edge(NavNodeId(slot), new_id, edge_type, dist);

                    bfs_queue.push_back(new_id.0);
                }
            }
        }
    }

    graph
}

// ---------------------------------------------------------------------------
// Large creature nav graph (2x2x2 footprint)
// ---------------------------------------------------------------------------

/// Find the surface y for a 2x2 large-creature footprint at anchor (ax, az).
///
/// Uses RLE column spans to find the topmost solid voxel in each of the 4
/// columns without iterating through Y values. Returns `None` if any column
/// has no solid ground or if the height variation across the 4 columns
/// exceeds 1 voxel. Otherwise returns `max_surface + 1` (the air layer
/// above the highest ground point — the creature stands at its tallest
/// point, straddling any minor unevenness).
fn large_node_surface_y(world: &VoxelWorld, ax: i32, az: i32) -> Option<i32> {
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
                None => return None, // No solid ground in this column.
            }
        }
    }
    if max_surface - min_surface > 1 {
        return None; // Height variation exceeds 1-voxel tolerance.
    }
    Some(max_surface + 1)
}

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

fn is_large_node_valid(world: &VoxelWorld, ax: i32, az: i32) -> bool {
    if ax < 0 || az < 0 {
        return false;
    }
    let sx = world.size_x as i32;
    let sy = world.size_y as i32;
    let sz = world.size_z as i32;
    if ax + 2 > sx || az + 2 > sz || 3 > sy {
        return false;
    }

    let air_y = match large_node_surface_y(world, ax, az) {
        Some(y) => y,
        None => return false,
    };

    // Need 2 voxels of clearance above the surface.
    if air_y + 2 > sy {
        return false;
    }

    // Check clearance at air_y and air_y+1.
    for y in air_y..air_y + 2 {
        for dz in 0..2 {
            for dx in 0..2 {
                if world.get(VoxelCoord::new(ax + dx, y, az + dz)).is_solid() {
                    return false;
                }
            }
        }
    }

    true
}

/// Check whether a large creature can move from anchor `from` to anchor `to`.
///
/// Both anchors must have valid surface heights differing by at most 1 voxel.
/// Each column in the union of the two 2x2 footprints must have solid ground,
/// all column surfaces must be within 1 voxel of each other, and there must
/// be 2 voxels of air clearance above the highest surface in the union.
fn is_large_edge_valid(world: &VoxelWorld, from: (i32, i32), to: (i32, i32)) -> bool {
    // Both endpoints must have valid surface heights within 1 voxel of each other.
    let from_y = match large_node_surface_y(world, from.0, from.1) {
        Some(y) => y,
        None => return false,
    };
    let to_y = match large_node_surface_y(world, to.0, to.1) {
        Some(y) => y,
        None => return false,
    };
    if (from_y - to_y).abs() > 1 {
        return false;
    }
    let max_air_y = from_y.max(to_y);

    let min_x = from.0.min(to.0);
    let max_x = from.0.max(to.0) + 2;
    let min_z = from.1.min(to.1);
    let max_z = from.1.max(to.1) + 2;

    let sx = world.size_x as i32;
    let sy = world.size_y as i32;
    let sz = world.size_z as i32;
    if max_x > sx || max_z > sz || max_air_y + 2 > sy {
        return false;
    }

    // Check each column in the union footprint: find its surface, verify all
    // surfaces are within 1 voxel of each other.
    let mut union_min_surface = i32::MAX;
    let mut union_max_surface = i32::MIN;
    for z in min_z..max_z {
        for x in min_x..max_x {
            match top_solid_y_from_spans(world, x as u32, z as u32) {
                Some(y) => {
                    union_min_surface = union_min_surface.min(y as i32);
                    union_max_surface = union_max_surface.max(y as i32);
                }
                None => return false, // No ground in this column.
            }
        }
    }
    if union_max_surface - union_min_surface > 1 {
        return false; // Union footprint has too much height variation.
    }

    // Air clearance: 2 voxels above max_air_y across the entire union.
    for y in max_air_y..max_air_y + 2 {
        for z in min_z..max_z {
            for x in min_x..max_x {
                if world.get(VoxelCoord::new(x, y, z)).is_solid() {
                    return false;
                }
            }
        }
    }

    true
}

/// All 8 horizontal neighbor offsets for large creature nav graph BFS.
#[rustfmt::skip]
const LARGE_NAV_8_NEIGHBORS: [(i32, i32); 8] = [
    (-1, -1), ( 0, -1), ( 1, -1),
    (-1,  0),           ( 1,  0),
    (-1,  1), ( 0,  1), ( 1,  1),
];

/// Build a navigation graph for large (2x2x2 footprint) creatures.
///
/// Nodes exist at anchor positions `(x, y, z)` where the 2x2 ground footprint
/// has solid voxels within 1 voxel of height variation and 2 voxels of air
/// clearance above the highest ground point. The node y is `max_surface + 1`
/// (the creature stands at its tallest point, straddling minor unevenness).
///
/// **Algorithm (seed + BFS):**
/// 1. Precompute per-column top-solid-Y from span data.
/// 2. Scan all anchor positions to find valid seeds with proper surface
///    heights and air clearance.
/// 3. Insert valid seeds as nodes in the graph.
/// 4. BFS from seeds: check 8 horizontal neighbors, insert new valid nodes,
///    create edges. No separate edge pass needed.
///
/// All edges are `ForestFloor` type since large creatures are ground-only.
/// The resulting graph uses the same `NavGraph` struct as the standard graph,
/// so all existing pathfinding code works unchanged.
pub fn build_large_nav_graph(world: &VoxelWorld) -> NavGraph {
    let mut graph = NavGraph::new();

    let sx = world.size_x as usize;
    let sy = world.size_y as usize;
    let sz = world.size_z as usize;

    if sx < 2 || sz < 2 || sy < 3 {
        return graph;
    }

    graph.world_size = (sx, sy, sz);

    // --- Precompute per-column top-solid-Y from RLE spans ---
    let mut col_top_solid: Vec<Option<u8>> = vec![None; sx * sz];
    for z in 0..sz {
        for x in 0..sx {
            col_top_solid[x + z * sx] = top_solid_y_from_spans(world, x as u32, z as u32);
        }
    }

    // --- Helper: compute large_node_surface_y using precomputed heights ---
    let surface_y_from_precomputed =
        |ax: usize, az: usize, col_top: &[Option<u8>]| -> Option<i32> {
            let mut min_s = i32::MAX;
            let mut max_s = i32::MIN;
            for dz in 0..2usize {
                for dx in 0..2usize {
                    match col_top[(ax + dx) + (az + dz) * sx] {
                        Some(y) => {
                            min_s = min_s.min(y as i32);
                            max_s = max_s.max(y as i32);
                        }
                        None => return None,
                    }
                }
            }
            if max_s - min_s > 1 {
                return None;
            }
            Some(max_s + 1)
        };

    // Helper: check if an anchor position is valid (surface y + air clearance).
    let is_anchor_valid = |x: usize, z: usize, col_top: &[Option<u8>]| -> Option<i32> {
        if x + 1 >= sx || z + 1 >= sz {
            return None;
        }
        let air_y = surface_y_from_precomputed(x, z, col_top)?;
        if air_y + 2 > sy as i32 {
            return None;
        }
        // Check clearance: 2 voxels of air above the surface.
        for dy in 0..2 {
            for dz2 in 0..2 {
                for dx2 in 0..2 {
                    if world
                        .get(VoxelCoord::new(x as i32 + dx2, air_y + dy, z as i32 + dz2))
                        .is_solid()
                    {
                        return None;
                    }
                }
            }
        }
        Some(air_y)
    };

    // --- Step 1-3: Find valid seed anchors, insert as nodes ---
    // Use a LookupMap keyed by (x, z) anchor to track which anchors are in
    // the graph. We can't use spatial_index directly because the y varies.
    let mut anchor_in_graph: LookupMap<(i32, i32), ()> = LookupMap::new();
    let mut bfs_queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();

    for z in 0..sz.saturating_sub(1) {
        for x in 0..sx.saturating_sub(1) {
            let air_y = match is_anchor_valid(x, z, &col_top_solid) {
                Some(y) => y,
                None => continue,
            };

            let coord = VoxelCoord::new(x as i32, air_y, z as i32);
            let node_id = graph.add_node(coord, VoxelType::ForestFloor);
            graph.spatial_index.insert(coord, node_id.0);
            anchor_in_graph.insert((x as i32, z as i32), ());
            bfs_queue.push_back(node_id.0);
        }
    }

    // --- Step 4: BFS — discover neighbors, create edges (and new nodes) ---
    while let Some(slot) = bfs_queue.pop_front() {
        let (ax, air_y, az) = {
            let node = graph.nodes[slot as usize].as_ref().unwrap();
            (node.position.x, node.position.y, node.position.z)
        };

        for &(dx, dz) in &LARGE_NAV_8_NEIGHBORS {
            let nx = ax + dx;
            let nz = az + dz;
            if nx < 0 || nz < 0 {
                continue;
            }
            let (nxu, nzu) = (nx as usize, nz as usize);
            if nxu + 1 >= sx || nzu + 1 >= sz {
                continue;
            }

            // Check if neighbor anchor is already in the graph.
            if anchor_in_graph.contains_key(&(nx, nz)) {
                // Already in graph — find its node and create edge if needed.
                let n_air_y = match large_node_surface_y(world, nx, nz) {
                    Some(y) => y,
                    None => continue,
                };
                let n_coord = VoxelCoord::new(nx, n_air_y, nz);
                let neighbor_slot = match graph.spatial_index.get(&n_coord).copied() {
                    Some(s) => s,
                    None => continue,
                };

                // Check if already connected.
                let already_connected = graph.nodes[slot as usize]
                    .as_ref()
                    .unwrap()
                    .edge_indices
                    .iter()
                    .any(|&eidx| graph.edges[eidx].to == NavNodeId(neighbor_slot));
                if already_connected {
                    continue;
                }

                if !is_large_edge_valid(world, (ax, az), (nx, nz)) {
                    continue;
                }

                let dy = n_air_y - air_y;
                let dist = scaled_distance(dx, dy, dz);
                graph.add_edge(
                    NavNodeId(slot),
                    NavNodeId(neighbor_slot),
                    EdgeType::ForestFloor,
                    dist,
                );
            } else {
                // Not in graph — check if it's a valid anchor.
                let n_air_y = match is_anchor_valid(nxu, nzu, &col_top_solid) {
                    Some(y) => y,
                    None => continue,
                };

                if !is_large_edge_valid(world, (ax, az), (nx, nz)) {
                    continue;
                }

                let n_coord = VoxelCoord::new(nx, n_air_y, nz);
                let new_id = graph.add_node(n_coord, VoxelType::ForestFloor);
                graph.spatial_index.insert(n_coord, new_id.0);
                anchor_in_graph.insert((nx, nz), ());

                let dy = n_air_y - air_y;
                let dist = scaled_distance(dx, dy, dz);
                graph.add_edge(NavNodeId(slot), new_id, EdgeType::ForestFloor, dist);

                bfs_queue.push_back(new_id.0);
            }
        }
    }

    graph
}

/// Incrementally update the large nav graph after a voxel changed from Air to
/// solid (e.g. construction materialization).
///
/// Checks which large-creature anchor positions are affected by the changed
/// coordinate, removes invalidated nodes, adds newly valid nodes, and
/// recomputes edges. Returns removed node IDs for creature resnapping.
pub fn update_large_after_voxel_solidified(
    graph: &mut NavGraph,
    world: &VoxelWorld,
    coord: VoxelCoord,
) -> Vec<NavNodeId> {
    let mut removed_ids = Vec::new();
    let sx = graph.world_size.0;
    let sz = graph.world_size.2;
    if sx < 2 || sz < 2 {
        return removed_ids;
    }

    // A changed voxel at (cx, cy, cz) can affect large anchors at:
    // - y=0 (ground support): anchors (cx-1..cx, 1, cz-1..cz)
    // - y=1 or y=2 (clearance): anchors (cx-1..cx, 1, cz-1..cz)
    // So we always check the 2x2 set of possible anchor positions.
    let mut affected_anchors: Vec<(i32, i32)> = Vec::new();
    for dz in -1..=0 {
        for dx in -1..=0 {
            let ax = coord.x + dx;
            let az = coord.z + dz;
            if ax >= 0 && az >= 0 && (ax as usize) + 1 < sx && (az as usize) + 1 < sz {
                affected_anchors.push((ax, az));
            }
        }
    }

    // Helper: find the existing large node at anchor (ax, az) by scanning Y
    // values. Large nodes have a unique (x, z) anchor — only one Y is valid.
    let find_existing_node = |graph: &NavGraph, ax: i32, az: i32| -> Option<(VoxelCoord, u32)> {
        let sy = graph.world_size.1;
        for y in 0..sy {
            let coord = VoxelCoord::new(ax, y as i32, az);
            if let Some(&slot) = graph.spatial_index.get(&coord) {
                return Some((coord, slot));
            }
        }
        None
    };

    // Helper: allocate a node slot (reusing free slots or appending).
    let alloc_node = |graph: &mut NavGraph, coord: VoxelCoord| -> u32 {
        let slot = if let Some(free) = graph.free_slots.pop() {
            let id = NavNodeId(free as u32);
            graph.nodes[free] = Some(NavNode {
                id,
                position: coord,
                surface_type: VoxelType::ForestFloor,
                edge_indices: Vec::new(),
            });
            free
        } else {
            let slot = graph.nodes.len();
            let id = NavNodeId(slot as u32);
            graph.nodes.push(Some(NavNode {
                id,
                position: coord,
                surface_type: VoxelType::ForestFloor,
                edge_indices: Vec::new(),
            }));
            slot
        };
        graph.spatial_index.insert(coord, slot as u32);
        slot as u32
    };

    // Step 1: Update nodes at affected anchors.
    for &(ax, az) in &affected_anchors {
        let should_exist = is_large_node_valid(world, ax, az);
        let existing = find_existing_node(graph, ax, az);
        let is_node = existing.is_some();

        if !should_exist && is_node {
            let (existing_coord, slot_val) = existing.unwrap();
            removed_ids.push(NavNodeId(slot_val));
            graph.nodes[slot_val as usize] = None;
            graph.spatial_index.remove(&existing_coord);
            graph.free_slots.push(slot_val as usize);
        } else if should_exist && is_node {
            // Node exists — check if it needs to move to a different y.
            let expected_air_y = large_node_surface_y(world, ax, az).unwrap();
            let (existing_coord, slot_val) = existing.unwrap();
            if existing_coord.y != expected_air_y {
                // Remove old node at wrong y.
                removed_ids.push(NavNodeId(slot_val));
                graph.nodes[slot_val as usize] = None;
                graph.spatial_index.remove(&existing_coord);
                graph.free_slots.push(slot_val as usize);
                // Add new node at correct y.
                let new_coord = VoxelCoord::new(ax, expected_air_y, az);
                alloc_node(graph, new_coord);
            }
        } else if should_exist && !is_node {
            let air_y = large_node_surface_y(world, ax, az).unwrap();
            let anchor_coord = VoxelCoord::new(ax, air_y, az);
            alloc_node(graph, anchor_coord);
        }
    }

    // Step 2: Collect dirty set — affected anchors + their 8 horizontal neighbors.
    let mut dirty_set: Vec<usize> = Vec::new();
    let mut is_dirty = vec![false; graph.nodes.len()];

    for &(ax, az) in &affected_anchors {
        for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                let nx = ax + dx;
                let nz = az + dz;
                if nx < 0 || nz < 0 || (nx as usize) + 1 >= sx || (nz as usize) + 1 >= sz {
                    continue;
                }
                // Find the node at any y for this anchor.
                if let Some((_, slot)) = find_existing_node(graph, nx, nz) {
                    let s = slot as usize;
                    if s < is_dirty.len() && !is_dirty[s] {
                        is_dirty[s] = true;
                        dirty_set.push(s);
                    }
                }
            }
        }
    }

    // Step 3: Clear edges touching dirty nodes.
    for &slot in &dirty_set {
        if let Some(node) = &graph.nodes[slot] {
            let edge_indices: Vec<usize> = node.edge_indices.clone();
            for &eidx in &edge_indices {
                let edge = &graph.edges[eidx];
                let other_slot = edge.to.0 as usize;
                if let Some(other_node) = graph.nodes[other_slot].as_mut() {
                    other_node
                        .edge_indices
                        .retain(|&rev_idx| graph.edges[rev_idx].to != NavNodeId(slot as u32));
                }
            }
            if let Some(node) = graph.nodes[slot].as_mut() {
                node.edge_indices.clear();
            }
        }
    }

    // Step 4: Recompute edges for dirty nodes.
    let offsets: [(i32, i32); 8] = [
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (1, 1),
    ];

    for &slot in &dirty_set {
        let node = match &graph.nodes[slot] {
            Some(n) => n,
            None => continue,
        };
        let ax = node.position.x;
        let az = node.position.z;

        for &(dx, dz) in &offsets {
            let nx = ax + dx;
            let nz = az + dz;
            if nx < 0 || nz < 0 || (nx as usize) + 1 >= sx || (nz as usize) + 1 >= sz {
                continue;
            }
            // Find neighbor node at any y.
            let (_, n_slot) = match find_existing_node(graph, nx, nz) {
                Some(v) => v,
                None => continue,
            };
            let ns = n_slot as usize;

            // Avoid duplicate edges: only create from smaller slot.
            if is_dirty.get(ns).copied().unwrap_or(false) && ns < slot {
                continue;
            }

            if !is_large_edge_valid(world, (ax, az), (nx, nz)) {
                continue;
            }

            let from_id = NavNodeId(slot as u32);
            let to_id = NavNodeId(ns as u32);
            let from_node_y = graph.nodes[slot].as_ref().unwrap().position.y;
            let to_node_y = graph.nodes[ns].as_ref().unwrap().position.y;
            let dy = to_node_y - from_node_y;
            let dist = scaled_distance(dx, dy, dz);
            graph.add_edge(from_id, to_id, EdgeType::ForestFloor, dist);
        }
    }

    removed_ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    /// Cached test world. Constructed once (tree gen into a 64^3 world),
    /// then cloned by `test_world()`. 5 call sites benefit.
    static CACHED_TEST_WORLD: LazyLock<VoxelWorld> = LazyLock::new(|| {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let mut config = GameConfig {
            world_size: (64, 64, 64),
            floor_y: 0,
            ..GameConfig::default()
        };
        config.tree_profile.leaves.canopy_density = 0.0;
        config.terrain_max_height = 0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng, &|_| {});

        world
    });

    /// Empty face data for tests that don't use buildings.
    fn no_faces() -> BTreeMap<VoxelCoord, FaceData> {
        BTreeMap::new()
    }

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
        graph.add_edge(a, b, EdgeType::ForestFloor, 5 * DIST_SCALE);

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
    fn spread_destinations_returns_center_for_single() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        let b = graph.add_node(VoxelCoord::new(1, 0, 0), VoxelType::ForestFloor);
        graph.add_edge(a, b, EdgeType::ForestFloor, DIST_SCALE);

        let result = graph.spread_destinations(a, 1);
        assert_eq!(result, vec![a]);
    }

    #[test]
    fn spread_destinations_bfs_order() {
        // Build a linear chain: A -- B -- C -- D
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        let b = graph.add_node(VoxelCoord::new(1, 0, 0), VoxelType::ForestFloor);
        let c = graph.add_node(VoxelCoord::new(2, 0, 0), VoxelType::ForestFloor);
        let d = graph.add_node(VoxelCoord::new(3, 0, 0), VoxelType::ForestFloor);
        graph.add_edge(a, b, EdgeType::ForestFloor, DIST_SCALE);
        graph.add_edge(b, c, EdgeType::ForestFloor, DIST_SCALE);
        graph.add_edge(c, d, EdgeType::ForestFloor, DIST_SCALE);

        // Spread from B, requesting 3 destinations.
        let result = graph.spread_destinations(b, 3);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], b); // Center is always first.
        // A and C are both 1 edge from B; order depends on edge_indices order.
        assert!(result.contains(&a));
        assert!(result.contains(&c));
    }

    #[test]
    fn spread_destinations_limits_to_count() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        let b = graph.add_node(VoxelCoord::new(1, 0, 0), VoxelType::ForestFloor);
        let c = graph.add_node(VoxelCoord::new(2, 0, 0), VoxelType::ForestFloor);
        graph.add_edge(a, b, EdgeType::ForestFloor, DIST_SCALE);
        graph.add_edge(b, c, EdgeType::ForestFloor, DIST_SCALE);

        let result = graph.spread_destinations(b, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], b);
    }

    #[test]
    fn spread_destinations_handles_fewer_nodes_than_requested() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        let b = graph.add_node(VoxelCoord::new(1, 0, 0), VoxelType::ForestFloor);
        graph.add_edge(a, b, EdgeType::ForestFloor, DIST_SCALE);

        // Request 5 but only 2 nodes exist.
        let result = graph.spread_destinations(a, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], a);
        assert_eq!(result[1], b);
    }

    #[test]
    fn spread_destinations_empty_on_zero_count() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        assert!(graph.spread_destinations(a, 0).is_empty());
    }

    #[test]
    fn spread_destinations_skips_dead_nodes() {
        let mut graph = NavGraph::new();
        let a = graph.add_node(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        let b = graph.add_node(VoxelCoord::new(1, 0, 0), VoxelType::ForestFloor);
        let c = graph.add_node(VoxelCoord::new(2, 0, 0), VoxelType::ForestFloor);
        graph.add_edge(a, b, EdgeType::ForestFloor, DIST_SCALE);
        graph.add_edge(b, c, EdgeType::ForestFloor, DIST_SCALE);

        // Kill node B — A can't reach C.
        graph.kill_node(b);
        let result = graph.spread_destinations(a, 3);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], a);
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
        let surface = derive_surface_type(&world, &no_faces(), VoxelCoord::new(4, 1, 4));
        assert_eq!(surface, VoxelType::ForestFloor);
    }

    #[test]
    fn surface_type_standing_on_trunk() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(4, 5, 4), VoxelType::Trunk);
        // Air at y=6 is above trunk.
        let surface = derive_surface_type(&world, &no_faces(), VoxelCoord::new(4, 6, 4));
        assert_eq!(surface, VoxelType::Trunk);
    }

    #[test]
    fn surface_type_clinging_to_trunk() {
        let mut world = VoxelWorld::new(8, 8, 8);
        // Trunk at x=3, air at x=4 clinging to trunk side.
        world.set(VoxelCoord::new(3, 5, 4), VoxelType::Trunk);
        let surface = derive_surface_type(&world, &no_faces(), VoxelCoord::new(4, 5, 4));
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
        let surface = derive_surface_type(&world, &no_faces(), VoxelCoord::new(4, 1, 4));
        assert_eq!(surface, VoxelType::ForestFloor);
    }

    #[test]
    fn air_above_dirt_has_forest_floor_surface() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(4, 0, 4), VoxelType::ForestFloor);
        world.set(VoxelCoord::new(4, 1, 4), VoxelType::Dirt);
        world.set(VoxelCoord::new(4, 2, 4), VoxelType::Dirt);
        // Air at y=3 is above Dirt — should map to ForestFloor for nav.
        let surface = derive_surface_type(&world, &no_faces(), VoxelCoord::new(4, 3, 4));
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
    /// Clone a pre-built test world from the cache.
    fn test_world() -> VoxelWorld {
        CACHED_TEST_WORLD.clone()
    }

    #[test]
    fn nav_nodes_are_air_voxels() {
        let world = test_world();
        let graph = build_nav_graph(&world, &no_faces());

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
        let graph = build_nav_graph(&world, &no_faces());

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
        let graph = build_nav_graph(&world, &no_faces());

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
        let graph = build_nav_graph(&world, &no_faces());

        let trunk_nodes: Vec<_> = graph
            .live_nodes()
            .filter(|n| n.surface_type == VoxelType::Trunk)
            .collect();
        assert!(!trunk_nodes.is_empty(), "Should have trunk surface nodes");
    }

    #[test]
    fn build_nav_graph_is_connected() {
        let world = test_world();
        let graph = build_nav_graph(&world, &no_faces());

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
            floor_y: 0,
            ..GameConfig::default()
        };
        // High split chance to test connectivity with many branches.
        config.tree_profile.split.split_chance_base = 1.0;
        config.tree_profile.split.min_progress_for_split = 0.05;
        config.tree_profile.leaves.canopy_density = 0.0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng, &|_| {});

        let graph = build_nav_graph(&world, &no_faces());
        let live_count = graph.node_count();
        assert!(live_count > 0);

        // BFS flood fill from node 0.
        let n = graph.node_slot_count();
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

        // Count live nodes that are unreachable. Allow a tiny tolerance
        // since tree generation with high split chance can produce isolated
        // branch tips depending on the exact FP math.
        let unreachable_count = (0..n)
            .filter(|&i| graph.nodes[i].is_some() && !visited[i])
            .count();
        let max_unreachable = (live_count / 1000).max(1); // 0.1% tolerance
        assert!(
            unreachable_count <= max_unreachable,
            "Found {unreachable_count} unreachable nodes (out of {live_count}), max allowed {max_unreachable}",
        );
    }

    #[test]
    fn voxel_nav_determinism() {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let config = GameConfig {
            world_size: (64, 64, 64),
            floor_y: 0,
            ..GameConfig::default()
        };

        // Build two graphs from the same seed.
        let mut world_a = VoxelWorld::new(64, 64, 64);
        let mut rng_a = GameRng::new(42);
        tree_gen::generate_tree(&mut world_a, &config, &mut rng_a, &|_| {});
        let graph_a = build_nav_graph(&world_a, &no_faces());

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(42);
        tree_gen::generate_tree(&mut world_b, &config, &mut rng_b, &|_| {});
        let graph_b = build_nav_graph(&world_b, &no_faces());

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
            floor_y: 0,
            ..GameConfig::default()
        };

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng, &|_| {});

        let graph = build_nav_graph(&world, &no_faces());
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
            floor_y: 0,
            ..GameConfig::default()
        };
        config.tree_profile.roots.root_energy_fraction = 0.2;
        config.tree_profile.roots.root_initial_count = 5;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng, &|_| {});

        let graph = build_nav_graph(&world, &no_faces());
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
        let mut graph = build_nav_graph(&world, &no_faces());

        // There should be a nav node at (4,1,3) — air above floor.
        assert!(
            graph.has_node_at(VoxelCoord::new(4, 1, 3)),
            "Expected nav node at (4,1,3) before solidification",
        );

        // Solidify (4,1,3) — it should no longer be a nav node.
        world.set(VoxelCoord::new(4, 1, 3), VoxelType::GrownPlatform);
        let removed =
            graph.update_after_voxel_solidified(&world, &no_faces(), VoxelCoord::new(4, 1, 3));

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
        let mut graph = build_nav_graph(&world, &no_faces());

        // (4,1,3) is a nav node (air above floor). Solidifying it should
        // create a new nav node at (4,2,3) — air above the new solid.
        assert!(
            !graph.has_node_at(VoxelCoord::new(4, 2, 3)),
            "No nav node at (4,2,3) before solidification",
        );

        world.set(VoxelCoord::new(4, 1, 3), VoxelType::GrownPlatform);
        graph.update_after_voxel_solidified(&world, &no_faces(), VoxelCoord::new(4, 1, 3));

        assert!(
            graph.has_node_at(VoxelCoord::new(4, 2, 3)),
            "Should have a nav node at (4,2,3) after solidification — air above new solid",
        );
    }

    #[test]
    fn incremental_update_matches_full_rebuild() {
        let mut world = platform_world();
        let mut graph = build_nav_graph(&world, &no_faces());

        // Solidify (4,1,3).
        world.set(VoxelCoord::new(4, 1, 3), VoxelType::GrownPlatform);
        graph.update_after_voxel_solidified(&world, &no_faces(), VoxelCoord::new(4, 1, 3));

        // Full rebuild on the same world state.
        let rebuilt = build_nav_graph(&world, &no_faces());

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

    // --- Large (2x2x2) nav graph tests ---

    /// Helper: create a flat floor world of given size (solid at y=0, air above).
    fn flat_floor_world(sx: u32, sy: u32, sz: u32) -> VoxelWorld {
        let mut world = VoxelWorld::new(sx, sy, sz);
        for z in 0..sz {
            for x in 0..sx {
                world.set(
                    VoxelCoord::new(x as i32, 0, z as i32),
                    VoxelType::ForestFloor,
                );
            }
        }
        world
    }

    #[test]
    fn large_nav_no_floor() {
        // No solid ground → no large nodes.
        let world = VoxelWorld::new(10, 6, 10);
        let graph = build_large_nav_graph(&world);
        assert_eq!(graph.live_nodes().count(), 0, "No floor → no large nodes");
    }

    #[test]
    fn large_nav_flat_floor() {
        // 10x10 flat floor → (10-1)×(10-1) = 81 anchor positions.
        let world = flat_floor_world(10, 6, 10);
        let graph = build_large_nav_graph(&world);

        assert_eq!(
            graph.live_nodes().count(),
            81,
            "10×10 floor should produce 9×9=81 large nodes",
        );

        // Fully connected: every interior node should have 8 neighbors,
        // corner nodes 3, edge nodes 5. Check total edges.
        // In a 9×9 grid:
        // - 4 corners × 3 edges = 12
        // - (7×4) edge cells × 5 edges = 140
        // - 7×7 interior × 8 edges = 392
        // Total per-node half = (12+140+392)/2 = 272 bidirectional edges
        // But we store 2 entries per edge (one per direction), so 544 total.
        let total_edge_refs: usize = graph.live_nodes().map(|n| n.edge_indices.len()).sum();
        assert_eq!(
            total_edge_refs, 544,
            "9×9 fully connected grid should have 544 edge references",
        );
    }

    #[test]
    fn large_nav_obstacle_blocks_node() {
        // A 2-voxel tall obstacle at (5,1..2,5) creates a surface height of
        // y=2 in that column, while surrounding columns are at y=0. The
        // 2-voxel height difference exceeds the 1-voxel tolerance, blocking
        // anchors (4,4), (5,4), (4,5), (5,5).
        let mut world = flat_floor_world(10, 6, 10);
        world.set(VoxelCoord::new(5, 1, 5), VoxelType::Trunk);
        world.set(VoxelCoord::new(5, 2, 5), VoxelType::Trunk);
        let graph = build_large_nav_graph(&world);

        // 81 - 4 = 77 nodes.
        assert_eq!(
            graph.live_nodes().count(),
            77,
            "2-voxel obstacle at (5,1..2,5) should remove 4 anchor positions",
        );
        // Verify the specific removed anchors (none at any y).
        assert!(!graph.has_node_at(VoxelCoord::new(4, 1, 4)));
        assert!(!graph.has_node_at(VoxelCoord::new(5, 1, 4)));
        assert!(!graph.has_node_at(VoxelCoord::new(4, 1, 5)));
        assert!(!graph.has_node_at(VoxelCoord::new(5, 1, 5)));
    }

    #[test]
    fn large_nav_obstacle_blocks_edge() {
        // Remove floor at (0,0,2) — outside both anchor footprints but inside
        // the union ground check for the diagonal edge (0,0)→(1,1).
        // Anchor (0,0) ground = {(0,0),(1,0),(0,1),(1,1)} — doesn't include (0,2).
        // Anchor (1,1) ground = {(1,1),(2,1),(1,2),(2,2)} — doesn't include (0,2).
        // Union ground for diagonal = {0..3, 0..3} — includes (0,2).
        // So both nodes remain valid, but the diagonal edge is blocked.
        let world2 = {
            let mut w = flat_floor_world(10, 6, 10);
            w.set(VoxelCoord::new(0, 0, 2), VoxelType::Air);
            w
        };
        let graph = build_large_nav_graph(&world2);

        // Both anchors (0,0) and (1,1) should still be valid.
        assert!(graph.has_node_at(VoxelCoord::new(0, 1, 0)));
        assert!(graph.has_node_at(VoxelCoord::new(1, 1, 1)));

        // But the diagonal edge between them should NOT exist.
        let node_0_0 = graph
            .live_nodes()
            .find(|n| n.position == VoxelCoord::new(0, 1, 0))
            .unwrap();
        let has_edge_to_1_1 = node_0_0
            .edge_indices
            .iter()
            .any(|&idx| graph.node(graph.edge(idx).to).position == VoxelCoord::new(1, 1, 1));
        assert!(
            !has_edge_to_1_1,
            "Missing ground in union area should block diagonal edge",
        );
    }

    #[test]
    fn large_nav_headroom() {
        // Solid at y=2 blocks the node (need 2 voxels of clearance).
        let mut world = flat_floor_world(10, 6, 10);
        // Place solid at (3,2,3) — blocks anchors (2,2),(3,2),(2,3),(3,3)
        // because their 2x2x2 volume includes y=2.
        world.set(VoxelCoord::new(3, 2, 3), VoxelType::Branch);
        let graph = build_large_nav_graph(&world);

        assert!(!graph.has_node_at(VoxelCoord::new(2, 1, 2)));
        assert!(!graph.has_node_at(VoxelCoord::new(3, 1, 2)));
        assert!(!graph.has_node_at(VoxelCoord::new(2, 1, 3)));
        assert!(!graph.has_node_at(VoxelCoord::new(3, 1, 3)));
        assert_eq!(
            graph.live_nodes().count(),
            77,
            "Headroom obstacle should remove 4 nodes",
        );
    }

    #[test]
    fn large_nav_world_boundary() {
        // In a 3x6x3 world, only anchor (0,0) can fit (footprint 0..2, 0..2).
        // Anchors at x=2 or z=2 would need x+1=3 or z+1=3 which is OOB.
        let world = flat_floor_world(3, 6, 3);
        let graph = build_large_nav_graph(&world);

        assert_eq!(
            graph.live_nodes().count(),
            4,
            "3×3 floor: anchors at (0,0),(1,0),(0,1),(1,1) = 4 nodes",
        );
        // Specifically, max anchor is (1,1) since footprint (1..3, 1..3) fits in 3x3.
        assert!(graph.has_node_at(VoxelCoord::new(0, 1, 0)));
        assert!(graph.has_node_at(VoxelCoord::new(1, 1, 0)));
        assert!(graph.has_node_at(VoxelCoord::new(0, 1, 1)));
        assert!(graph.has_node_at(VoxelCoord::new(1, 1, 1)));
    }

    #[test]
    fn large_nav_determinism() {
        // Two builds from the same world produce identical results.
        let world = flat_floor_world(10, 6, 10);
        let g1 = build_large_nav_graph(&world);
        let g2 = build_large_nav_graph(&world);

        let pos1: Vec<VoxelCoord> = g1.live_nodes().map(|n| n.position).collect();
        let pos2: Vec<VoxelCoord> = g2.live_nodes().map(|n| n.position).collect();
        assert_eq!(
            pos1, pos2,
            "Node positions should be identical across builds"
        );

        let mut edges1: Vec<(VoxelCoord, VoxelCoord)> = Vec::new();
        for node in g1.live_nodes() {
            for &idx in &node.edge_indices {
                let e = g1.edge(idx);
                edges1.push((g1.node(e.from).position, g1.node(e.to).position));
            }
        }
        let mut edges2: Vec<(VoxelCoord, VoxelCoord)> = Vec::new();
        for node in g2.live_nodes() {
            for &idx in &node.edge_indices {
                let e = g2.edge(idx);
                edges2.push((g2.node(e.from).position, g2.node(e.to).position));
            }
        }
        assert_eq!(edges1, edges2, "Edges should be identical across builds");
    }

    // --- Large nav incremental update tests ---

    #[test]
    fn large_nav_incremental_remove() {
        let mut world = flat_floor_world(10, 6, 10);
        let mut graph = build_large_nav_graph(&world);

        assert!(graph.has_node_at(VoxelCoord::new(3, 1, 3)));

        // Solidify (3,1,3) and (3,2,3) — a 2-voxel tall obstacle creates a
        // surface at y=2 in that column, which exceeds the 1-voxel tolerance
        // relative to y=0 neighbors. Blocks anchors (2,2),(3,2),(2,3),(3,3).
        world.set(VoxelCoord::new(3, 1, 3), VoxelType::GrownPlatform);
        update_large_after_voxel_solidified(&mut graph, &world, VoxelCoord::new(3, 1, 3));
        world.set(VoxelCoord::new(3, 2, 3), VoxelType::GrownPlatform);
        let removed =
            update_large_after_voxel_solidified(&mut graph, &world, VoxelCoord::new(3, 2, 3));

        assert!(!removed.is_empty(), "Should have removed at least one node");
        assert!(!graph.has_node_at(VoxelCoord::new(3, 1, 3)));
        assert!(!graph.has_node_at(VoxelCoord::new(2, 1, 2)));
        assert!(!graph.has_node_at(VoxelCoord::new(3, 1, 2)));
        assert!(!graph.has_node_at(VoxelCoord::new(2, 1, 3)));
    }

    #[test]
    fn large_nav_incremental_add() {
        // Start with a world that has a gap in the floor, then fill it in.
        let mut world = flat_floor_world(10, 6, 10);
        world.set(VoxelCoord::new(5, 0, 5), VoxelType::Air); // Remove one floor cell
        let mut graph = build_large_nav_graph(&world);

        // Anchors (4,4),(5,4),(4,5),(5,5) are invalid because (5,0,5) is
        // in their ground support.
        assert!(!graph.has_node_at(VoxelCoord::new(4, 1, 4)));
        assert!(!graph.has_node_at(VoxelCoord::new(5, 1, 4)));
        assert!(!graph.has_node_at(VoxelCoord::new(4, 1, 5)));
        assert!(!graph.has_node_at(VoxelCoord::new(5, 1, 5)));

        // Restore the floor cell.
        world.set(VoxelCoord::new(5, 0, 5), VoxelType::ForestFloor);
        let removed =
            update_large_after_voxel_solidified(&mut graph, &world, VoxelCoord::new(5, 0, 5));

        assert!(
            removed.is_empty(),
            "Restoring floor should not remove nodes"
        );
        // All 4 anchors should now exist.
        assert!(graph.has_node_at(VoxelCoord::new(4, 1, 4)));
        assert!(graph.has_node_at(VoxelCoord::new(5, 1, 4)));
        assert!(graph.has_node_at(VoxelCoord::new(4, 1, 5)));
        assert!(graph.has_node_at(VoxelCoord::new(5, 1, 5)));
    }

    // --- Large nav height tolerance tests ---

    /// Helper: create a world with controlled terrain heights for large nav tests.
    /// Sets ForestFloor at y=0 everywhere, then stacks Dirt up to the given
    /// height at each (x, z). Heights are given as (x, z, surface_y) tuples
    /// where surface_y is the y of the topmost solid voxel.
    fn hilly_world(sx: u32, sy: u32, sz: u32, hills: &[(i32, i32, i32)]) -> VoxelWorld {
        let mut world = VoxelWorld::new(sx, sy, sz);
        // Base floor everywhere.
        for z in 0..sz {
            for x in 0..sx {
                world.set(
                    VoxelCoord::new(x as i32, 0, z as i32),
                    VoxelType::ForestFloor,
                );
            }
        }
        // Add dirt columns to reach desired heights.
        for &(x, z, surface_y) in hills {
            for y in 1..=surface_y {
                world.set(VoxelCoord::new(x, y, z), VoxelType::Dirt);
            }
        }
        world
    }

    #[test]
    fn large_node_valid_on_flat_ground() {
        // Baseline: 2x2 flat ground at y=0 with plenty of air clearance.
        let world = flat_floor_world(10, 10, 10);
        assert!(
            is_large_node_valid(&world, 3, 3),
            "2x2 flat ground should be a valid large node",
        );
        let y = large_node_surface_y(&world, 3, 3);
        assert_eq!(y, Some(1), "Surface y should be 1 (air above y=0 floor)");
    }

    #[test]
    fn large_node_valid_with_one_voxel_step() {
        // One corner of the 2x2 footprint is 1 voxel higher than the rest.
        // Anchor (3, 3): footprint covers (3,3), (4,3), (3,4), (4,4).
        // Raise (4, 4) by 1 voxel (surface at y=1 instead of y=0).
        let world = hilly_world(10, 10, 10, &[(4, 4, 1)]);
        assert!(
            is_large_node_valid(&world, 3, 3),
            "1-voxel height difference within footprint should be valid",
        );
        // Node y should be max_surface + 1 = 1 + 1 = 2.
        let y = large_node_surface_y(&world, 3, 3);
        assert_eq!(y, Some(2), "Surface y should be max_surface + 1 = 2",);
    }

    #[test]
    fn large_node_invalid_with_two_voxel_step() {
        // One corner is 2 voxels higher — exceeds tolerance.
        let world = hilly_world(10, 10, 10, &[(4, 4, 2)]);
        assert!(
            !is_large_node_valid(&world, 3, 3),
            "2-voxel height difference within footprint should be invalid",
        );
        assert_eq!(
            large_node_surface_y(&world, 3, 3),
            None,
            "large_node_surface_y should return None for 2-voxel step",
        );
    }

    #[test]
    fn large_edge_valid_between_different_heights() {
        // Two adjacent anchors at heights differing by 1 should connect.
        // Ramp: columns x<=1 at y=0, columns x>=2 at y=1.
        // Anchor (0, 0): footprint (0,0),(1,0),(0,1),(1,1) all at y=0 → surface_y=1.
        // Anchor (1, 0): footprint (1,0),(2,0),(1,1),(2,1) → mixed y=0/y=1 → surface_y=2.
        let world = hilly_world(
            10,
            10,
            10,
            &[
                (2, 0, 1),
                (2, 1, 1),
                (3, 0, 1),
                (3, 1, 1),
                (4, 0, 1),
                (4, 1, 1),
                (5, 0, 1),
                (5, 1, 1),
                (6, 0, 1),
                (6, 1, 1),
                (7, 0, 1),
                (7, 1, 1),
                (8, 0, 1),
                (8, 1, 1),
                (9, 0, 1),
                (9, 1, 1),
            ],
        );

        let from_y = large_node_surface_y(&world, 0, 0);
        let to_y = large_node_surface_y(&world, 1, 0);
        assert_eq!(from_y, Some(1));
        assert_eq!(to_y, Some(2));

        assert!(
            is_large_edge_valid(&world, (0, 0), (1, 0)),
            "Edge between nodes at heights 1 and 2 should be valid",
        );
    }

    #[test]
    fn large_edge_invalid_between_heights_too_far() {
        // Two anchors with surface y differing by 2 should NOT connect.
        // Anchor (0, 0): all columns at y=0 → surface_y=1.
        // Anchor (5, 0): all columns raised to y=2 → surface_y=3.
        // (Non-adjacent, but is_large_edge_valid is a pure geometric check.)
        let world = hilly_world(10, 10, 10, &[(5, 0, 2), (6, 0, 2), (5, 1, 2), (6, 1, 2)]);

        let from_y = large_node_surface_y(&world, 0, 0);
        let to_y = large_node_surface_y(&world, 5, 0);
        assert_eq!(from_y, Some(1));
        assert_eq!(to_y, Some(3));

        assert!(
            !is_large_edge_valid(&world, (0, 0), (5, 0)),
            "Edge between nodes at heights 1 and 3 should be invalid",
        );
    }

    #[test]
    fn large_edge_distance_includes_height() {
        // Edge between anchors at different heights should include dy in distance.
        // Same ramp as large_edge_valid_between_different_heights.
        // Anchor (0, 0) at surface_y=1, anchor (1, 0) at surface_y=2.
        // dx=1, dy=1, dz=0 → distance = sqrt(1+1+0) = sqrt(2) ≈ 1.414.
        let world = hilly_world(
            10,
            10,
            10,
            &[
                (2, 0, 1),
                (2, 1, 1),
                (3, 0, 1),
                (3, 1, 1),
                (4, 0, 1),
                (4, 1, 1),
                (5, 0, 1),
                (5, 1, 1),
                (6, 0, 1),
                (6, 1, 1),
                (7, 0, 1),
                (7, 1, 1),
                (8, 0, 1),
                (8, 1, 1),
                (9, 0, 1),
                (9, 1, 1),
            ],
        );
        let graph = build_large_nav_graph(&world);

        // Find node at anchor (0, 0) with surface_y=1.
        let from_node = graph
            .live_nodes()
            .find(|n| n.position.x == 0 && n.position.z == 0 && n.position.y == 1)
            .expect("Should have node at anchor (0, 0)");
        let to_pos = VoxelCoord::new(1, 2, 0);

        let edge = from_node.edge_indices.iter().find_map(|&idx| {
            let e = graph.edge(idx);
            if graph.node(e.to).position == to_pos {
                Some(e)
            } else {
                None
            }
        });
        let edge = edge.expect("Should have edge from (0,0) to (1,0)");

        // dx=1, dy=1, dz=0 → scaled_distance(1,1,0) = isqrt(2 * 1024²) = 1448
        let expected = scaled_distance(1, 1, 0);
        assert_eq!(
            edge.distance, expected,
            "Edge distance should be scaled sqrt(2) = {expected}, got {}",
            edge.distance,
        );
    }

    #[test]
    fn scaled_distance_known_values() {
        // Adjacent: sqrt(1) * 1024 = 1024
        assert_eq!(scaled_distance(1, 0, 0), DIST_SCALE);
        // 2D diagonal: sqrt(2) * 1024 ≈ 1448
        assert_eq!(scaled_distance(1, 1, 0), 1448);
        // 3D diagonal: floor(sqrt(3) * 1024) = 1773
        assert_eq!(scaled_distance(1, 1, 1), 1773);
        // Zero distance.
        assert_eq!(scaled_distance(0, 0, 0), 0);
        // Negative deltas (same magnitude).
        assert_eq!(scaled_distance(-1, -1, 0), scaled_distance(1, 1, 0));
    }

    #[test]
    fn scaled_distance_large_coords_no_overflow() {
        // Maximum world distance: dx=255, dy=127, dz=255.
        let d = scaled_distance(255, 127, 255);
        // sqrt(255² + 127² + 255²) ≈ 383.8, × 1024 ≈ 393,036
        assert!(d > 390_000 && d < 400_000, "Expected ~393k, got {d}");
    }

    // --- Building face awareness tests ---

    /// Helper: create a small world with a foundation and building interior.
    fn make_building_world() -> (VoxelWorld, BTreeMap<VoxelCoord, FaceData>) {
        use crate::building::compute_building_face_layout;
        let mut world = VoxelWorld::new(16, 16, 16);
        // Lay solid foundation at y=0.
        for x in 3..6 {
            for z in 3..6 {
                world.set(VoxelCoord::new(x, 0, z), VoxelType::ForestFloor);
            }
        }
        // Place 3x3x1 building interior at y=1.
        let anchor = VoxelCoord::new(3, 0, 3);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);
        for &coord in layout.keys() {
            world.set(coord, VoxelType::BuildingInterior);
        }
        (world, layout)
    }

    #[test]
    fn building_interior_creates_nav_node() {
        let (world, faces) = make_building_world();
        let graph = build_nav_graph(&world, &faces);

        // All 9 interior voxels (3x3 at y=1) should be nav nodes.
        for x in 3..6 {
            for z in 3..6 {
                assert!(
                    graph.has_node_at(VoxelCoord::new(x, 1, z)),
                    "Expected nav node at ({x}, 1, {z})",
                );
            }
        }
    }

    #[test]
    fn wall_blocks_cardinal_edge() {
        let (world, faces) = make_building_world();
        let graph = build_nav_graph(&world, &faces);

        // (3,1,4) is on the NegX wall edge. Its NegX face is Window (blocking).
        // There should be no edge from (3,1,4) to (2,1,4) through the window.
        // Actually (2,1,4) may or may not be a node. Check the interior node
        // doesn't connect outside through a blocking face.
        let interior = VoxelCoord::new(3, 1, 4);
        let outside = VoxelCoord::new(2, 1, 4);

        // If outside is a node, there should be no edge between them.
        if graph.has_node_at(interior) && graph.has_node_at(outside) {
            let interior_id = graph
                .live_nodes()
                .find(|n| n.position == interior)
                .unwrap()
                .id;
            let has_edge_to_outside = graph.neighbors(interior_id).iter().any(|&idx| {
                graph.edge(idx).to.0 as usize != interior_id.0 as usize && {
                    let to = graph.node(graph.edge(idx).to);
                    to.position == outside
                }
            });
            assert!(
                !has_edge_to_outside,
                "Window face should block edge from interior to outside",
            );
        }
    }

    #[test]
    fn door_allows_cardinal_edge() {
        let (world, faces) = make_building_world();
        let graph = build_nav_graph(&world, &faces);

        // Door is at center of +Z edge: (4,1,5), facing PosZ.
        // The node at (4,1,5) should connect to (4,1,6) if that's a node.
        let door_inside = VoxelCoord::new(4, 1, 5);

        // The voxel outside the door at (4,1,6) should be a nav node if it has
        // the building wall as a surface. Check that the door node exists.
        assert!(
            graph.has_node_at(door_inside),
            "Door voxel should be a nav node",
        );

        // The door face (PosZ) is not blocking, so edges should exist
        // through it. Check that (4,1,5) has an edge going in the +Z direction.
        let door_id = graph
            .live_nodes()
            .find(|n| n.position == door_inside)
            .unwrap()
            .id;

        // Check if there's any neighbor in the +Z direction.
        let has_posz_edge = graph.neighbors(door_id).iter().any(|&idx| {
            let to_pos = graph.node(graph.edge(idx).to).position;
            to_pos.z > door_inside.z
        });
        assert!(has_posz_edge, "Door face should allow edge in +Z direction",);
    }

    #[test]
    fn wall_blocks_diagonal() {
        let (world, faces) = make_building_world();
        let graph = build_nav_graph(&world, &faces);

        // Corner (3,1,3): NegX=Window, NegZ=Window. Any diagonal going both
        // NegX and NegZ should be blocked.
        let corner = VoxelCoord::new(3, 1, 3);
        let diagonal = VoxelCoord::new(2, 1, 2);

        if graph.has_node_at(corner) && graph.has_node_at(diagonal) {
            let corner_id = graph
                .live_nodes()
                .find(|n| n.position == corner)
                .unwrap()
                .id;
            let has_diag_edge = graph
                .neighbors(corner_id)
                .iter()
                .any(|&idx| graph.node(graph.edge(idx).to).position == diagonal);
            assert!(!has_diag_edge, "Window faces should block diagonal edges",);
        }
    }

    #[test]
    fn open_interior_faces_allow_movement() {
        let (world, faces) = make_building_world();
        let graph = build_nav_graph(&world, &faces);

        // Interior voxel (4,1,4) has all-Open horizontal faces.
        // It should connect to all 4 cardinal neighbors inside the building.
        let center = VoxelCoord::new(4, 1, 4);
        assert!(graph.has_node_at(center));
        let center_id = graph
            .live_nodes()
            .find(|n| n.position == center)
            .unwrap()
            .id;

        let cardinal_neighbors = [
            VoxelCoord::new(5, 1, 4),
            VoxelCoord::new(3, 1, 4),
            VoxelCoord::new(4, 1, 5),
            VoxelCoord::new(4, 1, 3),
        ];
        for &expected in &cardinal_neighbors {
            assert!(graph.has_node_at(expected), "Expected node at {expected}");
            let has_edge = graph
                .neighbors(center_id)
                .iter()
                .any(|&idx| graph.node(graph.edge(idx).to).position == expected);
            assert!(
                has_edge,
                "Center should connect to interior neighbor at {expected}",
            );
        }
    }

    #[test]
    fn exterior_air_gets_node_from_building_wall() {
        let (world, faces) = make_building_world();
        let graph = build_nav_graph(&world, &faces);

        // Air at (2,1,3) is next to BuildingInterior (3,1,3) which has
        // NegX=Window (blocking). The window acts as a virtual surface,
        // so (2,1,3) should be a nav node.
        let outside = VoxelCoord::new(2, 1, 3);
        assert!(
            graph.has_node_at(outside),
            "Air next to building wall should become a nav node",
        );
    }

    #[test]
    fn building_surface_types_correct() {
        let (world, faces) = make_building_world();
        let graph = build_nav_graph(&world, &faces);

        // Interior voxel with Floor face: should have GrownPlatform surface.
        let interior = VoxelCoord::new(4, 1, 4);
        let node = graph.live_nodes().find(|n| n.position == interior).unwrap();
        assert_eq!(
            node.surface_type,
            VoxelType::GrownPlatform,
            "Building interior with Floor should be GrownPlatform",
        );
    }

    #[test]
    fn building_incremental_creates_node_at_coord() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // Foundation.
        for x in 3..6 {
            for z in 3..6 {
                world.set(VoxelCoord::new(x, 0, z), VoxelType::ForestFloor);
            }
        }

        let mut faces = BTreeMap::new();
        let mut graph = build_nav_graph(&world, &faces);

        // Place a single building interior voxel.
        let coord = VoxelCoord::new(4, 1, 4);
        world.set(coord, VoxelType::BuildingInterior);
        let mut fd = FaceData::default();
        fd.set(FaceDirection::NegY, FaceType::Floor);
        fd.set(FaceDirection::PosY, FaceType::Ceiling);
        fd.set(FaceDirection::PosX, FaceType::Window);
        fd.set(FaceDirection::NegX, FaceType::Window);
        fd.set(FaceDirection::PosZ, FaceType::Window);
        fd.set(FaceDirection::NegZ, FaceType::Window);
        faces.insert(coord, fd);

        graph.update_after_building_voxel_set(&world, &faces, coord);
        assert!(
            graph.has_node_at(coord),
            "Incremental update should create node at building voxel",
        );
    }

    #[test]
    fn building_incremental_matches_full_rebuild() {
        let mut world = VoxelWorld::new(16, 16, 16);
        for x in 3..6 {
            for z in 3..6 {
                world.set(VoxelCoord::new(x, 0, z), VoxelType::ForestFloor);
            }
        }

        let mut faces = BTreeMap::new();
        let mut graph = build_nav_graph(&world, &faces);

        // Place building interior voxels one by one.
        use crate::building::compute_building_face_layout;
        let anchor = VoxelCoord::new(3, 0, 3);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);
        for (&coord, fd) in &layout {
            world.set(coord, VoxelType::BuildingInterior);
            faces.insert(coord, fd.clone());
            graph.update_after_building_voxel_set(&world, &faces, coord);
        }

        // Full rebuild.
        let rebuilt = build_nav_graph(&world, &faces);

        // Compare node positions.
        let mut inc_positions: Vec<VoxelCoord> = graph.live_nodes().map(|n| n.position).collect();
        let mut full_positions: Vec<VoxelCoord> =
            rebuilt.live_nodes().map(|n| n.position).collect();
        inc_positions.sort();
        full_positions.sort();
        assert_eq!(
            inc_positions, full_positions,
            "Incremental building update should match full rebuild node positions",
        );

        // Compare edges.
        let mut inc_edges: Vec<(VoxelCoord, VoxelCoord)> = Vec::new();
        for node in graph.live_nodes() {
            for &edge_idx in &node.edge_indices {
                let edge = graph.edge(edge_idx);
                inc_edges.push((graph.node(edge.from).position, graph.node(edge.to).position));
            }
        }
        let mut full_edges: Vec<(VoxelCoord, VoxelCoord)> = Vec::new();
        for node in rebuilt.live_nodes() {
            for &edge_idx in &node.edge_indices {
                let edge = rebuilt.edge(edge_idx);
                full_edges.push((
                    rebuilt.node(edge.from).position,
                    rebuilt.node(edge.to).position,
                ));
            }
        }
        inc_edges.sort();
        full_edges.sort();
        assert_eq!(
            inc_edges, full_edges,
            "Incremental building update should match full rebuild edges",
        );
    }

    #[test]
    fn ladder_edge_types_derived_correctly() {
        use VoxelType::*;
        let a = VoxelCoord::new(0, 0, 0);
        let b = VoxelCoord::new(0, 1, 0);

        // Same ladder type → corresponding ladder climb.
        assert_eq!(
            derive_edge_type(WoodLadder, WoodLadder, a, b),
            EdgeType::WoodLadderClimb
        );
        assert_eq!(
            derive_edge_type(RopeLadder, RopeLadder, a, b),
            EdgeType::RopeLadderClimb
        );

        // Mixed ladder/non-ladder → BranchWalk (stepping on/off).
        assert_eq!(
            derive_edge_type(WoodLadder, Trunk, a, b),
            EdgeType::BranchWalk
        );
        assert_eq!(
            derive_edge_type(GrownPlatform, RopeLadder, a, b),
            EdgeType::BranchWalk
        );

        // Mixed ladder types → BranchWalk (one is ladder, other is different ladder).
        assert_eq!(
            derive_edge_type(WoodLadder, RopeLadder, a, b),
            EdgeType::BranchWalk
        );
    }

    #[test]
    fn ladder_surface_type_derived_correctly() {
        let mut world = VoxelWorld::new(16, 16, 16);
        let faces = no_faces();
        // Place a WoodLadder voxel with a solid neighbor below (floor) so it
        // qualifies as a nav node.
        let floor = VoxelCoord::new(5, 0, 5);
        let ladder = VoxelCoord::new(5, 1, 5);
        world.set(floor, VoxelType::ForestFloor);
        world.set(ladder, VoxelType::WoodLadder);

        let surface = derive_surface_type(&world, &faces, ladder);
        assert_eq!(surface, VoxelType::WoodLadder);
    }

    // --- RLE-aware scan range tests ---

    #[test]
    fn empty_world_produces_empty_nav_graph() {
        let world = VoxelWorld::new(16, 16, 16);
        let graph = build_nav_graph(&world, &no_faces());
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn neighbor_column_solid_extends_scan_range() {
        // A solid voxel in column (5, 5) should cause the adjacent all-Air
        // column (6, 5) to produce a nav node at the same Y, since the air
        // voxel at (6, y, 5) is face-adjacent to solid at (5, y, 5).
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 3, 5), VoxelType::Trunk);
        let graph = build_nav_graph(&world, &no_faces());

        // The air voxels face-adjacent to the trunk at (5,3,5) should be nav nodes.
        assert!(graph.has_node_at(VoxelCoord::new(6, 3, 5))); // +X neighbor
        assert!(graph.has_node_at(VoxelCoord::new(4, 3, 5))); // -X neighbor
        assert!(graph.has_node_at(VoxelCoord::new(5, 3, 6))); // +Z neighbor
        assert!(graph.has_node_at(VoxelCoord::new(5, 3, 4))); // -Z neighbor
        assert!(graph.has_node_at(VoxelCoord::new(5, 4, 5))); // +Y neighbor
        // (5, 2, 5) is at y=2 which is ≥1, so it's also a nav node.
        assert!(graph.has_node_at(VoxelCoord::new(5, 2, 5))); // -Y neighbor
    }

    #[test]
    fn rle_aware_build_matches_brute_force_node_set() {
        // Build a nav graph on a world with a tree and verify that every air
        // voxel adjacent to solid has a nav node (and no others do).
        let world = test_world();
        let graph = build_nav_graph(&world, &no_faces());

        let sx = world.size_x as usize;
        let sy = world.size_y as usize;
        let sz = world.size_z as usize;

        // Check a sample of positions to verify agreement between has_node_at
        // and should_be_nav_node.
        for y in 1..sy {
            for z in 0..sz {
                for x in 0..sx {
                    let coord = VoxelCoord::new(x as i32, y as i32, z as i32);
                    let expected = should_be_nav_node(&world, &no_faces(), coord);
                    assert_eq!(
                        graph.has_node_at(coord),
                        expected,
                        "Mismatch at ({x}, {y}, {z}): has_node_at={}, should_be={}",
                        graph.has_node_at(coord),
                        expected,
                    );
                }
            }
        }
    }
}
