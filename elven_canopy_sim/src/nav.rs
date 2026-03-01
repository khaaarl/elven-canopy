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
// least one solid voxel becomes a nav node. `BuildingInterior` voxels are
// always nav nodes (face data provides surfaces). Air voxels adjacent to a
// `BuildingInterior` face that blocks movement also become nav nodes. Edges
// connect 26-neighbors among nav nodes, subject to face-blocking checks
// (`is_edge_blocked_by_faces()`). This means the nav graph reflects actual
// world geometry — construction changes the navigable topology via
// incremental updates.
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
// **Dual-graph pattern for large creatures.** `build_large_nav_graph()` builds
// a separate `NavGraph` for 2x2x2 footprint creatures (e.g. elephants). Nodes
// exist at anchor `(x, y, z)` where the 2x2 ground footprint has solid voxels
// within 1 voxel of height variation and 2 voxels of air clearance above the
// highest ground point. Edges connect 8-neighbors with union-footprint
// clearance checks, allowing up to 1 voxel of height change between adjacent
// nodes. The large graph uses the same `NavGraph` struct so all existing
// pathfinding code works unchanged. `sim.rs` stores both graphs and dispatches
// via `graph_for_species()` based on the species' footprint.
//
// All storage uses `Vec` indexed by `NavNodeId`/`NavEdgeId` for O(1) lookup
// and deterministic iteration order. No `HashMap`.
//
// See also: `world.rs` for the voxel grid, `tree_gen.rs` for tree geometry,
// `pathfinding.rs` for A* search over this graph, `sim.rs` which owns the
// `NavGraph` as part of `SimState`, `species.rs` for the `footprint` field.
//
// **Critical constraint: determinism.** The graph is built by iterating voxels
// in fixed order (matching the flat index of `VoxelWorld`). Node/edge IDs are
// sequential integers assigned in that order. Incremental updates are also
// deterministic — they process affected positions in a fixed order.

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
        face_data: &BTreeMap<VoxelCoord, FaceData>,
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
            let should_be_node = should_be_nav_node(world, face_data, pos);
            let flat = match self.flat_index(pos) {
                Some(f) => f,
                None => continue,
            };
            let current_slot = self.spatial_index[flat];
            let is_node = current_slot != u32::MAX;

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
                let surface = derive_surface_type(world, face_data, pos);
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

                        // Check if face data blocks this edge.
                        if is_edge_blocked_by_faces(face_data, pos, np) {
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
            if self.flat_index(neighbor).is_some() {
                affected.push(neighbor);
            }
        }

        // Step 2: For each affected position, add/remove/update nav node.
        for &pos in &affected {
            let should_exist = should_be_nav_node(world, face_data, pos);
            let flat = match self.flat_index(pos) {
                Some(f) => f,
                None => continue,
            };
            let current_slot = self.spatial_index[flat];
            let is_node = current_slot != u32::MAX;

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
                self.spatial_index[flat] = slot as u32;
            } else if !should_exist && is_node {
                let slot = current_slot as usize;
                let id = NavNodeId(current_slot);
                removed_ids.push(id);
                self.nodes[slot] = None;
                self.spatial_index[flat] = u32::MAX;
                self.free_slots.push(slot);
            } else if should_exist && is_node {
                let surface = derive_surface_type(world, face_data, pos);
                if let Some(node) = self.nodes[current_slot as usize].as_mut() {
                    node.surface_type = surface;
                }
            }
        }

        // Steps 3-5: Same dirty-set + edge recomputation as
        // update_after_voxel_solidified.
        let mut dirty_set: Vec<usize> = Vec::new();
        let mut is_dirty = vec![false; self.nodes.len()];
        for &pos in &affected {
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

                        if is_dirty[ns] && ns < slot {
                            continue;
                        }

                        if is_edge_blocked_by_faces(face_data, pos, np) {
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
    if voxel == VoxelType::BuildingInterior {
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
            Branch | Leaf | Fruit | GrownPlatform | Bridge | Root | BuildingInterior => {
                EdgeType::BranchWalk
            }
            GrownStairs | GrownWall => EdgeType::TrunkClimb,
            Air => EdgeType::BranchWalk, // shouldn't happen
        };
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
pub fn build_nav_graph(world: &VoxelWorld, face_data: &BTreeMap<VoxelCoord, FaceData>) -> NavGraph {
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
                if !should_be_nav_node(world, face_data, coord) {
                    continue;
                }

                let surface = derive_surface_type(world, face_data, coord);
                let node_id = graph.add_node(coord, surface);
                let flat_idx = x + z * sx + y * sx * sz;
                graph.spatial_index[flat_idx] = node_id.0;
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

                    // Check if face data blocks this edge.
                    if is_edge_blocked_by_faces(face_data, from_node.position, to_node.position) {
                        continue;
                    }

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

// ---------------------------------------------------------------------------
// Large creature nav graph (2x2x2 footprint)
// ---------------------------------------------------------------------------

/// Find the surface y for a 2x2 large-creature footprint at anchor (ax, az).
///
/// Scans each of the 4 columns to find the topmost solid voxel. Returns
/// `None` if any column has no solid ground or if the height variation
/// across the 4 columns exceeds 1 voxel. Otherwise returns `max_surface + 1`
/// (the air layer above the highest ground point — the creature stands at
/// its tallest point, straddling any minor unevenness).
fn large_node_surface_y(world: &VoxelWorld, ax: i32, az: i32) -> Option<i32> {
    let sy = world.size_y as i32;
    let mut min_surface = i32::MAX;
    let mut max_surface = i32::MIN;
    for dz in 0..2 {
        for dx in 0..2 {
            let mut found = false;
            for y in (0..sy).rev() {
                if world.get(VoxelCoord::new(ax + dx, y, az + dz)).is_solid() {
                    min_surface = min_surface.min(y);
                    max_surface = max_surface.max(y);
                    found = true;
                    break;
                }
            }
            if !found {
                return None; // No solid ground in this column.
            }
        }
    }
    if max_surface - min_surface > 1 {
        return None; // Height variation exceeds 1-voxel tolerance.
    }
    Some(max_surface + 1)
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
            let mut found = false;
            for y in (0..sy).rev() {
                if world.get(VoxelCoord::new(x, y, z)).is_solid() {
                    union_min_surface = union_min_surface.min(y);
                    union_max_surface = union_max_surface.max(y);
                    found = true;
                    break;
                }
            }
            if !found {
                return false; // No ground in this column.
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

/// Build a navigation graph for large (2x2x2 footprint) creatures.
///
/// Nodes exist at anchor positions `(x, y, z)` where the 2x2 ground footprint
/// has solid voxels within 1 voxel of height variation and 2 voxels of air
/// clearance above the highest ground point. The node y is `max_surface + 1`
/// (the creature stands at its tallest point, straddling minor unevenness).
///
/// Edges connect horizontal 8-neighbors (dx,dz in {-1,0,1}²\{0,0}), allowing
/// up to 1 voxel of height change between adjacent nodes. Edge distances use
/// `sqrt(dx² + dy² + dz²)` so height-changing edges are slightly more costly.
///
/// All edges are `ForestFloor` type since large creatures are ground-only.
/// The resulting graph uses the same `NavGraph` struct as the standard graph,
/// so all existing pathfinding code works unchanged.
pub fn build_large_nav_graph(world: &VoxelWorld) -> NavGraph {
    let mut graph = NavGraph::new();

    let sx = world.size_x as usize;
    let sy = world.size_y as usize;
    let sz = world.size_z as usize;
    let total = sx * sy * sz;

    if total == 0 || sx < 2 || sz < 2 || sy < 3 {
        return graph;
    }

    graph.world_size = (sx, sy, sz);
    graph.spatial_index = vec![u32::MAX; total];

    // Pass 1: create nodes.
    // Large nodes live at the air layer above ground. Iterate in flat-index order.
    for z in 0..sz.saturating_sub(1) {
        for x in 0..sx.saturating_sub(1) {
            if !is_large_node_valid(world, x as i32, z as i32) {
                continue;
            }
            let air_y = large_node_surface_y(world, x as i32, z as i32).unwrap();
            let coord = VoxelCoord::new(x as i32, air_y, z as i32);
            let node_id = graph.add_node(coord, VoxelType::ForestFloor);
            let flat_idx = x + z * sx + air_y as usize * sx * sz;
            graph.spatial_index[flat_idx] = node_id.0;
        }
    }

    // Pass 2: create edges.
    // Use 4 positive-half horizontal offsets to avoid duplicate edges.
    #[rustfmt::skip]
    let positive_half: [(i32, i32); 4] = [
        ( 1, -1), ( 1, 0), ( 1, 1),
        ( 0,  1),
    ];

    for z in 0..sz.saturating_sub(1) {
        for x in 0..sx.saturating_sub(1) {
            // Look up surface y for this anchor.
            let air_y = match large_node_surface_y(world, x as i32, z as i32) {
                Some(y) => y as usize,
                None => continue,
            };
            let flat_idx = x + z * sx + air_y * sx * sz;
            if flat_idx >= graph.spatial_index.len() {
                continue;
            }
            let from_slot = graph.spatial_index[flat_idx];
            if from_slot == u32::MAX {
                continue;
            }

            for &(dx, dz) in &positive_half {
                let nx = x as i32 + dx;
                let nz = z as i32 + dz;
                if nx < 0 || nz < 0 {
                    continue;
                }
                let (nxu, nzu) = (nx as usize, nz as usize);
                if nxu + 1 >= sx || nzu + 1 >= sz {
                    continue;
                }

                let n_air_y = match large_node_surface_y(world, nx, nz) {
                    Some(y) => y as usize,
                    None => continue,
                };
                let n_flat = nxu + nzu * sx + n_air_y * sx * sz;
                if n_flat >= graph.spatial_index.len() {
                    continue;
                }
                let to_slot = graph.spatial_index[n_flat];
                if to_slot == u32::MAX {
                    continue;
                }

                if !is_large_edge_valid(world, (x as i32, z as i32), (nx, nz)) {
                    continue;
                }

                let from = NavNodeId(from_slot);
                let to = NavNodeId(to_slot);
                let dy = n_air_y as i32 - air_y as i32;
                let dist = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                graph.add_edge(from, to, EdgeType::ForestFloor, dist);
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

    // Step 1: Update nodes at affected anchors.
    for &(ax, az) in &affected_anchors {
        let should_exist = is_large_node_valid(world, ax, az);

        // Find existing node — try current surface y and previous known y.
        // We need to check the spatial index at any y that might have a node.
        let mut existing_flat = None;
        let total = sx * graph.world_size.1 * sz;
        for y in 0..graph.world_size.1 {
            let flat = ax as usize + az as usize * sx + y * sx * sz;
            if flat < total && graph.spatial_index[flat] != u32::MAX {
                existing_flat = Some(flat);
                break;
            }
        }

        let current_slot = existing_flat.map_or(u32::MAX, |f| graph.spatial_index[f]);
        let is_node = current_slot != u32::MAX;

        if !should_exist && is_node {
            let id = NavNodeId(current_slot);
            removed_ids.push(id);
            graph.nodes[current_slot as usize] = None;
            if let Some(flat) = existing_flat {
                graph.spatial_index[flat] = u32::MAX;
            }
            graph.free_slots.push(current_slot as usize);
        } else if should_exist && is_node {
            // Node exists — check if it needs to move to a different y
            // (surface height may have changed due to height tolerance).
            let expected_air_y = large_node_surface_y(world, ax, az).unwrap();
            let existing_y = graph.nodes[current_slot as usize]
                .as_ref()
                .unwrap()
                .position
                .y;
            if existing_y != expected_air_y {
                // Remove old node at wrong y.
                let id = NavNodeId(current_slot);
                removed_ids.push(id);
                graph.nodes[current_slot as usize] = None;
                if let Some(flat) = existing_flat {
                    graph.spatial_index[flat] = u32::MAX;
                }
                graph.free_slots.push(current_slot as usize);
                // Add new node at correct y.
                let anchor_coord = VoxelCoord::new(ax, expected_air_y, az);
                let flat = ax as usize + az as usize * sx + expected_air_y as usize * sx * sz;
                let slot = if let Some(free) = graph.free_slots.pop() {
                    let id = NavNodeId(free as u32);
                    graph.nodes[free] = Some(NavNode {
                        id,
                        position: anchor_coord,
                        surface_type: VoxelType::ForestFloor,
                        edge_indices: Vec::new(),
                    });
                    free
                } else {
                    let slot = graph.nodes.len();
                    let id = NavNodeId(slot as u32);
                    graph.nodes.push(Some(NavNode {
                        id,
                        position: anchor_coord,
                        surface_type: VoxelType::ForestFloor,
                        edge_indices: Vec::new(),
                    }));
                    slot
                };
                graph.spatial_index[flat] = slot as u32;
            }
        } else if should_exist && !is_node {
            let air_y = large_node_surface_y(world, ax, az).unwrap();
            let anchor_coord = VoxelCoord::new(ax, air_y, az);
            let flat = ax as usize + az as usize * sx + air_y as usize * sx * sz;
            let slot = if let Some(free) = graph.free_slots.pop() {
                let id = NavNodeId(free as u32);
                graph.nodes[free] = Some(NavNode {
                    id,
                    position: anchor_coord,
                    surface_type: VoxelType::ForestFloor,
                    edge_indices: Vec::new(),
                });
                free
            } else {
                let slot = graph.nodes.len();
                let id = NavNodeId(slot as u32);
                graph.nodes.push(Some(NavNode {
                    id,
                    position: anchor_coord,
                    surface_type: VoxelType::ForestFloor,
                    edge_indices: Vec::new(),
                }));
                slot
            };
            graph.spatial_index[flat] = slot as u32;
        }
    }

    // Step 2: Collect dirty set — affected anchors + their 8 horizontal neighbors.
    let mut dirty_set: Vec<usize> = Vec::new();
    let mut is_dirty = vec![false; graph.nodes.len()];
    let total = sx * graph.world_size.1 * sz;

    for &(ax, az) in &affected_anchors {
        for dz in -1i32..=1 {
            for dx in -1i32..=1 {
                let nx = ax + dx;
                let nz = az + dz;
                if nx < 0 || nz < 0 || (nx as usize) + 1 >= sx || (nz as usize) + 1 >= sz {
                    continue;
                }
                // Find the node at any y for this (nx, nz).
                for y in 0..graph.world_size.1 {
                    let flat = nx as usize + nz as usize * sx + y * sx * sz;
                    if flat < total {
                        let slot = graph.spatial_index[flat];
                        if slot != u32::MAX {
                            let s = slot as usize;
                            if s < is_dirty.len() && !is_dirty[s] {
                                is_dirty[s] = true;
                                dirty_set.push(s);
                            }
                            break;
                        }
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
            let mut n_slot = u32::MAX;
            for y in 0..graph.world_size.1 {
                let n_flat = nx as usize + nz as usize * sx + y * sx * sz;
                if n_flat < total {
                    let s = graph.spatial_index[n_flat];
                    if s != u32::MAX {
                        n_slot = s;
                        break;
                    }
                }
            }
            if n_slot == u32::MAX {
                continue;
            }
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
            let dist = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
            graph.add_edge(from_id, to_id, EdgeType::ForestFloor, dist);
        }
    }

    removed_ids
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_world() -> VoxelWorld {
        use crate::config::GameConfig;
        use crate::prng::GameRng;
        use crate::tree_gen;

        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        // Disable leaves and terrain for basic nav tests.
        config.tree_profile.leaves.canopy_density = 0.0;
        config.terrain_max_height = 0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

        world
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
            ..GameConfig::default()
        };
        // High split chance to test connectivity with many branches.
        config.tree_profile.split.split_chance_base = 1.0;
        config.tree_profile.split.min_progress_for_split = 0.05;
        config.tree_profile.leaves.canopy_density = 0.0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

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
        let graph_a = build_nav_graph(&world_a, &no_faces());

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(42);
        tree_gen::generate_tree(&mut world_b, &config, &mut rng_b);
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
            ..GameConfig::default()
        };

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

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
            ..GameConfig::default()
        };
        config.tree_profile.roots.root_energy_fraction = 0.2;
        config.tree_profile.roots.root_initial_count = 5;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        tree_gen::generate_tree(&mut world, &config, &mut rng);

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

        // dx=1, dy=1, dz=0 → sqrt(2)
        let expected = (2.0_f32).sqrt();
        assert!(
            (edge.distance - expected).abs() < 0.001,
            "Edge distance should be sqrt(2) ≈ {expected}, got {}",
            edge.distance,
        );
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
}
