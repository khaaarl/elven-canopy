// Smooth mesh generation pipeline for solid voxel surfaces.
//
// Transforms the blocky per-face voxel mesh into a subdivided, chamfered,
// and iteratively smoothed mesh. The pipeline operates on an intermediate
// `SmoothMesh` representation that tracks vertex connectivity (needed for
// 1-ring neighbor queries), then flattens back to `SurfaceMesh` format.
//
// The pipeline stages are:
// 1. Face subdivision: each visible solid face → 8 triangles (4 corners,
//    4 edge midpoints, 1 center). Vertices are deduplicated by pre-smoothing
//    grid position.
// 2. Anchoring: mark vertices that must not move (face centers, vertices
//    adjacent to non-solid voxels like `BuildingInterior`, vertices with
//    insufficient anchored neighbors).
// 3. Chamfer: non-anchored vertices with ≥2 anchored neighbors move toward
//    the centroid of their anchored neighbors. Solid vertices first, then
//    leaf-only vertices (so solid geometry is independent of leaves).
// 4. Curvature-minimizing smoothing: iterative Jacobi-style passes that
//    minimize total squared Laplacian pointiness in each vertex's 1-ring
//    neighborhood.
// 5. Vertex normals: area-weighted average of incident triangle normals.
//
// All solid opaque voxel types (Trunk, Branch, Root, Dirt, GrownPlatform,
// GrownWall, Strut) and Leaf voxels participate. Leaf↔leaf faces are culled
// (shell-only). Chamfer and smoothing process solid vertices first, then
// leaf-only vertices. Per-vertex `has_solid_face` / `has_leaf_face` flags
// classify vertices for ordered processing.
//
// See `docs/drafts/visual_smooth.md` for the full design document.
// See `mesh_gen.rs` for the mesh generation entry point and the
// `SurfaceMesh` / `ChunkMesh` output types.

use std::collections::BTreeMap;

use crate::mesh_gen::SurfaceMesh;

/// A vertex in the smooth mesh intermediate representation.
#[derive(Clone, Debug)]
pub struct SmoothVertex {
    /// Current position (modified by chamfer and smoothing passes).
    pub position: [f32; 3],
    /// Whether this vertex is anchored (fixed in place).
    pub anchored: bool,
    /// Whether this vertex is a face center (set during construction).
    /// Used by the anchoring pass — face centers are always anchored.
    pub is_face_center: bool,
    /// Whether this vertex belongs to at least one solid (non-leaf) face.
    pub has_solid_face: bool,
    /// Whether this vertex belongs to at least one leaf face.
    pub has_leaf_face: bool,
    /// Initial normal — the normalized average of all contributing face
    /// normals. For single-face vertices this is the axis-aligned face normal.
    /// For shared vertices it blends the normals of all incident faces.
    pub initial_normal: [f32; 3],
    /// Edge-connected neighbor vertex indices (the 1-ring).
    pub neighbors: Vec<u32>,
}

/// A key for vertex deduplication. Uses doubled integer coordinates to avoid
/// float keys: a corner at (1, 2, 3) becomes (2, 4, 6), an edge midpoint at
/// (1.5, 2, 3) becomes (3, 4, 6). This gives exact comparison for all
/// positions on the half-integer grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct VertexKey {
    /// 2x the x coordinate.
    x2: i32,
    /// 2x the y coordinate.
    y2: i32,
    /// 2x the z coordinate.
    z2: i32,
}

impl VertexKey {
    fn from_position(pos: [f32; 3]) -> Self {
        Self {
            x2: (pos[0] * 2.0).round() as i32,
            y2: (pos[1] * 2.0).round() as i32,
            z2: (pos[2] * 2.0).round() as i32,
        }
    }
}

/// Per-triangle surface tag identifying which material surface a triangle
/// belongs to. Used to split a unified smooth mesh into separate
/// `SurfaceMesh` outputs (bark vs ground) after smoothing.
pub type SurfaceTag = u8;

/// Surface tag for bark voxels (Trunk, Branch, Root, construction types).
pub const TAG_BARK: SurfaceTag = 0;
/// Surface tag for ground voxels (Dirt).
pub const TAG_GROUND: SurfaceTag = 1;
/// Surface tag for leaf voxels.
pub const TAG_LEAF: SurfaceTag = 2;

/// Intermediate mesh representation with vertex connectivity, used for the
/// smoothing pipeline. Built by adding subdivided faces, then processed
/// through anchoring, chamfer, and smoothing passes before being flattened
/// to `SurfaceMesh` output.
#[derive(Clone, Debug)]
pub struct SmoothMesh {
    /// All vertices in the mesh.
    pub vertices: Vec<SmoothVertex>,
    /// Triangles as triples of vertex indices.
    pub triangles: Vec<[u32; 3]>,
    /// Per-triangle surface tag (bark=0, ground=1, leaf=2). Parallel to `triangles`.
    pub triangle_tags: Vec<SurfaceTag>,
    /// Per-triangle color. Parallel to `triangles`. Each triangle's vertices
    /// get this color in the output surface mesh.
    pub triangle_colors: Vec<[f32; 4]>,
    /// Per-triangle source voxel position. Used for spatial filtering so
    /// each triangle is emitted by exactly the chunk that owns its source
    /// voxel, regardless of where smoothing moved the vertices.
    pub triangle_voxel_pos: Vec<[i32; 3]>,
    /// Deduplication map: grid position → vertex index.
    dedup: BTreeMap<VertexKey, u32>,
}

impl Default for SmoothMesh {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothMesh {
    /// Create an empty smooth mesh.
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
            triangle_tags: Vec::new(),
            triangle_colors: Vec::new(),
            triangle_voxel_pos: Vec::new(),
            dedup: BTreeMap::new(),
        }
    }

    /// Get or create a vertex at the given position with the given face
    /// normal. If a vertex already exists at this position (via
    /// deduplication), its initial normal is updated to the accumulated
    /// sum of all contributing face normals (normalized later).
    fn get_or_create_vertex(&mut self, position: [f32; 3], face_normal: [f32; 3]) -> u32 {
        let key = VertexKey::from_position(position);
        if let Some(&idx) = self.dedup.get(&key) {
            let v = &mut self.vertices[idx as usize];
            v.initial_normal[0] += face_normal[0];
            v.initial_normal[1] += face_normal[1];
            v.initial_normal[2] += face_normal[2];
            idx
        } else {
            let idx = self.vertices.len() as u32;
            self.vertices.push(SmoothVertex {
                position,
                anchored: false,
                is_face_center: false,
                has_solid_face: false,
                has_leaf_face: false,
                initial_normal: face_normal,
                neighbors: Vec::new(),
            });
            self.dedup.insert(key, idx);
            idx
        }
    }

    /// Add an edge between two vertices (bidirectional). Skips if already
    /// connected.
    fn add_edge(&mut self, a: u32, b: u32) {
        if !self.vertices[a as usize].neighbors.contains(&b) {
            self.vertices[a as usize].neighbors.push(b);
        }
        if !self.vertices[b as usize].neighbors.contains(&a) {
            self.vertices[b as usize].neighbors.push(a);
        }
    }

    /// Add a subdivided face (8 triangles) for a solid voxel face.
    ///
    /// `corners` are the 4 corner positions in CCW winding order (as viewed
    /// from outside). `face_normal` is the outward-facing axis-aligned normal.
    /// `color` is the voxel's vertex color (stored per-triangle, not per-vertex,
    /// since a shared vertex can belong to faces of different voxel types).
    /// `tag` identifies the surface material (bark vs ground) for splitting
    /// the output into separate `SurfaceMesh`es.
    ///
    /// Returns the 9 vertex indices used: `[c0, c1, c2, c3, m01, m12, m23,
    /// m30, center]`. The caller can use these to anchor vertices (e.g., when
    /// the face borders a non-solid constructed voxel).
    pub fn add_subdivided_face(
        &mut self,
        corners: [[f32; 3]; 4],
        face_normal: [f32; 3],
        color: [f32; 4],
        tag: SurfaceTag,
        voxel_pos: [i32; 3],
    ) -> [u32; 9] {
        // Compute edge midpoints and face center.
        let midpoints: [[f32; 3]; 4] = [
            mid(corners[0], corners[1]),
            mid(corners[1], corners[2]),
            mid(corners[2], corners[3]),
            mid(corners[3], corners[0]),
        ];
        let center = [
            (corners[0][0] + corners[1][0] + corners[2][0] + corners[3][0]) * 0.25,
            (corners[0][1] + corners[1][1] + corners[2][1] + corners[3][1]) * 0.25,
            (corners[0][2] + corners[1][2] + corners[2][2] + corners[3][2]) * 0.25,
        ];

        // Get or create all 9 vertices.
        let c0 = self.get_or_create_vertex(corners[0], face_normal);
        let c1 = self.get_or_create_vertex(corners[1], face_normal);
        let c2 = self.get_or_create_vertex(corners[2], face_normal);
        let c3 = self.get_or_create_vertex(corners[3], face_normal);
        let m01 = self.get_or_create_vertex(midpoints[0], face_normal);
        let m12 = self.get_or_create_vertex(midpoints[1], face_normal);
        let m23 = self.get_or_create_vertex(midpoints[2], face_normal);
        let m30 = self.get_or_create_vertex(midpoints[3], face_normal);
        let ct = self.get_or_create_vertex(center, face_normal);
        self.vertices[ct as usize].is_face_center = true;

        // 8 triangles radiating from center (CCW winding from outside):
        // Each slice: center, perimeter_a, perimeter_b
        // Going around: c0, m01, c1, m12, c2, m23, c3, m30
        // Mark all 9 vertices with solid/leaf participation.
        let is_leaf = tag == TAG_LEAF;
        for &vi in &[c0, c1, c2, c3, m01, m12, m23, m30, ct] {
            if is_leaf {
                self.vertices[vi as usize].has_leaf_face = true;
            } else {
                self.vertices[vi as usize].has_solid_face = true;
            }
        }

        let ring = [c0, m01, c1, m12, c2, m23, c3, m30];
        for i in 0..8 {
            let a = ring[i];
            let b = ring[(i + 1) % 8];
            self.triangles.push([ct, a, b]);
            self.triangle_tags.push(tag);
            self.triangle_colors.push(color);
            self.triangle_voxel_pos.push(voxel_pos);

            // Add edges: center↔a, center↔b, a↔b
            self.add_edge(ct, a);
            self.add_edge(ct, b);
            self.add_edge(a, b);
        }

        [c0, c1, c2, c3, m01, m12, m23, m30, ct]
    }

    /// Apply all three anchoring rules in order:
    /// 1. Face centers are anchored.
    /// 2. (Caller handles non-solid adjacency by calling `anchor_vertices`.)
    /// 3. Vertices with <2 anchored neighbors are anchored.
    ///
    /// Rule 2 is applied by the caller between face construction and this
    /// method, using the vertex indices returned by `add_subdivided_face`.
    /// This method applies rules 1 and 3.
    pub fn apply_anchoring(&mut self) {
        // Rule 1: face centers are anchored.
        for v in &mut self.vertices {
            if v.is_face_center {
                v.anchored = true;
            }
        }

        // Rule 3: iteratively anchor low-valence vertices with <2 anchored
        // neighbors. Only vertices with total valence < 4 are candidates —
        // interior vertices (valence 4-8) always have enough connectivity for
        // stable smoothing (anchored face centers are within 2 hops). The
        // loop converges in 1-2 passes as newly anchored vertices provide
        // anchored-neighbor support to their own neighbors.
        loop {
            let mut changed = false;
            for i in 0..self.vertices.len() {
                if self.vertices[i].anchored || self.vertices[i].neighbors.len() >= 4 {
                    continue;
                }
                let anchored_neighbor_count = self.vertices[i]
                    .neighbors
                    .iter()
                    .filter(|&&n| self.vertices[n as usize].anchored)
                    .count();
                if anchored_neighbor_count < 2 {
                    self.vertices[i].anchored = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Mark specific vertices as anchored. Used by the caller to implement
    /// anchoring rule 2 (faces adjacent to non-solid constructed voxels).
    pub fn anchor_vertices(&mut self, indices: &[u32]) {
        for &idx in indices {
            self.vertices[idx as usize].anchored = true;
        }
    }

    /// Apply the chamfer pass in two phases:
    /// 1. Solid vertices first (leaf-only vertices ignored as neighbors)
    /// 2. Leaf-only vertices (see already-chamfered solid as anchors)
    ///
    /// This ensures solid geometry is identical whether leaves exist or not.
    ///
    /// Skip vertices with 4+ anchored neighbors where 3+ share the same
    /// coordinate on any axis — diagonal staircase saddle-skip heuristic.
    pub fn chamfer(&mut self) {
        // Phase 1: chamfer solid vertices (has_solid_face = true).
        // Only consider anchored neighbors that have solid faces —
        // leaf-only anchored vertices are invisible to this pass.
        let mut solid_displacements = vec![[0.0f32; 3]; self.vertices.len()];
        for (i, v) in self.vertices.iter().enumerate() {
            if v.anchored || !v.has_solid_face {
                continue;
            }
            if let Some(d) = self.compute_chamfer_offset(i, true) {
                solid_displacements[i] = d;
            }
        }
        // Apply solid displacements.
        for (i, v) in self.vertices.iter_mut().enumerate() {
            v.position[0] += solid_displacements[i][0];
            v.position[1] += solid_displacements[i][1];
            v.position[2] += solid_displacements[i][2];
        }

        // Phase 2: chamfer leaf-only vertices (has_leaf_face && !has_solid_face).
        // These see ALL anchored neighbors (including solid vertices that
        // just moved in phase 1).
        let mut leaf_displacements = vec![[0.0f32; 3]; self.vertices.len()];
        for (i, v) in self.vertices.iter().enumerate() {
            if v.anchored || v.has_solid_face || !v.has_leaf_face {
                continue;
            }
            if let Some(d) = self.compute_chamfer_offset(i, false) {
                leaf_displacements[i] = d;
            }
        }
        for (i, v) in self.vertices.iter_mut().enumerate() {
            v.position[0] += leaf_displacements[i][0];
            v.position[1] += leaf_displacements[i][1];
            v.position[2] += leaf_displacements[i][2];
        }
    }

    /// Compute chamfer displacement for a single vertex. If `solid_only`
    /// is true, only considers anchored neighbors with `has_solid_face`
    /// (ignoring leaf-only anchored vertices).
    fn compute_chamfer_offset(&self, vi: usize, solid_only: bool) -> Option<[f32; 3]> {
        let v = &self.vertices[vi];
        let mut sum = [0.0f32; 3];
        let mut count = 0u32;
        for &ni in &v.neighbors {
            let n = &self.vertices[ni as usize];
            if !n.anchored {
                continue;
            }
            if solid_only && !n.has_solid_face {
                continue;
            }
            sum[0] += n.position[0];
            sum[1] += n.position[1];
            sum[2] += n.position[2];
            count += 1;
        }
        if count < 2 {
            return None;
        }

        // Saddle-skip: 4+ anchored neighbors with 3+ sharing any axis.
        if count >= 4 {
            let pos = v.position;
            for (axis, &pos_val) in pos.iter().enumerate() {
                let mut same = 0u32;
                for &ni in &v.neighbors {
                    let n = &self.vertices[ni as usize];
                    if n.anchored
                        && (!solid_only || n.has_solid_face)
                        && (n.position[axis] - pos_val).abs() < 0.01
                    {
                        same += 1;
                    }
                }
                if same >= 3 {
                    return None;
                }
            }
        }

        let avg = [
            sum[0] / count as f32,
            sum[1] / count as f32,
            sum[2] / count as f32,
        ];
        Some([
            avg[0] - v.position[0],
            avg[1] - v.position[1],
            avg[2] - v.position[2],
        ])
    }

    /// Normalize all initial normals (which were accumulated from multiple
    /// face contributions during vertex creation). Call this after all faces
    /// have been added and before the chamfer pass.
    pub fn normalize_initial_normals(&mut self) {
        for v in &mut self.vertices {
            let len = (v.initial_normal[0] * v.initial_normal[0]
                + v.initial_normal[1] * v.initial_normal[1]
                + v.initial_normal[2] * v.initial_normal[2])
                .sqrt();
            if len > 1e-9 {
                v.initial_normal[0] /= len;
                v.initial_normal[1] /= len;
                v.initial_normal[2] /= len;
            }
        }
    }

    /// Run one iteration of curvature-minimizing smoothing. For each
    /// non-anchored vertex, samples 5 candidate positions along the vertex's
    /// current normal at offsets {-2d, -d, 0, +d, +2d}, picks the one that
    /// minimizes total squared Laplacian pointiness over the vertex and its
    /// 1-ring, then applies all displacements simultaneously (Jacobi-style).
    /// Run one smoothing iteration, only processing vertices that match
    /// the filter predicate. Used to process solid vertices before leaf.
    pub fn smooth_iteration_filtered(&mut self, d: f32, filter: impl Fn(&SmoothVertex) -> bool) {
        let offsets = [-2.0 * d, -d, 0.0, d, 2.0 * d];
        let vertex_count = self.vertices.len();
        let mut best_displacements: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]; vertex_count];

        // Build per-vertex triangle index for fast normal computation.
        let vertex_tris = self.build_vertex_triangle_index();

        for (vi, disp) in best_displacements.iter_mut().enumerate() {
            if self.vertices[vi].anchored || !filter(&self.vertices[vi]) {
                continue;
            }

            // Compute current vertex normal (area-weighted from incident
            // triangles). This is recalculated each iteration since positions
            // change.
            let normal = self.vertex_normal_fast(&vertex_tris[vi]);
            let normal_len =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            if normal_len < 1e-9 {
                continue;
            }
            let normal = [
                normal[0] / normal_len,
                normal[1] / normal_len,
                normal[2] / normal_len,
            ];

            let original_pos = self.vertices[vi].position;
            let mut best_cost = f32::MAX;
            let mut best_offset = 0.0f32;

            // Clone neighbors once outside the offset loop to avoid repeated
            // heap allocations in the inner loop.
            let neighbors: Vec<u32> = self.vertices[vi].neighbors.clone();

            // For each neighbor, record whether it's "above" or "below"
            // the vertex along the normal (sign of the dot product of
            // the neighbor-to-vertex offset with the normal). This
            // determines the local curvature sign. If all neighbors are
            // on the same side, it's purely convex or concave. Mixed
            // signs = saddle point.
            let neighbor_signs: Vec<(u32, bool)> = neighbors
                .iter()
                .map(|&ni| {
                    let np = self.vertices[ni as usize].position;
                    let to_neighbor = [
                        np[0] - original_pos[0],
                        np[1] - original_pos[1],
                        np[2] - original_pos[2],
                    ];
                    let dot = to_neighbor[0] * normal[0]
                        + to_neighbor[1] * normal[1]
                        + to_neighbor[2] * normal[2];
                    (ni, dot >= 0.0)
                })
                .collect();
            let has_positive = neighbor_signs.iter().any(|&(_, s)| s);
            let has_negative = neighbor_signs.iter().any(|&(_, s)| !s);
            let was_saddle = has_positive && has_negative;

            for &offset in &offsets {
                // Temporarily move vertex to candidate position.
                self.vertices[vi].position = [
                    original_pos[0] + offset * normal[0],
                    original_pos[1] + offset * normal[1],
                    original_pos[2] + offset * normal[2],
                ];

                // Evaluate total squared pointiness over vi and its 1-ring.
                let mut cost = 0.0f32;
                let k = self.laplacian_pointiness(vi as u32);
                cost += k * k;
                for &ni in &neighbors {
                    let k = self.laplacian_pointiness(ni);
                    cost += k * k;
                }

                // If the vertex was NOT a saddle point before, check if
                // this candidate position creates one. If so, reject it
                // by setting cost to infinity.
                if !was_saddle {
                    let candidate_pos = self.vertices[vi].position;
                    let mut cand_pos = false;
                    let mut cand_neg = false;
                    for &(ni, _) in &neighbor_signs {
                        let np = self.vertices[ni as usize].position;
                        let to_neighbor = [
                            np[0] - candidate_pos[0],
                            np[1] - candidate_pos[1],
                            np[2] - candidate_pos[2],
                        ];
                        let dot = to_neighbor[0] * normal[0]
                            + to_neighbor[1] * normal[1]
                            + to_neighbor[2] * normal[2];
                        if dot >= 0.0 {
                            cand_pos = true;
                        } else {
                            cand_neg = true;
                        }
                    }
                    if cand_pos && cand_neg {
                        // Would create a new saddle — reject this candidate.
                        continue;
                    }
                }

                if cost < best_cost {
                    best_cost = cost;
                    best_offset = offset;
                }
            }

            // Restore original position (will be updated in batch below).
            self.vertices[vi].position = original_pos;
            *disp = [
                best_offset * normal[0],
                best_offset * normal[1],
                best_offset * normal[2],
            ];
        }

        // Apply all displacements simultaneously (Jacobi).
        for (v, d) in self.vertices.iter_mut().zip(best_displacements.iter()) {
            if !v.anchored {
                v.position[0] += d[0];
                v.position[1] += d[1];
                v.position[2] += d[2];
            }
        }
    }

    /// Run the full smoothing pipeline: 2 iterations with halving sample
    /// distance. Within each iteration, solid vertices are processed first
    /// (ignoring leaf-only neighbors), then leaf-only vertices (seeing
    /// updated solid positions).
    pub fn smooth(&mut self) {
        for &d in &[0.1f32, 0.05] {
            // Solid pass: only process vertices with has_solid_face.
            self.smooth_iteration_filtered(d, |v| v.has_solid_face);
            // Leaf pass: only process leaf-only vertices.
            self.smooth_iteration_filtered(d, |v| v.has_leaf_face && !v.has_solid_face);
        }
    }

    /// Compute Laplacian pointiness for a vertex: the distance from the
    /// vertex to the centroid of its neighbors.
    fn laplacian_pointiness(&self, vi: u32) -> f32 {
        let v = &self.vertices[vi as usize];
        let n = v.neighbors.len();
        if n == 0 {
            return 0.0;
        }
        let mut cx = 0.0f32;
        let mut cy = 0.0f32;
        let mut cz = 0.0f32;
        for &ni in &v.neighbors {
            let nb = &self.vertices[ni as usize];
            cx += nb.position[0];
            cy += nb.position[1];
            cz += nb.position[2];
        }
        let inv_n = 1.0 / n as f32;
        cx *= inv_n;
        cy *= inv_n;
        cz *= inv_n;
        let dx = cx - v.position[0];
        let dy = cy - v.position[1];
        let dz = cz - v.position[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Build an index mapping each vertex to its incident triangle indices.
    /// Returns a Vec where `result[vi]` is the list of triangle indices
    /// (into `self.triangles`) that vertex `vi` participates in.
    fn build_vertex_triangle_index(&self) -> Vec<Vec<usize>> {
        let mut index = vec![Vec::new(); self.vertices.len()];
        for (ti, tri) in self.triangles.iter().enumerate() {
            for &vi in tri {
                index[vi as usize].push(ti);
            }
        }
        index
    }

    /// Compute the area-weighted normal for a vertex using a precomputed
    /// triangle index (fast O(valence) instead of O(triangles)).
    fn vertex_normal_fast(&self, tri_indices: &[usize]) -> [f32; 3] {
        let mut normal = [0.0f32; 3];
        for &ti in tri_indices {
            let tri = &self.triangles[ti];
            let p0 = self.vertices[tri[0] as usize].position;
            let p1 = self.vertices[tri[1] as usize].position;
            let p2 = self.vertices[tri[2] as usize].position;
            // e2×e1 for outward-pointing normals (matches compute_vertex_normals).
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let n = [
                e2[1] * e1[2] - e2[2] * e1[1],
                e2[2] * e1[0] - e2[0] * e1[2],
                e2[0] * e1[1] - e2[1] * e1[0],
            ];
            normal[0] += n[0];
            normal[1] += n[1];
            normal[2] += n[2];
        }
        normal
    }

    /// Flatten the smooth mesh into a single `SurfaceMesh` (all tags merged).
    /// Computes final vertex normals as area-weighted averages of incident
    /// triangle normals. Includes ALL triangles (no spatial filtering).
    ///
    /// Since colors are per-triangle, shared vertices at material boundaries
    /// are duplicated — once per triangle color. This means the output may
    /// have more vertices than the smooth mesh.
    pub fn to_surface_mesh(&self) -> SurfaceMesh {
        let normals = self.compute_vertex_normals();
        self.emit_surface(None, None, &normals)
    }

    /// Flatten the smooth mesh, filtered to the given bounds and split by tag.
    /// Returns one `SurfaceMesh` per tag in a `BTreeMap<SurfaceTag, SurfaceMesh>`.
    ///
    /// Vertex normals are computed from ALL triangles (including border) to
    /// ensure consistency at chunk boundaries. Then only triangles within
    /// bounds are emitted, split by their surface tag. Shared vertices at
    /// material boundaries are duplicated per-surface with matching positions
    /// and normals but per-triangle colors.
    pub fn to_split_surface_meshes_filtered(
        &self,
        min: [i32; 3],
        max: [i32; 3],
    ) -> BTreeMap<SurfaceTag, SurfaceMesh> {
        let normals = self.compute_vertex_normals();

        // Collect all unique tags.
        let mut tags: Vec<SurfaceTag> = self.triangle_tags.to_vec();
        tags.sort_unstable();
        tags.dedup();

        let mut result = BTreeMap::new();
        for tag in tags {
            let surface = self.emit_surface(Some((min, max)), Some(tag), &normals);
            if !surface.is_empty() {
                result.insert(tag, surface);
            }
        }
        result
    }

    /// Flatten the smooth mesh, filtered by bounds (optional), split by tag.
    /// Used for tests that don't need tag splitting.
    pub fn to_surface_mesh_filtered(&self, min: [i32; 3], max: [i32; 3]) -> SurfaceMesh {
        let normals = self.compute_vertex_normals();
        self.emit_surface(Some((min, max)), None, &normals)
    }

    /// Compute area-weighted vertex normals from ALL triangles. Normals are
    /// computed from the full mesh (including border) so that boundary
    /// vertices get consistent normals regardless of which chunk emits them.
    fn compute_vertex_normals(&self) -> Vec<[f32; 3]> {
        let mut normals = vec![[0.0f32; 3]; self.vertices.len()];
        for tri in &self.triangles {
            let p0 = self.vertices[tri[0] as usize].position;
            let p1 = self.vertices[tri[1] as usize].position;
            let p2 = self.vertices[tri[2] as usize].position;

            // Cross product e2×e1 (reversed from the standard e1×e2) to
            // produce outward-pointing normals that match Godot's CW winding
            // convention for our triangle order [center, a, b].
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let n = [
                e2[1] * e1[2] - e2[2] * e1[1],
                e2[2] * e1[0] - e2[0] * e1[2],
                e2[0] * e1[1] - e2[1] * e1[0],
            ];

            for &vi in tri {
                normals[vi as usize][0] += n[0];
                normals[vi as usize][1] += n[1];
                normals[vi as usize][2] += n[2];
            }
        }

        for n in &mut normals {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len > 1e-9 {
                n[0] /= len;
                n[1] /= len;
                n[2] /= len;
            }
        }

        normals
    }

    /// Emit a `SurfaceMesh` from the smooth mesh, optionally filtering by
    /// spatial bounds and/or surface tag. Colors are per-triangle: each
    /// triangle's 3 vertices get that triangle's color. Shared vertices
    /// referenced by triangles with different colors are duplicated in the
    /// output (same position and normal, different color).
    fn emit_surface(
        &self,
        bounds: Option<([i32; 3], [i32; 3])>,
        tag_filter: Option<SurfaceTag>,
        normals: &[[f32; 3]],
    ) -> SurfaceMesh {
        let mut surface = SurfaceMesh::default();

        // Because colors are per-triangle, we can't share vertices between
        // triangles of different colors. We emit 3 vertices per triangle
        // (no index sharing). This is slightly more data but avoids the
        // complexity of a multi-key dedup (position + color).
        for (ti, tri) in self.triangles.iter().enumerate() {
            // Tag filter.
            if let Some(tag) = tag_filter
                && self.triangle_tags[ti] != tag
            {
                continue;
            }

            // Bounds filter using the source voxel position. Each triangle
            // is emitted by exactly the chunk that owns its source voxel,
            // regardless of where smoothing moved the vertices.
            if let Some((min, max)) = bounds {
                let v = self.triangle_voxel_pos[ti];
                if v[0] < min[0]
                    || v[0] >= max[0]
                    || v[1] < min[1]
                    || v[1] >= max[1]
                    || v[2] < min[2]
                    || v[2] >= max[2]
                {
                    continue;
                }
            }

            let color = self.triangle_colors[ti];
            let base = surface.vertex_count() as u32;
            let use_smooth_normals = crate::mesh_gen::smooth_normals_enabled();

            // Compute flat face normal (used when smooth normals are off).
            let flat_n = if !use_smooth_normals {
                let p0 = self.vertices[tri[0] as usize].position;
                let p1 = self.vertices[tri[1] as usize].position;
                let p2 = self.vertices[tri[2] as usize].position;
                let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
                let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
                let n = [
                    e2[1] * e1[2] - e2[2] * e1[1],
                    e2[2] * e1[0] - e2[0] * e1[2],
                    e2[0] * e1[1] - e2[1] * e1[0],
                ];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if len > 1e-9 {
                    [n[0] / len, n[1] / len, n[2] / len]
                } else {
                    [0.0, 1.0, 0.0]
                }
            } else {
                [0.0, 0.0, 0.0] // unused
            };

            for &vi in tri {
                let v = &self.vertices[vi as usize];
                surface.vertices.push(v.position[0]);
                surface.vertices.push(v.position[1]);
                surface.vertices.push(v.position[2]);

                if use_smooth_normals {
                    let n = &normals[vi as usize];
                    surface.normals.push(n[0]);
                    surface.normals.push(n[1]);
                    surface.normals.push(n[2]);
                } else {
                    surface.normals.push(flat_n[0]);
                    surface.normals.push(flat_n[1]);
                    surface.normals.push(flat_n[2]);
                }

                surface.colors.push(color[0]);
                surface.colors.push(color[1]);
                surface.colors.push(color[2]);
                surface.colors.push(color[3]);
            }

            surface.indices.push(base);
            surface.indices.push(base + 1);
            surface.indices.push(base + 2);
        }

        surface
    }
}

/// Midpoint of two 3D positions.
fn mid(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        (a[0] + b[0]) * 0.5,
        (a[1] + b[1]) * 0.5,
        (a[2] + b[2]) * 0.5,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_gen::{FACE_NORMALS, FACE_VERTICES, voxel_color};
    use crate::types::VoxelType;

    /// Build a smooth mesh for a single solid voxel at the given world
    /// position, with all 6 faces visible (no neighbors).
    fn single_voxel_mesh(wx: f32, wy: f32, wz: f32, vt: VoxelType) -> SmoothMesh {
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(vt);
        let vpos = [wx as i32, wy as i32, wz as i32];
        for face_idx in 0..6 {
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[face_idx][vi][0] + wx,
                    FACE_VERTICES[face_idx][vi][1] + wy,
                    FACE_VERTICES[face_idx][vi][2] + wz,
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, TAG_BARK, vpos);
        }
        mesh.normalize_initial_normals();
        mesh
    }

    #[test]
    fn single_voxel_subdivided_vertex_count() {
        // A single solid voxel with all 6 faces visible should produce:
        // - 8 corner vertices (shared by 3 faces each)
        // - 12 edge-midpoint vertices (shared by 2 faces each)
        // - 6 center vertices (unique to each face)
        // Total: 26 unique vertices in the SmoothMesh.
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        assert_eq!(mesh.vertices.len(), 26);
    }

    #[test]
    fn single_voxel_to_surface_mesh_vertex_count() {
        // The SurfaceMesh output emits 3 vertices per triangle (per-triangle
        // colors prevent vertex sharing). 48 triangles × 3 = 144 vertices.
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        let surface = mesh.to_surface_mesh();
        assert_eq!(surface.vertex_count(), 48 * 3);
    }

    #[test]
    fn single_voxel_subdivided_triangle_count() {
        // 6 faces × 8 triangles = 48 triangles
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        assert_eq!(mesh.triangles.len(), 48);
    }

    #[test]
    fn computed_normals_point_outward() {
        // Verify that the area-weighted vertex normals computed from the
        // subdivided triangles point outward (same direction as the face
        // normal). This catches winding order bugs.
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        let normals = mesh.compute_vertex_normals();

        // Check the +Y face center at (0.5, 1.0, 0.5).
        let key = VertexKey::from_position([0.5, 1.0, 0.5]);
        let idx = *mesh.dedup.get(&key).expect("+Y center should exist");
        let n = normals[idx as usize];
        assert!(
            n[1] > 0.9,
            "+Y face center normal should point up, got {:?}",
            n
        );

        // Check the -Y face center at (0.5, 0.0, 0.5).
        let key = VertexKey::from_position([0.5, 0.0, 0.5]);
        let idx = *mesh.dedup.get(&key).expect("-Y center should exist");
        let n = normals[idx as usize];
        assert!(
            n[1] < -0.9,
            "-Y face center normal should point down, got {:?}",
            n
        );

        // Check the +X face center at (1.0, 0.5, 0.5).
        let key = VertexKey::from_position([1.0, 0.5, 0.5]);
        let idx = *mesh.dedup.get(&key).expect("+X center should exist");
        let n = normals[idx as usize];
        assert!(
            n[0] > 0.9,
            "+X face center normal should point +X, got {:?}",
            n
        );
    }

    #[test]
    fn face_center_vertices_at_correct_positions() {
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);

        // The +Y face center should be at (0.5, 1.0, 0.5).
        let key = VertexKey::from_position([0.5, 1.0, 0.5]);
        let idx = mesh.dedup.get(&key).expect("+Y face center should exist");
        let v = &mesh.vertices[*idx as usize];
        assert!((v.position[0] - 0.5).abs() < 1e-6);
        assert!((v.position[1] - 1.0).abs() < 1e-6);
        assert!((v.position[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn corner_vertex_normal_is_averaged() {
        // A corner shared by 3 perpendicular faces should have a normalized
        // average of 3 face normals.
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);

        // Corner at (1, 1, 1) is shared by +X, +Y, +Z faces.
        let key = VertexKey::from_position([1.0, 1.0, 1.0]);
        let idx = mesh.dedup.get(&key).expect("corner (1,1,1) should exist");
        let n = mesh.vertices[*idx as usize].initial_normal;
        // Average of (1,0,0), (0,1,0), (0,0,1) normalized = (1,1,1)/sqrt(3)
        let expected = 1.0 / 3.0_f32.sqrt();
        assert!((n[0] - expected).abs() < 1e-5);
        assert!((n[1] - expected).abs() < 1e-5);
        assert!((n[2] - expected).abs() < 1e-5);
    }

    #[test]
    fn edge_midpoint_normal_is_averaged() {
        // An edge midpoint shared by 2 perpendicular faces should have a
        // normalized average of 2 face normals.
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);

        // Edge midpoint at (1.0, 0.5, 0.0) is on the edge between +X and -Z.
        let key = VertexKey::from_position([1.0, 0.5, 0.0]);
        let idx = mesh.dedup.get(&key).expect("edge midpoint should exist");
        let n = mesh.vertices[*idx as usize].initial_normal;
        // Average of (1,0,0) and (0,0,-1) normalized = (1,0,-1)/sqrt(2)
        let expected = 1.0 / 2.0_f32.sqrt();
        assert!((n[0] - expected).abs() < 1e-5);
        assert!((n[1]).abs() < 1e-5);
        assert!((n[2] + expected).abs() < 1e-5);
    }

    #[test]
    fn center_vertex_has_valence_8() {
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);

        // Face center at (0.5, 1.0, 0.5) (+Y face) should have 8 neighbors.
        let key = VertexKey::from_position([0.5, 1.0, 0.5]);
        let idx = *mesh.dedup.get(&key).expect("+Y face center should exist");
        assert_eq!(mesh.vertices[idx as usize].neighbors.len(), 8);
    }

    #[test]
    fn corner_vertex_has_valence_6_when_3_faces() {
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);

        // Corner at (1,1,1) shared by 3 faces. Connected to 2 edge-midpoint
        // vertices per face = 6 neighbors.
        let key = VertexKey::from_position([1.0, 1.0, 1.0]);
        let idx = *mesh.dedup.get(&key).expect("corner should exist");
        assert_eq!(mesh.vertices[idx as usize].neighbors.len(), 6);
    }

    #[test]
    fn two_adjacent_voxels_share_vertices() {
        // Two trunk voxels side by side along X. The shared face is culled
        // (opaque↔opaque), but corner and edge-midpoint vertices on the
        // boundary should be shared between the remaining visible faces.
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);

        // Voxel at (0,0,0) — add all faces except +X (culled by neighbor).
        for face_idx in [1, 2, 3, 4, 5] {
            // Skip face 0 (+X)
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| FACE_VERTICES[face_idx][vi]);
            mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, TAG_BARK, [0, 0, 0]);
        }
        // Voxel at (1,0,0) — add all faces except -X (culled by neighbor).
        for face_idx in [0, 2, 3, 4, 5] {
            // Skip face 1 (-X)
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[face_idx][vi][0] + 1.0,
                    FACE_VERTICES[face_idx][vi][1],
                    FACE_VERTICES[face_idx][vi][2],
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, TAG_BARK, [0, 0, 0]);
        }
        mesh.normalize_initial_normals();

        // 10 visible faces × 9 vertices each = 90 before dedup.
        // Shared vertices on the x=1 boundary: 4 corners + 4 edge midpoints
        // on the shared edge = vertices that belong to faces of both voxels.
        // The 4 corners on the x=1 plane are shared between +Y/-Y/+Z/-Z of
        // voxel 0 and +Y/-Y/+Z/-Z of voxel 1.
        // Without dedup: 10 * 9 = 90. With dedup: significantly fewer.
        // Just verify sharing happened — exact count depends on the geometry.
        assert!(
            mesh.vertices.len() < 90,
            "vertices should be deduplicated, got {}",
            mesh.vertices.len()
        );

        // Triangles: 10 faces × 8 = 80 triangles.
        assert_eq!(mesh.triangles.len(), 80);
    }

    #[test]
    fn to_surface_mesh_produces_correct_counts() {
        let mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        let surface = mesh.to_surface_mesh();

        // 48 triangles × 3 vertices each (per-triangle colors prevent sharing).
        let expected_verts = 48 * 3;
        assert_eq!(surface.vertex_count(), expected_verts);
        assert_eq!(surface.indices.len(), 48 * 3);
        assert_eq!(surface.normals.len(), expected_verts * 3);
        assert_eq!(surface.colors.len(), expected_verts * 4);
        assert!(surface.uvs.is_empty());
    }

    // --- Anchoring tests ---

    #[test]
    fn face_centers_are_anchored() {
        let mut mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        mesh.apply_anchoring();

        // All 6 face centers (valence 8) should be anchored.
        let center_positions = [
            [0.5, 1.0, 0.5], // +Y
            [0.5, 0.0, 0.5], // -Y
            [1.0, 0.5, 0.5], // +X
            [0.0, 0.5, 0.5], // -X
            [0.5, 0.5, 1.0], // +Z
            [0.5, 0.5, 0.0], // -Z
        ];
        for pos in &center_positions {
            let key = VertexKey::from_position(*pos);
            let idx = *mesh.dedup.get(&key).expect("center should exist");
            assert!(
                mesh.vertices[idx as usize].anchored,
                "face center at {:?} should be anchored",
                pos
            );
        }
    }

    #[test]
    fn anchor_vertices_marks_specific_vertices() {
        let mut mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);

        // Manually anchor the +Y face vertices (simulating adjacency to
        // BuildingInterior).
        let key = VertexKey::from_position([0.5, 1.0, 0.5]);
        let center_idx = *mesh.dedup.get(&key).unwrap();
        // The center vertex has 8 neighbors = all perimeter vertices of the
        // face. Anchor them all plus the center.
        let mut to_anchor: Vec<u32> = mesh.vertices[center_idx as usize].neighbors.clone();
        to_anchor.push(center_idx);
        mesh.anchor_vertices(&to_anchor);

        for &idx in &to_anchor {
            assert!(mesh.vertices[idx as usize].anchored);
        }
    }

    #[test]
    fn insufficient_neighbor_anchoring() {
        // Build a single face (not a full voxel) — some perimeter vertices
        // will have only 1 anchored neighbor (the face center) after rule 1.
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);
        let corners = [
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
        ];
        mesh.add_subdivided_face(corners, [0.0, 0.0, -1.0], color, TAG_BARK, [0, 0, 0]);
        mesh.normalize_initial_normals();
        mesh.apply_anchoring();

        // On a single isolated face:
        // - Center (valence 8): anchored by rule 1 (is_face_center).
        // - Corners (valence 2 < 4, 0 anchored neighbors): anchored by rule 3.
        // - Edge midpoints (valence 3 < 4, but after corners get anchored they
        //   have 3 anchored neighbors ≥ 2): NOT anchored by rule 3.
        //
        // 1 center + 4 corners = 5 anchored. 4 edge midpoints remain free
        // (they're well-constrained by center + both adjacent corners).
        let anchored_count = mesh.vertices.iter().filter(|v| v.anchored).count();
        assert_eq!(
            anchored_count, 5,
            "expected 5 anchored vertices (1 center + 4 corners), got {anchored_count}"
        );
    }

    // --- Chamfer tests ---

    #[test]
    fn chamfer_moves_corner_vertices_inward() {
        let mut mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        mesh.apply_anchoring();

        // Record corner position before chamfer.
        let key = VertexKey::from_position([1.0, 1.0, 1.0]);
        let idx = *mesh.dedup.get(&key).unwrap();
        let before = mesh.vertices[idx as usize].position;

        mesh.chamfer();

        let after = mesh.vertices[idx as usize].position;

        // Corner should have moved inward (toward the voxel center).
        // The (1,1,1) corner should move in the (-,-,-) direction.
        assert!(
            after[0] < before[0],
            "x should decrease: {before:?} → {after:?}"
        );
        assert!(
            after[1] < before[1],
            "y should decrease: {before:?} → {after:?}"
        );
        assert!(
            after[2] < before[2],
            "z should decrease: {before:?} → {after:?}"
        );
    }

    #[test]
    fn chamfer_does_not_move_face_centers() {
        let mut mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        mesh.apply_anchoring();

        let key = VertexKey::from_position([0.5, 1.0, 0.5]);
        let idx = *mesh.dedup.get(&key).unwrap();
        let before = mesh.vertices[idx as usize].position;

        mesh.chamfer();

        let after = mesh.vertices[idx as usize].position;
        assert_eq!(before, after, "face center should not move");
    }

    #[test]
    fn chamfer_does_not_move_flat_surface_vertices() {
        // Build a 2×2 flat surface of voxels (a flat top). The edge-midpoint
        // vertices on the interior of the flat surface should not move.
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);
        // Only add the +Y face for 4 voxels in a 2×2 grid.
        for x in 0..2 {
            for z in 0..2 {
                let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                    [
                        FACE_VERTICES[2][vi][0] + x as f32,
                        FACE_VERTICES[2][vi][1],
                        FACE_VERTICES[2][vi][2] + z as f32,
                    ]
                });
                mesh.add_subdivided_face(corners, FACE_NORMALS[2], color, TAG_BARK, [0, 0, 0]);
            }
        }
        mesh.normalize_initial_normals();
        mesh.apply_anchoring();

        // The shared edge midpoint at (1.0, 1.0, 0.5) is in the interior
        // of the flat surface. Its normal is (0,1,0) and all its anchored
        // neighbors are coplanar, so offset·normal = 0.
        let key = VertexKey::from_position([1.0, 1.0, 0.5]);
        if let Some(&idx) = mesh.dedup.get(&key) {
            let before = mesh.vertices[idx as usize].position;
            mesh.chamfer();
            let after = mesh.vertices[idx as usize].position;
            let dist = ((after[0] - before[0]).powi(2)
                + (after[1] - before[1]).powi(2)
                + (after[2] - before[2]).powi(2))
            .sqrt();
            assert!(
                dist < 1e-6,
                "flat surface vertex should not move, moved {dist}"
            );
        }
    }

    /// Build a smooth mesh from a set of voxel positions. Generates all
    /// visible faces (face culling based on which positions are in the set),
    /// normalizes, anchors, and chamfers.
    fn build_chamfered_voxel_mesh(voxels: &[(i32, i32, i32)]) -> SmoothMesh {
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);
        let voxel_set: std::collections::HashSet<(i32, i32, i32)> =
            voxels.iter().copied().collect();

        for &(wx, wy, wz) in voxels {
            for face_idx in 0..6 {
                let (dx, dy, dz) = [
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ][face_idx];
                if voxel_set.contains(&(wx + dx, wy + dy, wz + dz)) {
                    continue;
                }
                let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                    [
                        FACE_VERTICES[face_idx][vi][0] + wx as f32,
                        FACE_VERTICES[face_idx][vi][1] + wy as f32,
                        FACE_VERTICES[face_idx][vi][2] + wz as f32,
                    ]
                });
                mesh.add_subdivided_face(
                    corners,
                    FACE_NORMALS[face_idx],
                    color,
                    TAG_BARK,
                    [wx, wy, wz],
                );
            }
        }

        mesh.normalize_initial_normals();
        mesh.apply_anchoring();
        mesh.chamfer();
        mesh
    }

    /// Check that all vertices on a given face (identified by having a
    /// specific coordinate on a specific axis equal to `face_val`) are
    /// coplanar — they all share the same value on that axis after chamfer.
    /// Returns the number of vertices checked.
    fn assert_face_vertices_coplanar(
        mesh: &SmoothMesh,
        axis: usize,
        face_val: f32,
        tolerance: f32,
        label: &str,
    ) -> usize {
        let mut checked = 0;
        for v in &mesh.vertices {
            // Find vertices originally near this face plane (pre-smoothing
            // position via the dedup key would be ideal, but we check
            // post-chamfer position with tolerance).
            if (v.position[axis] - face_val).abs() < tolerance {
                // This vertex should be exactly on the face plane.
                // Allow tiny float error but not a chamfer bump.
                assert!(
                    (v.position[axis] - face_val).abs() < 0.001,
                    "{label}: vertex at {:?} should have axis[{axis}]={face_val}, \
                     diff={}",
                    v.position,
                    (v.position[axis] - face_val).abs()
                );
                checked += 1;
            }
        }
        checked
    }

    #[test]
    fn cardinal_staircase_wide() {
        // Wide cardinal staircase: 3 steps, each 6 wide in Z, stepping
        // along X with increasing Y. All +Y top faces should remain
        // perfectly flat (vertices stay on their y-plane).
        let mut voxels = Vec::new();
        for step in 0..3 {
            let y_height = step + 1; // step 0: y=0, step 1: y=0..1, step 2: y=0..2
            for x in step..(step + 3) {
                for y in 0..y_height {
                    for z in 0..6 {
                        voxels.push((x, y, z));
                    }
                }
            }
        }
        // Deduplicate (overlapping step ranges).
        voxels.sort();
        voxels.dedup();

        let mesh = build_chamfered_voxel_mesh(&voxels);

        // Check that all +Y top face centers are still at their original Y.
        // Each step's top is at y = step_height.
        for step in 0..3_i32 {
            let top_y = (step + 1) as f32;
            let checked = assert_face_vertices_coplanar(
                &mesh,
                1,
                top_y,
                0.01,
                &format!("cardinal step {step} top y={top_y}"),
            );
            assert!(
                checked > 0,
                "should have found vertices on step {step} top face (y={top_y})"
            );
        }
    }

    #[test]
    fn chamfer_solid_positions_identical_with_and_without_leaves() {
        // Solid vertex positions after chamfer must be the same whether
        // leaf voxels are present or not.
        let solid_voxels: Vec<(i32, i32, i32)> = vec![
            (4, 0, 4),
            (4, 0, 5),
            (5, 0, 4),
            (5, 0, 5),
            (4, 1, 4),
            (4, 1, 5),
            (5, 1, 4),
            (5, 1, 5),
        ];

        // Build solid-only mesh.
        let mesh_solid = build_chamfered_voxel_mesh(&solid_voxels);

        // Build solid+leaf mesh (leaves on top).
        let mut all_voxels = solid_voxels.clone();
        // Add leaf voxels adjacent to solid.
        let leaf_positions = vec![
            (4, 2, 4),
            (4, 2, 5),
            (5, 2, 4),
            (5, 2, 5),
            (3, 2, 4),
            (6, 2, 5),
        ];

        let mut mesh_with_leaf = SmoothMesh::new();
        let voxel_set: std::collections::HashSet<(i32, i32, i32)> = solid_voxels
            .iter()
            .chain(leaf_positions.iter())
            .copied()
            .collect();

        for &(wx, wy, wz) in &solid_voxels {
            let color = voxel_color(VoxelType::Trunk);
            for face_idx in 0..6 {
                let (dx, dy, dz) = [
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ][face_idx];
                // Solid→leaf not culled, solid→solid culled.
                let neighbor = (wx + dx, wy + dy, wz + dz);
                if solid_voxels.contains(&neighbor) {
                    continue;
                }
                // Leaf neighbors don't cull solid faces.
                let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                    [
                        FACE_VERTICES[face_idx][vi][0] + wx as f32,
                        FACE_VERTICES[face_idx][vi][1] + wy as f32,
                        FACE_VERTICES[face_idx][vi][2] + wz as f32,
                    ]
                });
                mesh_with_leaf.add_subdivided_face(
                    corners,
                    FACE_NORMALS[face_idx],
                    color,
                    TAG_BARK,
                    [wx, wy, wz],
                );
            }
        }
        for &(wx, wy, wz) in &leaf_positions {
            let color = voxel_color(VoxelType::Leaf);
            for face_idx in 0..6 {
                let (dx, dy, dz) = [
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ][face_idx];
                let neighbor = (wx + dx, wy + dy, wz + dz);
                if voxel_set.contains(&neighbor) {
                    continue;
                }
                let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                    [
                        FACE_VERTICES[face_idx][vi][0] + wx as f32,
                        FACE_VERTICES[face_idx][vi][1] + wy as f32,
                        FACE_VERTICES[face_idx][vi][2] + wz as f32,
                    ]
                });
                mesh_with_leaf.add_subdivided_face(
                    corners,
                    FACE_NORMALS[face_idx],
                    color,
                    TAG_LEAF,
                    [wx, wy, wz],
                );
            }
        }
        mesh_with_leaf.normalize_initial_normals();
        mesh_with_leaf.apply_anchoring();
        mesh_with_leaf.chamfer();

        // Compare solid vertex positions between the two meshes.
        let eps = 1e-6;
        for (key, &idx_solid) in &mesh_solid.dedup {
            if let Some(&idx_mixed) = mesh_with_leaf.dedup.get(key) {
                let vs = &mesh_solid.vertices[idx_solid as usize];
                let vm = &mesh_with_leaf.vertices[idx_mixed as usize];
                if vs.has_solid_face && vm.has_solid_face {
                    let dist = ((vs.position[0] - vm.position[0]).powi(2)
                        + (vs.position[1] - vm.position[1]).powi(2)
                        + (vs.position[2] - vm.position[2]).powi(2))
                    .sqrt();
                    assert!(
                        dist < eps,
                        "solid vertex at {:?} differs: solid-only={:?} mixed={:?} dist={dist}",
                        vs.position,
                        vs.position,
                        vm.position
                    );
                }
            }
        }
    }

    // --- Smoothing tests ---

    #[test]
    fn smoothing_reduces_pointiness() {
        let mut mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        mesh.apply_anchoring();
        mesh.chamfer();

        // Compute total pointiness before smoothing.
        let pointiness_before: f32 = (0..mesh.vertices.len())
            .map(|i| {
                let k = mesh.laplacian_pointiness(i as u32);
                k * k
            })
            .sum();

        mesh.smooth();

        let pointiness_after: f32 = (0..mesh.vertices.len())
            .map(|i| {
                let k = mesh.laplacian_pointiness(i as u32);
                k * k
            })
            .sum();

        assert!(
            pointiness_after < pointiness_before,
            "smoothing should reduce total pointiness: {pointiness_before} → {pointiness_after}"
        );
    }

    #[test]
    fn octagonal_prism_convergence() {
        // Create a long rod of solid voxels along the Z axis. After chamfer
        // and smoothing, the cross-section should approximate an octagon:
        // the 4 edge vertices should move inward, reducing corner curvature.
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);

        // 8-voxel rod along Z at x=0, y=0.
        for z in 0..8 {
            for face_idx in 0..6 {
                let (dx, dy, dz) = [
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ][face_idx];
                // Cull internal faces along Z.
                if dz == 1 && z < 7 {
                    continue;
                }
                if dz == -1 && z > 0 {
                    continue;
                }
                let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                    [
                        FACE_VERTICES[face_idx][vi][0],
                        FACE_VERTICES[face_idx][vi][1],
                        FACE_VERTICES[face_idx][vi][2] + z as f32,
                    ]
                });
                mesh.add_subdivided_face(
                    corners,
                    FACE_NORMALS[face_idx],
                    color,
                    TAG_BARK,
                    [0, 0, 0],
                );
            }
        }
        mesh.normalize_initial_normals();
        mesh.apply_anchoring();
        mesh.chamfer();
        mesh.smooth();

        // Check cross-section at z=4 (middle of the rod). The corner vertex
        // at (1, 1, 4) should have moved inward from its original position.
        let key = VertexKey::from_position([1.0, 1.0, 4.0]);
        let idx = *mesh
            .dedup
            .get(&key)
            .expect("corner at (1,1,4) should exist");
        let v = &mesh.vertices[idx as usize];

        assert!(
            !v.anchored,
            "corner at (1,1,4) should not be anchored, has {} neighbors, {} anchored",
            v.neighbors.len(),
            v.neighbors
                .iter()
                .filter(|&&n| mesh.vertices[n as usize].anchored)
                .count(),
        );

        let pos = v.position;
        // The corner should have moved inward (both x and y decreased from 1.0).
        assert!(pos[0] < 1.0, "corner x should be < 1.0, got {}", pos[0]);
        assert!(pos[1] < 1.0, "corner y should be < 1.0, got {}", pos[1]);
        // Z should stay approximately at 4.0 (no Z displacement for a
        // mid-rod cross-section).
        assert!(
            (pos[2] - 4.0).abs() < 0.1,
            "corner z should be ~4.0, got {}",
            pos[2]
        );
    }

    #[test]
    fn chunk_boundary_alignment() {
        // Generate hilly terrain spanning two chunks. Independently smooth
        // each chunk and verify that shared boundary vertices match.
        use crate::mesh_gen::ChunkCoord;
        use crate::world::VoxelWorld;

        let mut world = VoxelWorld::new(32, 16, 16);
        // Create hilly terrain: varying heights across the chunk boundary.
        for x in 0..32 {
            let height = 3 + (x % 3); // Heights: 3, 4, 5, 3, 4, 5, ...
            for y in 0..height {
                for z in 0..16 {
                    world.set(crate::types::VoxelCoord::new(x, y, z), VoxelType::Dirt);
                }
            }
        }

        // Build smooth meshes for chunk 0 (x=0..15) and chunk 1 (x=16..31)
        // using the same border radius.
        let border = 2;
        let mesh0 = build_smooth_chunk(&world, ChunkCoord::new(0, 0, 0), border);
        let mesh1 = build_smooth_chunk(&world, ChunkCoord::new(1, 0, 0), border);

        // Find vertices on the shared boundary (x=16 in world coords).
        // Both meshes should produce identical positions for these vertices.
        let boundary_x = 16.0;
        let epsilon = 1e-5;

        let mut matched = 0;
        for (key0, &idx0) in &mesh0.dedup {
            let pos0 = mesh0.vertices[idx0 as usize].position;
            if (pos0[0] - boundary_x).abs() > 0.5 {
                continue; // Not near boundary.
            }
            if let Some(&idx1) = mesh1.dedup.get(key0) {
                let pos1 = mesh1.vertices[idx1 as usize].position;
                let dist = ((pos0[0] - pos1[0]).powi(2)
                    + (pos0[1] - pos1[1]).powi(2)
                    + (pos0[2] - pos1[2]).powi(2))
                .sqrt();
                assert!(
                    dist < epsilon,
                    "boundary vertex at {:?} differs: {:?} vs {:?} (dist={dist})",
                    pos0,
                    pos0,
                    pos1,
                );
                matched += 1;
            }
        }
        assert!(matched > 0, "should have found shared boundary vertices");
    }

    #[test]
    fn chunk_boundary_iteration_limit() {
        // Determine how many smoothing iterations are safe with a 2-voxel
        // border by increasing iteration count until boundary vertices diverge.
        // This empirically validates the information-causality analysis.
        use crate::mesh_gen::ChunkCoord;
        use crate::world::VoxelWorld;

        let mut world = VoxelWorld::new(32, 16, 16);
        for x in 0..32 {
            let height = 3 + (x % 3);
            for y in 0..height {
                for z in 0..16 {
                    world.set(crate::types::VoxelCoord::new(x, y, z), VoxelType::Dirt);
                }
            }
        }

        let border = 2;
        let epsilon = 1e-5;
        let mut last_safe_iterations = 0;

        for num_iterations in 1..=10 {
            let mesh0 =
                build_smooth_chunk_n(&world, ChunkCoord::new(0, 0, 0), border, num_iterations);
            let mesh1 =
                build_smooth_chunk_n(&world, ChunkCoord::new(1, 0, 0), border, num_iterations);

            let boundary_x = 16.0;
            let mut all_match = true;

            for (key0, &idx0) in &mesh0.dedup {
                let pos0 = mesh0.vertices[idx0 as usize].position;
                if (pos0[0] - boundary_x).abs() > 0.5 {
                    continue;
                }
                if let Some(&idx1) = mesh1.dedup.get(key0) {
                    let pos1 = mesh1.vertices[idx1 as usize].position;
                    let dist = ((pos0[0] - pos1[0]).powi(2)
                        + (pos0[1] - pos1[1]).powi(2)
                        + (pos0[2] - pos1[2]).powi(2))
                    .sqrt();
                    if dist >= epsilon {
                        all_match = false;
                        break;
                    }
                }
            }

            if all_match {
                last_safe_iterations = num_iterations;
            } else {
                break;
            }
        }

        // With a 2-voxel border, we expect at least 3 iterations to be safe
        // (chamfer + 3 smoothing = 4 hops, fitting in ~4 vertex hops from
        // 2 voxels). If this assertion fails, reduce the default iteration
        // count or increase the border.
        assert!(
            last_safe_iterations >= 3,
            "expected at least 3 safe iterations with 2-voxel border, got {last_safe_iterations}"
        );
    }

    // --- Filtering tests ---

    #[test]
    fn to_surface_mesh_filtered_excludes_border_triangles() {
        // Build a mesh with voxels at (8,8,8) and (17,8,8). The first is
        // inside a 16³ chunk, the second is outside. Filter to chunk bounds
        // [0,16) and verify only the first voxel's geometry is included.
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);

        // Voxel at (8,8,8): all 6 faces.
        for face_idx in 0..6 {
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[face_idx][vi][0] + 8.0,
                    FACE_VERTICES[face_idx][vi][1] + 8.0,
                    FACE_VERTICES[face_idx][vi][2] + 8.0,
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, TAG_BARK, [8, 8, 8]);
        }
        // Voxel at (17,8,8): all 6 faces (outside chunk 0).
        for face_idx in 0..6 {
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[face_idx][vi][0] + 17.0,
                    FACE_VERTICES[face_idx][vi][1] + 8.0,
                    FACE_VERTICES[face_idx][vi][2] + 8.0,
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, TAG_BARK, [17, 8, 8]);
        }
        mesh.normalize_initial_normals();

        // Unfiltered: 12 faces × 8 = 96 triangles.
        let unfiltered = mesh.to_surface_mesh();
        assert_eq!(unfiltered.indices.len(), 96 * 3);

        // Filtered to chunk [0,16): only voxel at (8,8,8) included.
        let filtered = mesh.to_surface_mesh_filtered([0, 0, 0], [16, 16, 16]);
        assert_eq!(
            filtered.indices.len(),
            48 * 3,
            "filtered should have 48 triangles (6 faces of voxel at (8,8,8))"
        );
        // Fewer vertices in filtered output.
        assert!(filtered.vertex_count() < unfiltered.vertex_count());
    }

    #[test]
    fn filtered_normals_use_all_triangles() {
        // Verify that vertex normals at the filter boundary are computed
        // from ALL triangles (including border), not just the filtered ones.
        // This prevents lighting seams between adjacent chunks.
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);

        // Two adjacent voxels at (8,8,8) and (8,8,17) with shared face culled.
        // Voxel 0 is inside chunk [0,16), voxel 1 is outside.
        // Shared face on z=9 boundary is culled.
        for face_idx in [0, 1, 2, 3, 4] {
            // Skip face 5 (-Z) — not relevant. Skip +Z (face 4) if we want
            // to cull. Actually, let's not cull — just add all faces for both
            // voxels and test that the normals match.
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[face_idx][vi][0] + 8.0,
                    FACE_VERTICES[face_idx][vi][1] + 8.0,
                    FACE_VERTICES[face_idx][vi][2] + 8.0,
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, TAG_BARK, [8, 8, 8]);
        }
        // Add -Z face for voxel 0.
        {
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[5][vi][0] + 8.0,
                    FACE_VERTICES[5][vi][1] + 8.0,
                    FACE_VERTICES[5][vi][2] + 8.0,
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[5], color, TAG_BARK, [8, 8, 8]);
        }
        // Voxel 1 at (8,8,17): all 6 faces.
        for face_idx in 0..6 {
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[face_idx][vi][0] + 8.0,
                    FACE_VERTICES[face_idx][vi][1] + 8.0,
                    FACE_VERTICES[face_idx][vi][2] + 17.0,
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, TAG_BARK, [8, 8, 17]);
        }
        mesh.normalize_initial_normals();

        // Unfiltered normals for reference.
        let unfiltered = mesh.to_surface_mesh();

        // Filter to chunk [0,16) — voxel 0 included, voxel 1 excluded.
        let filtered = mesh.to_surface_mesh_filtered([0, 0, 0], [16, 16, 16]);

        // The +Y face center of voxel 0 at (8.5, 9.0, 8.5) should exist in
        // both outputs and have the same normal.
        let target = [8.5f32, 9.0, 8.5];
        let find_normal = |surface: &SurfaceMesh| -> Option<[f32; 3]> {
            for i in 0..surface.vertex_count() {
                let px = surface.vertices[i * 3];
                let py = surface.vertices[i * 3 + 1];
                let pz = surface.vertices[i * 3 + 2];
                if (px - target[0]).abs() < 1e-5
                    && (py - target[1]).abs() < 1e-5
                    && (pz - target[2]).abs() < 1e-5
                {
                    return Some([
                        surface.normals[i * 3],
                        surface.normals[i * 3 + 1],
                        surface.normals[i * 3 + 2],
                    ]);
                }
            }
            None
        };

        let n_unf = find_normal(&unfiltered).expect("vertex should be in unfiltered");
        let n_fil = find_normal(&filtered).expect("vertex should be in filtered");
        let diff = ((n_unf[0] - n_fil[0]).powi(2)
            + (n_unf[1] - n_fil[1]).powi(2)
            + (n_unf[2] - n_fil[2]).powi(2))
        .sqrt();
        assert!(
            diff < 1e-6,
            "filtered normal should match unfiltered: {:?} vs {:?}",
            n_fil,
            n_unf
        );
    }

    #[test]
    fn smoothing_does_not_move_anchored_vertices() {
        let mut mesh = single_voxel_mesh(0.0, 0.0, 0.0, VoxelType::Trunk);
        mesh.apply_anchoring();
        mesh.chamfer();

        // Record all anchored vertex positions.
        let anchored_positions: Vec<(usize, [f32; 3])> = mesh
            .vertices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.anchored)
            .map(|(i, v)| (i, v.position))
            .collect();

        mesh.smooth();

        // Verify none moved.
        for (i, before) in &anchored_positions {
            let after = mesh.vertices[*i].position;
            assert_eq!(
                &after, before,
                "anchored vertex {i} should not move during smoothing"
            );
        }
    }

    #[test]
    fn building_interior_anchors_adjacent_face() {
        // Integration test: a solid voxel next to a BuildingInterior should
        // have its adjacent face fully anchored.
        use crate::mesh_gen::ChunkCoord;
        use crate::world::VoxelWorld;

        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(crate::types::VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(
            crate::types::VoxelCoord::new(9, 8, 8),
            VoxelType::BuildingInterior,
        );

        let mesh = build_smooth_chunk(&world, ChunkCoord::new(0, 0, 0), 2);

        // The +X face of the trunk voxel faces BuildingInterior. All 9
        // vertices of that face should be anchored. The face center is at
        // (9.0, 8.5, 8.5).
        let center_key = VertexKey::from_position([9.0, 8.5, 8.5]);
        let center_idx = *mesh
            .dedup
            .get(&center_key)
            .expect("+X face center should exist");
        assert!(
            mesh.vertices[center_idx as usize].anchored,
            "+X face center should be anchored (adjacent to BuildingInterior)"
        );

        // Check a corner of that face too: (9, 8, 8).
        let corner_key = VertexKey::from_position([9.0, 8.0, 8.0]);
        let corner_idx = *mesh
            .dedup
            .get(&corner_key)
            .expect("+X face corner should exist");
        assert!(
            mesh.vertices[corner_idx as usize].anchored,
            "+X face corner should be anchored (adjacent to BuildingInterior)"
        );
    }

    /// Build a smooth mesh for a single chunk with the given border radius.
    /// Uses the default 3 smoothing iterations.
    fn build_smooth_chunk(
        world: &crate::world::VoxelWorld,
        chunk: crate::mesh_gen::ChunkCoord,
        border: i32,
    ) -> SmoothMesh {
        build_smooth_chunk_n(world, chunk, border, 3)
    }

    /// Build a smooth mesh for a single chunk with configurable iteration
    /// count. Used by `chunk_boundary_iteration_limit` to test how many
    /// iterations are safe for a given border radius.
    fn build_smooth_chunk_n(
        world: &crate::world::VoxelWorld,
        chunk: crate::mesh_gen::ChunkCoord,
        border: i32,
        iterations: usize,
    ) -> SmoothMesh {
        use crate::mesh_gen::CHUNK_SIZE;
        use crate::types::VoxelCoord;

        let base_x = chunk.cx * CHUNK_SIZE;
        let base_y = chunk.cy * CHUNK_SIZE;
        let base_z = chunk.cz * CHUNK_SIZE;

        // Iterate voxels in the chunk + border range, but only emit geometry
        // for voxels within the chunk's own 16³ region. Border voxels provide
        // context for face culling and anchoring.
        let mut mesh = SmoothMesh::new();

        let min_x = base_x - border;
        let max_x = base_x + CHUNK_SIZE + border;
        let min_y = base_y - border;
        let max_y = base_y + CHUNK_SIZE + border;
        let min_z = base_z - border;
        let max_z = base_z + CHUNK_SIZE + border;

        let faces = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];

        for wy in min_y..max_y {
            for wz in min_z..max_z {
                for wx in min_x..max_x {
                    let vt = world.get(VoxelCoord::new(wx, wy, wz));
                    if !vt.is_opaque() {
                        continue;
                    }

                    let color = crate::mesh_gen::voxel_color(vt);

                    for (face_idx, &(dx, dy, dz)) in faces.iter().enumerate() {
                        let neighbor = world.get(VoxelCoord::new(wx + dx, wy + dy, wz + dz));
                        if neighbor.is_opaque() {
                            continue;
                        }

                        let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                            [
                                FACE_VERTICES[face_idx][vi][0] + wx as f32,
                                FACE_VERTICES[face_idx][vi][1] + wy as f32,
                                FACE_VERTICES[face_idx][vi][2] + wz as f32,
                            ]
                        });
                        let normal = FACE_NORMALS[face_idx];

                        let vert_indices = mesh.add_subdivided_face(
                            corners,
                            normal,
                            color,
                            TAG_BARK,
                            [wx, wy, wz],
                        );

                        // Anchoring rule 2: if neighbor is a non-solid
                        // constructed voxel, anchor all face vertices.
                        if matches!(
                            neighbor,
                            VoxelType::BuildingInterior
                                | VoxelType::WoodLadder
                                | VoxelType::RopeLadder
                        ) {
                            mesh.anchor_vertices(&vert_indices);
                        }
                    }
                }
            }
        }

        mesh.normalize_initial_normals();
        mesh.apply_anchoring();
        mesh.chamfer();

        let sample_distances = [
            0.1,
            0.05,
            0.025,
            0.0125,
            0.00625,
            0.003125,
            0.0015625,
            0.00078125,
            0.000390625,
            0.0001953125,
        ];
        for i in 0..iterations.min(sample_distances.len()) {
            mesh.smooth_iteration_filtered(sample_distances[i], |_| true);
        }

        mesh
    }
}
