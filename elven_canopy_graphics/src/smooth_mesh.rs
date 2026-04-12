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
// 2. Non-manifold resolution: split voxel-edge midpoint vertices and voxel
//    corner vertices that are non-manifold (shared by faces from
//    diagonally-adjacent voxels). See `resolve_non_manifold`.
// 3. Anchoring: mark vertices that must not move (face centers, vertices
//    adjacent to non-solid voxels like `BuildingInterior`, vertices with
//    insufficient anchored neighbors).
// 4. Chamfer: non-anchored vertices with ≥2 anchored neighbors move toward
//    the centroid of their anchored neighbors. Solid vertices first, then
//    leaf-only vertices (so solid geometry is independent of leaves).
// 5. Curvature-minimizing smoothing: iterative Jacobi-style passes that
//    minimize total squared Laplacian pointiness in each vertex's 1-ring
//    neighborhood.
// 6. Vertex normals: area-weighted average of incident triangle normals.
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
// See `mesh_decimation.rs` for an optional post-pipeline decimation pass
// (QEM edge-collapse, coplanar retri, collinear collapse).

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::mesh_gen::SurfaceMesh;

/// Deduplicate a `Vec<u32>` in-place, preserving the order of first
/// occurrences. Uses a simple quadratic scan suitable for small lists
/// (vertex neighbor lists are typically 6-12 elements).
pub(crate) fn dedup_preserve_order(vec: &mut Vec<u32>) {
    let mut write = 0;
    for read in 0..vec.len() {
        let val = vec[read];
        // Check if val already exists in the kept prefix [0..write).
        if !vec[..write].contains(&val) {
            vec[write] = val;
            write += 1;
        }
    }
    vec.truncate(write);
}

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
    /// Wind sway weight for leaf vertices (0.0 = anchored at wood, 1.0 = full
    /// sway). Computed per-vertex as the Euclidean distance from the vertex
    /// position to the nearest opaque voxel surface, normalized by a max
    /// distance cap. Only meaningful for vertices with `has_leaf_face`.
    /// Defaults to 1.0 (full sway).
    pub sway_weight: f32,
}

/// A key for vertex deduplication. Uses doubled integer coordinates to avoid
/// float keys: a corner at (1, 2, 3) becomes (2, 4, 6), an edge midpoint at
/// (1.5, 2, 3) becomes (3, 4, 6). This gives exact comparison for all
/// positions on the half-integer grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VertexKey {
    /// 2x the x coordinate.
    x2: i32,
    /// 2x the y coordinate.
    y2: i32,
    /// 2x the z coordinate.
    z2: i32,
}

impl VertexKey {
    pub fn from_position(pos: [f32; 3]) -> Self {
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
    /// Per-triangle face normal — the axis-aligned normal of the voxel face
    /// that generated each triangle. Used to recompute `initial_normal` for
    /// split vertices during non-manifold resolution, and to determine which
    /// "side" of a non-manifold voxel edge each triangle belongs to.
    pub triangle_face_normals: Vec<[f32; 3]>,
    /// Per-triangle face midpoint — the center position of the voxel face
    /// that generated each triangle. Together with `triangle_face_normals`,
    /// used to pair voxel faces into "sides" during non-manifold resolution.
    pub triangle_face_midpoints: Vec<[f32; 3]>,
    /// Deduplication map: grid position → vertex index.
    pub dedup: FxHashMap<VertexKey, u32>,
    /// Pipeline configuration (smoothing, decimation, normals). Stored on the
    /// mesh so that pipeline stages and `SmoothMesh` methods can read it without
    /// global state.
    pub config: crate::mesh_gen::MeshPipelineConfig,
}

impl Default for SmoothMesh {
    fn default() -> Self {
        Self::new()
    }
}

impl SmoothMesh {
    /// Create an empty smooth mesh with default pipeline config.
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
            triangle_tags: Vec::new(),
            triangle_colors: Vec::new(),
            triangle_voxel_pos: Vec::new(),
            triangle_face_normals: Vec::new(),
            triangle_face_midpoints: Vec::new(),
            dedup: FxHashMap::default(),
            config: crate::mesh_gen::MeshPipelineConfig::default(),
        }
    }

    /// Create a smooth mesh pre-sized for the given estimated face count.
    ///
    /// Each subdivided face produces 8 triangles and up to 9 vertices (with
    /// dedup reducing to ~5 unique vertices per face on average). Pre-sizing
    /// avoids repeated reallocations and FxHashMap rehashes during face
    /// construction.
    pub fn with_estimated_faces(
        face_estimate: usize,
        config: crate::mesh_gen::MeshPipelineConfig,
    ) -> Self {
        let tri_cap = face_estimate * 8;
        // ~5 unique vertices per face after dedup (corners and midpoints
        // are shared between adjacent faces).
        let vert_cap = face_estimate * 5;
        Self {
            vertices: Vec::with_capacity(vert_cap),
            triangles: Vec::with_capacity(tri_cap),
            triangle_tags: Vec::with_capacity(tri_cap),
            triangle_colors: Vec::with_capacity(tri_cap),
            triangle_voxel_pos: Vec::with_capacity(tri_cap),
            triangle_face_normals: Vec::with_capacity(tri_cap),
            triangle_face_midpoints: Vec::with_capacity(tri_cap),
            dedup: FxHashMap::with_capacity_and_hasher(vert_cap, Default::default()),
            config,
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
                sway_weight: 1.0,
            });
            self.dedup.insert(key, idx);
            idx
        }
    }

    /// Add an edge between two vertices (bidirectional). Pushes
    /// unconditionally — duplicates are removed later by
    /// `deduplicate_neighbors`.
    fn add_edge(&mut self, a: u32, b: u32) {
        self.vertices[a as usize].neighbors.push(b);
        self.vertices[b as usize].neighbors.push(a);
    }

    /// Deduplicate every vertex's neighbor list, preserving insertion order
    /// (first occurrence wins). Must be called after bulk edge insertion
    /// (face construction or `rebuild_neighbors`) before any code that reads
    /// neighbor lists. Preserving insertion order is critical: sorted order
    /// changes float accumulation order in chamfer/smoothing, which changes
    /// decimation collapse sequences and thus mesh output.
    ///
    /// Public so test helpers can finalize neighbor lists without running
    /// the full pipeline.
    pub fn deduplicate_neighbors(&mut self) {
        for v in &mut self.vertices {
            dedup_preserve_order(&mut v.neighbors);
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
            self.triangle_face_normals.push(face_normal);
            self.triangle_face_midpoints.push(center);

            // Add edges: center↔a, center↔b, a↔b
            self.add_edge(ct, a);
            self.add_edge(ct, b);
            self.add_edge(a, b);
        }

        [c0, c1, c2, c3, m01, m12, m23, m30, ct]
    }

    /// Resolve non-manifold topology by splitting vertices.
    ///
    /// Must be called after face subdivision + dedup and before
    /// `normalize_initial_normals` / `apply_anchoring` / `chamfer`.
    ///
    /// Solid (wood) and leaf surfaces are treated independently, matching
    /// the pipeline's existing separation (solid chamfer ignores leaves,
    /// leaf chamfer ignores solid). A solid midpoint shared by 2 solid
    /// faces + 1 leaf face is manifold from wood's perspective; similarly
    /// for leaf midpoints. Only same-surface-type faces count toward the
    /// non-manifold threshold.
    ///
    /// For each surface type, runs two passes:
    /// - **Pass 1 — Split non-manifold voxel edge midpoints:** Finds voxel
    ///   edges shared by more than 2 same-type voxel faces. Splits the
    ///   midpoint using side-pairing (`face_midpoint + face_normal * 0.5`).
    /// - **Pass 2 — Split non-manifold voxel vertices:** Finds vertices
    ///   where same-type incident triangles don't form a single connected
    ///   fan. Splits into one vertex per fan component.
    pub fn resolve_non_manifold(&mut self) {
        // Finalize neighbor lists after bulk edge insertion during face
        // construction (add_edge pushes unconditionally for speed).
        self.deduplicate_neighbors();

        let mut any_split = false;
        // Resolve solid surfaces (non-leaf).
        any_split |= self.resolve_non_manifold_pass1(false);
        any_split |= self.resolve_non_manifold_pass2(false);
        // Resolve leaf surfaces.
        any_split |= self.resolve_non_manifold_pass1(true);
        any_split |= self.resolve_non_manifold_pass2(true);
        if any_split {
            self.rebuild_neighbors();
            self.recompute_split_vertex_metadata();
        }
    }

    /// Pass 1: Find and split non-manifold voxel edge midpoints.
    ///
    /// Only considers triangles of the specified surface type (`leaf_only`:
    /// true = leaf triangles only, false = solid/non-leaf triangles only).
    /// A voxel-edge midpoint vertex is identified by its `VertexKey` having
    /// exactly one odd coordinate. If such a vertex has incident same-type
    /// triangles from more than 2 distinct voxel faces, the corresponding
    /// voxel edge is non-manifold for that surface type.
    ///
    /// Returns true if any splits were performed.
    fn resolve_non_manifold_pass1(&mut self, leaf_only: bool) -> bool {
        let vtx_tris = self.build_vertex_triangle_index();
        let mut any_split = false;

        // Collect midpoints to split before mutating (indices shift as we add
        // vertices, so collect first, then process with updated indices).
        let mut midpoints_to_split: Vec<u32> = Vec::new();

        for (vi, tri_indices) in vtx_tris.iter().enumerate() {
            // Check if this is a voxel-edge midpoint: exactly one odd
            // coordinate in the VertexKey (doubled integer coords).
            let key = VertexKey::from_position(self.vertices[vi].position);
            let odd_count = [key.x2 % 2 != 0, key.y2 % 2 != 0, key.z2 % 2 != 0]
                .iter()
                .filter(|&&b| b)
                .count();
            if odd_count != 1 {
                continue;
            }

            // Filter to only same-type triangles.
            let filtered: Vec<usize> = tri_indices
                .iter()
                .copied()
                .filter(|&ti| (self.triangle_tags[ti] == TAG_LEAF) == leaf_only)
                .collect();

            // Count distinct voxel faces among filtered triangles. A voxel
            // face is identified by its (face_normal, face_midpoint) pair.
            let mut face_keys: Vec<([i32; 3], [i32; 3])> = Vec::new();
            for &ti in &filtered {
                let n = self.triangle_face_normals[ti];
                let m = self.triangle_face_midpoints[ti];
                // Quantize to avoid float comparison issues (normals are
                // axis-aligned ±1, midpoints are on half-integer grid).
                let nk = [
                    (n[0] * 2.0).round() as i32,
                    (n[1] * 2.0).round() as i32,
                    (n[2] * 2.0).round() as i32,
                ];
                let mk = [
                    (m[0] * 2.0).round() as i32,
                    (m[1] * 2.0).round() as i32,
                    (m[2] * 2.0).round() as i32,
                ];
                if !face_keys.contains(&(nk, mk)) {
                    face_keys.push((nk, mk));
                }
            }

            if face_keys.len() <= 2 {
                continue; // Manifold for this surface type.
            }

            midpoints_to_split.push(vi as u32);
        }

        // Process each non-manifold midpoint, splitting only same-type
        // triangles.
        for &orig_vi in &midpoints_to_split {
            any_split |= self.split_midpoint_vertex(orig_vi, leaf_only);
        }

        any_split
    }

    /// Split a single non-manifold voxel-edge midpoint vertex. Only
    /// considers triangles of the specified surface type. The same-type
    /// voxel faces at the midpoint are paired into sides using
    /// `face_midpoint + face_normal * 0.5`. Triangles of the other
    /// surface type are left on the original vertex.
    fn split_midpoint_vertex(&mut self, vi: u32, leaf_only: bool) -> bool {
        // Collect only same-type incident triangles (must re-scan since
        // earlier splits may have changed indices).
        let incident: Vec<usize> = self
            .triangles
            .iter()
            .enumerate()
            .filter(|(ti, tri)| {
                tri.contains(&vi) && (self.triangle_tags[*ti] == TAG_LEAF) == leaf_only
            })
            .map(|(ti, _)| ti)
            .collect();

        if incident.is_empty() {
            return false;
        }

        // Collect distinct voxel faces and compute side keys.
        // side_key = face_midpoint + face_normal * 0.5, quantized.
        struct FaceInfo {
            normal_key: [i32; 3],
            midpoint_key: [i32; 3],
            side_key: [i32; 3],
        }

        let mut face_infos: Vec<FaceInfo> = Vec::new();
        let mut tri_face_idx: Vec<usize> = Vec::new(); // which face each incident tri belongs to

        for &ti in &incident {
            let n = self.triangle_face_normals[ti];
            let m = self.triangle_face_midpoints[ti];
            let nk = [
                (n[0] * 2.0).round() as i32,
                (n[1] * 2.0).round() as i32,
                (n[2] * 2.0).round() as i32,
            ];
            let mk = [
                (m[0] * 2.0).round() as i32,
                (m[1] * 2.0).round() as i32,
                (m[2] * 2.0).round() as i32,
            ];
            // side_key = (face_midpoint + face_normal * 0.5) in doubled
            // integer coords. Two faces on the same "side" of the diagonal
            // gap will have matching side_keys.
            let sk = [
                ((m[0] + n[0] * 0.5) * 2.0).round() as i32,
                ((m[1] + n[1] * 0.5) * 2.0).round() as i32,
                ((m[2] + n[2] * 0.5) * 2.0).round() as i32,
            ];

            let face_idx = face_infos
                .iter()
                .position(|f| f.normal_key == nk && f.midpoint_key == mk);
            if let Some(idx) = face_idx {
                tri_face_idx.push(idx);
            } else {
                tri_face_idx.push(face_infos.len());
                face_infos.push(FaceInfo {
                    normal_key: nk,
                    midpoint_key: mk,
                    side_key: sk,
                });
            }
        }

        if face_infos.len() <= 2 {
            return false; // Not non-manifold (or already resolved).
        }

        // Group faces into 2 sides by matching side_keys. Typically 4
        // voxel faces (2 diagonally-adjacent voxels × 2 faces each), but
        // asymmetric face culling (e.g., solid→leaf faces are kept while
        // leaf→solid faces are culled) can produce 3 faces.
        let mut side_assignment: Vec<u8> = vec![0; face_infos.len()];
        let side_a_key = face_infos[0].side_key;

        for (i, face) in face_infos.iter().enumerate() {
            if face.side_key == side_a_key {
                side_assignment[i] = 0;
            } else {
                side_assignment[i] = 1;
            }
        }

        // Determine which side each incident triangle belongs to.
        let mut tri_sides: Vec<u8> = Vec::new();
        for &fi in &tri_face_idx {
            tri_sides.push(side_assignment[fi]);
        }

        // Side 0 keeps the original vertex. Side 1 gets a duplicate.
        let new_vi = self.vertices.len() as u32;
        let orig_vertex = self.vertices[vi as usize].clone();
        self.vertices.push(orig_vertex);

        // Rewrite side 1 triangles to use the new vertex index.
        for (local_idx, &ti) in incident.iter().enumerate() {
            if tri_sides[local_idx] == 1 {
                let tri = &mut self.triangles[ti];
                for v in tri.iter_mut() {
                    if *v == vi {
                        *v = new_vi;
                    }
                }
            }
        }

        // Remove the original midpoint from dedup (now ambiguous).
        let key = VertexKey::from_position(self.vertices[vi as usize].position);
        self.dedup.remove(&key);

        true
    }

    /// Pass 2: Find and split non-manifold voxel vertices.
    ///
    /// Only considers triangles of the specified surface type. After pass 1,
    /// some voxel corner vertices may still be non-manifold for a given
    /// surface type — their same-type incident triangles don't form a single
    /// connected fan. This catches: (a) voxels sharing only a corner point,
    /// (b) corner vertices left non-manifold after a voxel-edge midpoint
    /// split (e.g., the bottom of a split voxel edge sitting on flat ground).
    ///
    /// Returns true if any splits were performed.
    fn resolve_non_manifold_pass2(&mut self, leaf_only: bool) -> bool {
        let vtx_tris = self.build_vertex_triangle_index();
        let mut any_split = false;

        // Collect vertices to split before mutating.
        let mut verts_to_split: Vec<u32> = Vec::new();

        for (vi, tri_indices) in vtx_tris.iter().enumerate() {
            // Filter to only same-type triangles.
            let filtered: Vec<usize> = tri_indices
                .iter()
                .copied()
                .filter(|&ti| (self.triangle_tags[ti] == TAG_LEAF) == leaf_only)
                .collect();
            if filtered.len() < 2 {
                continue;
            }
            if self.fan_components(vi as u32, &filtered).is_some() {
                verts_to_split.push(vi as u32);
            }
        }

        for &vi in &verts_to_split {
            any_split |= self.split_non_manifold_vertex(vi, leaf_only);
        }

        any_split
    }

    /// Partition incident triangles into connected fan components around a
    /// vertex. Two triangles are fan-adjacent if they share a triangle-mesh
    /// edge that includes the vertex (i.e., they share another vertex
    /// besides `vi`). Returns a per-triangle component assignment (parallel
    /// to `tri_indices`) where each value is a component index starting at 0.
    /// Returns `None` if there is only one component (the common manifold
    /// case), to avoid allocating for the majority of vertices.
    fn fan_components(&self, vi: u32, tri_indices: &[usize]) -> Option<Vec<usize>> {
        let n = tri_indices.len();
        if n <= 1 {
            return None;
        }

        // For each triangle, collect the other two vertex indices (not vi).
        let other_verts: Vec<[u32; 2]> = tri_indices
            .iter()
            .map(|&ti| {
                let tri = &self.triangles[ti];
                let others: Vec<u32> = tri.iter().copied().filter(|&v| v != vi).collect();
                [others[0], others[1]]
            })
            .collect();

        // Union-find to group fan-adjacent triangles.
        let mut parent: Vec<usize> = (0..n).collect();

        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }

        for i in 0..n {
            for j in (i + 1)..n {
                if other_verts[i].iter().any(|v| other_verts[j].contains(v)) {
                    let ri = find(&mut parent, i);
                    let rj = find(&mut parent, j);
                    if ri != rj {
                        parent[ri] = rj;
                    }
                }
            }
        }

        // Collect distinct roots and check for multiple components.
        let roots: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
        let mut unique_roots: Vec<usize> = roots.clone();
        unique_roots.sort_unstable();
        unique_roots.dedup();

        if unique_roots.len() <= 1 {
            return None;
        }

        // Remap roots to sequential component indices 0, 1, 2, ...
        let components: Vec<usize> = roots
            .iter()
            .map(|r| unique_roots.iter().position(|u| u == r).unwrap())
            .collect();
        Some(components)
    }

    /// Split a non-manifold vertex into one vertex per connected fan
    /// component. Only considers triangles of the specified surface type.
    /// The first component keeps the original vertex index; each additional
    /// component gets a duplicate. Triangles of the other surface type are
    /// left on the original vertex.
    fn split_non_manifold_vertex(&mut self, vi: u32, leaf_only: bool) -> bool {
        // Re-collect same-type incident triangles (indices may have shifted
        // from earlier splits in the same pass).
        let incident: Vec<usize> = self
            .triangles
            .iter()
            .enumerate()
            .filter(|(ti, tri)| {
                tri.contains(&vi) && (self.triangle_tags[*ti] == TAG_LEAF) == leaf_only
            })
            .map(|(ti, _)| ti)
            .collect();

        if incident.len() < 2 {
            return false;
        }

        let components = match self.fan_components(vi, &incident) {
            Some(c) => c,
            None => return false,
        };

        let n_components = *components.iter().max().unwrap() + 1;

        // Component 0 keeps original vertex. Each additional component gets
        // a duplicate.
        for comp in 1..n_components {
            let new_vi = self.vertices.len() as u32;
            let orig_vertex = self.vertices[vi as usize].clone();
            self.vertices.push(orig_vertex);

            // Rewrite triangles in this component to use the new vertex.
            for (local_idx, &ti) in incident.iter().enumerate() {
                if components[local_idx] == comp {
                    let tri = &mut self.triangles[ti];
                    for v in tri.iter_mut() {
                        if *v == vi {
                            *v = new_vi;
                        }
                    }
                }
            }
        }

        // Remove from dedup (now ambiguous).
        let key = VertexKey::from_position(self.vertices[vi as usize].position);
        self.dedup.remove(&key);

        true
    }

    /// Rebuild all neighbor lists from scratch using triangle connectivity.
    /// Called after non-manifold vertex splitting to ensure neighbor lists
    /// are consistent with the (modified) triangle data.
    fn rebuild_neighbors(&mut self) {
        for v in &mut self.vertices {
            v.neighbors.clear();
        }
        // Push edges unconditionally (no contains check). Duplicates are
        // removed by the deduplicate_neighbors call at the end.
        for ti in 0..self.triangles.len() {
            let [a, b, c] = self.triangles[ti];
            for (x, y) in [(a, b), (a, c), (b, c)] {
                self.vertices[x as usize].neighbors.push(y);
                self.vertices[y as usize].neighbors.push(x);
            }
        }
        self.deduplicate_neighbors();
    }

    /// Recompute `initial_normal`, `has_solid_face`, and `has_leaf_face` for
    /// all vertices from their incident triangles. Called after splitting to
    /// ensure both original and duplicated vertices have correct metadata
    /// (since the original lost some incident triangles during the split).
    fn recompute_split_vertex_metadata(&mut self) {
        // Reset all vertices.
        for v in &mut self.vertices {
            v.initial_normal = [0.0; 3];
            v.has_solid_face = false;
            v.has_leaf_face = false;
        }

        // Accumulate from triangles. This adds the face normal once per
        // incident triangle (8 per voxel face), whereas the original
        // get_or_create_vertex accumulates once per voxel face. The direction
        // is identical after normalize_initial_normals since all triangles
        // from the same voxel face share the same axis-aligned face normal.
        for (ti, tri) in self.triangles.iter().enumerate() {
            let fn_ = self.triangle_face_normals[ti];
            let tag = self.triangle_tags[ti];
            let is_leaf = tag == TAG_LEAF;

            for &vi in tri {
                let v = &mut self.vertices[vi as usize];
                v.initial_normal[0] += fn_[0];
                v.initial_normal[1] += fn_[1];
                v.initial_normal[2] += fn_[2];
                if is_leaf {
                    v.has_leaf_face = true;
                } else {
                    v.has_solid_face = true;
                }
            }
        }
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

            // Precompute centroid of vi's neighbors (constant across offsets
            // since only vi moves, not its neighbors).
            let vi_inv_n = 1.0 / neighbors.len() as f32;
            let mut vi_centroid = [0.0f32; 3];
            for &ni in &neighbors {
                let np = self.vertices[ni as usize].position;
                vi_centroid[0] += np[0];
                vi_centroid[1] += np[1];
                vi_centroid[2] += np[2];
            }
            vi_centroid[0] *= vi_inv_n;
            vi_centroid[1] *= vi_inv_n;
            vi_centroid[2] *= vi_inv_n;

            // Precompute base centroids for each neighbor (using vi's original
            // position). When vi moves by delta, neighbor ni's centroid shifts
            // by delta / deg(ni). This avoids recomputing centroids from
            // scratch for each of the 5 offset candidates.
            struct NeighborCache {
                pos: [f32; 3],
                base_centroid: [f32; 3],
                inv_deg: f32,
            }
            let neighbor_caches: Vec<NeighborCache> = neighbors
                .iter()
                .map(|&ni| {
                    let nv = &self.vertices[ni as usize];
                    let n_neighbors = &nv.neighbors;
                    let inv_deg = 1.0 / n_neighbors.len() as f32;
                    let mut cx = 0.0f32;
                    let mut cy = 0.0f32;
                    let mut cz = 0.0f32;
                    for &nni in n_neighbors {
                        let nnp = self.vertices[nni as usize].position;
                        cx += nnp[0];
                        cy += nnp[1];
                        cz += nnp[2];
                    }
                    cx *= inv_deg;
                    cy *= inv_deg;
                    cz *= inv_deg;
                    NeighborCache {
                        pos: nv.position,
                        base_centroid: [cx, cy, cz],
                        inv_deg,
                    }
                })
                .collect();

            for &offset in &offsets {
                let candidate_pos = [
                    original_pos[0] + offset * normal[0],
                    original_pos[1] + offset * normal[1],
                    original_pos[2] + offset * normal[2],
                ];

                // Pointiness of vi: distance from candidate_pos to centroid
                // of vi's neighbors (centroid is constant).
                let dx = vi_centroid[0] - candidate_pos[0];
                let dy = vi_centroid[1] - candidate_pos[1];
                let dz = vi_centroid[2] - candidate_pos[2];
                let k = (dx * dx + dy * dy + dz * dz).sqrt();
                let mut cost = k * k;

                // Delta from original position to candidate position.
                let delta = [
                    candidate_pos[0] - original_pos[0],
                    candidate_pos[1] - original_pos[1],
                    candidate_pos[2] - original_pos[2],
                ];

                // Pointiness of each neighbor: incrementally adjust centroid.
                for nc in &neighbor_caches {
                    let adj_centroid = [
                        nc.base_centroid[0] + delta[0] * nc.inv_deg,
                        nc.base_centroid[1] + delta[1] * nc.inv_deg,
                        nc.base_centroid[2] + delta[2] * nc.inv_deg,
                    ];
                    let dx = adj_centroid[0] - nc.pos[0];
                    let dy = adj_centroid[1] - nc.pos[1];
                    let dz = adj_centroid[2] - nc.pos[2];
                    let k = (dx * dx + dy * dy + dz * dz).sqrt();
                    cost += k * k;
                }

                // If the vertex was NOT a saddle point before, check if
                // this candidate position creates one. If so, reject it.
                if !was_saddle {
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
    /// vertex to the centroid of its neighbors. Only used in tests now —
    /// the production smoothing loop precomputes centroids incrementally.
    #[cfg(test)]
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
        // Pre-estimate output size: count matching triangles to pre-allocate.
        // Each triangle emits 3 vertices (no sharing due to per-tri colors).
        let matching_tris = self
            .triangles
            .iter()
            .enumerate()
            .filter(|&(ti, _)| {
                if let Some(tag) = tag_filter
                    && self.triangle_tags[ti] != tag
                {
                    return false;
                }
                if let Some((min, max)) = bounds {
                    let v = self.triangle_voxel_pos[ti];
                    if v[0] < min[0]
                        || v[0] >= max[0]
                        || v[1] < min[1]
                        || v[1] >= max[1]
                        || v[2] < min[2]
                        || v[2] >= max[2]
                    {
                        return false;
                    }
                }
                true
            })
            .count();
        let mut surface = SurfaceMesh {
            vertices: Vec::with_capacity(matching_tris * 9), // 3 verts × 3 floats
            normals: Vec::with_capacity(matching_tris * 9),  // 3 verts × 3 floats
            indices: Vec::with_capacity(matching_tris * 3),  // 3 indices
            colors: Vec::with_capacity(matching_tris * 12),  // 3 verts × 4 floats
            uvs: Vec::new(),
        };

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
            let use_smooth_normals = self.config.smooth_normals_enabled;

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

                // Leaf surfaces use per-vertex sway weight as color metadata;
                // non-leaf surfaces use the per-triangle voxel color.
                if tag_filter == Some(TAG_LEAF) {
                    surface.colors.push(v.sway_weight);
                    surface.colors.push(0.0);
                    surface.colors.push(0.0);
                    surface.colors.push(0.0);
                } else {
                    surface.colors.push(color[0]);
                    surface.colors.push(color[1]);
                    surface.colors.push(color[2]);
                    surface.colors.push(color[3]);
                }
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
    use elven_canopy_sim::types::VoxelType;

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
        mesh.deduplicate_neighbors();
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
        mesh.deduplicate_neighbors();
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
        mesh.deduplicate_neighbors();
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

    // --- Non-manifold detection utilities ---

    /// Check for non-manifold triangle-mesh edges (shared by 3+ triangles)
    /// and boundary edges (shared by only 1 triangle). Returns
    /// `(boundary_edges, non_manifold_edges)`. An empty pair means the mesh
    /// is watertight with manifold edges.
    fn check_watertight(mesh: &SmoothMesh) -> (Vec<(u32, u32)>, Vec<(u32, u32)>) {
        let mut edge_counts: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        for tri in &mesh.triangles {
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                let edge = if a < b { (a, b) } else { (b, a) };
                *edge_counts.entry(edge).or_insert(0) += 1;
            }
        }
        let mut boundary = Vec::new();
        let mut non_manifold = Vec::new();
        for (edge, count) in &edge_counts {
            match count {
                1 => boundary.push(*edge),
                2 => {}
                _ => non_manifold.push(*edge),
            }
        }
        (boundary, non_manifold)
    }

    /// Check for non-manifold vertices — vertices where incident triangles
    /// don't form a single connected fan. Two triangles incident to vertex v
    /// are "fan-adjacent" if they share a triangle-mesh edge that includes v
    /// (i.e., both triangles contain v and some other shared vertex w).
    /// Returns the list of non-manifold vertex indices.
    fn check_manifold_vertices(mesh: &SmoothMesh) -> Vec<u32> {
        let vtx_tris = mesh.build_vertex_triangle_index();
        let mut non_manifold = Vec::new();
        for (vi, tri_indices) in vtx_tris.iter().enumerate() {
            if tri_indices.len() < 2 {
                continue;
            }
            if mesh.fan_components(vi as u32, tri_indices).is_some() {
                non_manifold.push(vi as u32);
            }
        }
        non_manifold
    }

    /// Assert that a mesh is fully manifold: no boundary edges, no
    /// non-manifold triangle-mesh edges, and no non-manifold vertices.
    fn assert_fully_manifold(mesh: &SmoothMesh, label: &str) {
        let (boundary, nm_edges) = check_watertight(mesh);
        assert!(
            boundary.is_empty(),
            "{label}: mesh has {} boundary edges (holes). First 5: {:?}",
            boundary.len(),
            &boundary[..boundary.len().min(5)]
        );
        assert!(
            nm_edges.is_empty(),
            "{label}: mesh has {} non-manifold triangle-mesh edges. First 5: {:?}",
            nm_edges.len(),
            &nm_edges[..nm_edges.len().min(5)]
        );
        let nm_verts = check_manifold_vertices(mesh);
        assert!(
            nm_verts.is_empty(),
            "{label}: mesh has {} non-manifold vertices. First 5: {:?}",
            nm_verts.len(),
            &nm_verts[..nm_verts.len().min(5)]
        );
    }

    /// Check manifoldness for a single surface type's triangles within a
    /// mesh. Filters to only triangles matching `leaf_only`, then checks
    /// for non-manifold triangle-mesh edges and non-manifold vertices
    /// within that subset. This matches what `resolve_non_manifold`
    /// guarantees: per-surface-type manifoldness (solid and leaf are
    /// resolved independently).
    fn assert_surface_manifold(mesh: &SmoothMesh, leaf_only: bool, label: &str) {
        let tag_label = if leaf_only { "leaf" } else { "solid" };

        // Filter triangles to this surface type.
        let filtered_tris: Vec<usize> = (0..mesh.triangles.len())
            .filter(|&ti| (mesh.triangle_tags[ti] == TAG_LEAF) == leaf_only)
            .collect();

        // Check triangle-mesh edges within filtered triangles.
        let mut edge_counts: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        for &ti in &filtered_tris {
            let tri = &mesh.triangles[ti];
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                let edge = if a < b { (a, b) } else { (b, a) };
                *edge_counts.entry(edge).or_insert(0) += 1;
            }
        }
        let nm_edges: Vec<_> = edge_counts
            .iter()
            .filter(|(_, c)| **c > 2)
            .map(|(e, _)| *e)
            .collect();
        assert!(
            nm_edges.is_empty(),
            "{label} ({tag_label}): {} non-manifold triangle-mesh edges. First 5: {:?}",
            nm_edges.len(),
            &nm_edges[..nm_edges.len().min(5)]
        );

        // Check vertex fan connectivity within filtered triangles.
        let mut vtx_tris: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for &ti in &filtered_tris {
            for &vi in &mesh.triangles[ti] {
                vtx_tris.entry(vi).or_default().push(ti);
            }
        }
        let mut nm_verts = Vec::new();
        for (&vi, tri_indices) in &vtx_tris {
            if tri_indices.len() < 2 {
                continue;
            }
            if mesh.fan_components(vi, tri_indices).is_some() {
                nm_verts.push(vi);
            }
        }
        assert!(
            nm_verts.is_empty(),
            "{label} ({tag_label}): {} non-manifold vertices. First 5: {:?}",
            nm_verts.len(),
            &nm_verts[..nm_verts.len().min(5)]
        );
    }

    /// Assert per-surface-type manifoldness: solid triangles are manifold
    /// among themselves, and leaf triangles are manifold among themselves.
    fn assert_per_surface_manifold(mesh: &SmoothMesh, label: &str) {
        assert_surface_manifold(mesh, false, label);
        assert_surface_manifold(mesh, true, label);
    }

    /// Build a subdivided mesh from voxel positions with per-voxel surface
    /// tags. Face culling uses the same rules as `build_subdivided_voxel_mesh`
    /// but does NOT apply asymmetric solid/leaf culling (all face-adjacent
    /// pairs are fully culled). For testing non-manifold resolution with
    /// mixed surface types.
    fn build_subdivided_typed_voxel_mesh(voxels: &[((i32, i32, i32), SurfaceTag)]) -> SmoothMesh {
        let mut mesh = SmoothMesh::new();
        let voxel_set: std::collections::HashSet<(i32, i32, i32)> =
            voxels.iter().map(|&(pos, _)| pos).collect();

        for &((wx, wy, wz), tag) in voxels {
            let color = if tag == TAG_LEAF {
                voxel_color(VoxelType::Leaf)
            } else {
                voxel_color(VoxelType::Trunk)
            };
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
                mesh.add_subdivided_face(corners, FACE_NORMALS[face_idx], color, tag, [wx, wy, wz]);
            }
        }

        mesh.deduplicate_neighbors();
        mesh
    }

    /// Build a subdivided mesh from a set of voxel positions. Generates all
    /// visible faces (face culling based on which positions are in the set)
    /// and deduplicates vertices. Does NOT normalize, anchor, or chamfer —
    /// returns the raw post-subdivision mesh for testing non-manifold
    /// detection and splitting.
    fn build_subdivided_voxel_mesh(voxels: &[(i32, i32, i32)]) -> SmoothMesh {
        let typed: Vec<_> = voxels.iter().map(|&pos| (pos, TAG_BARK)).collect();
        build_subdivided_typed_voxel_mesh(&typed)
    }

    /// Build a smooth mesh from a set of voxel positions. Generates all
    /// visible faces (face culling based on which positions are in the set),
    /// normalizes, anchors, and chamfers.
    fn build_chamfered_voxel_mesh(voxels: &[(i32, i32, i32)]) -> SmoothMesh {
        let mut mesh = build_subdivided_voxel_mesh(voxels);
        mesh.resolve_non_manifold();
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
        let _all_voxels = solid_voxels.clone();
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
        mesh_with_leaf.deduplicate_neighbors();
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
                let (_dx, _dy, dz) = [
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
        mesh.deduplicate_neighbors();
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
        use elven_canopy_sim::world::VoxelZone;

        let mut world = VoxelZone::new(32, 16, 16);
        // Create hilly terrain: varying heights across the chunk boundary.
        for x in 0..32 {
            let height = 3 + (x % 3); // Heights: 3, 4, 5, 3, 4, 5, ...
            for y in 0..height {
                for z in 0..16 {
                    world.set(
                        elven_canopy_sim::types::VoxelCoord::new(x, y, z),
                        VoxelType::Dirt,
                    );
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
        use elven_canopy_sim::world::VoxelZone;

        let mut world = VoxelZone::new(32, 16, 16);
        for x in 0..32 {
            let height = 3 + (x % 3);
            for y in 0..height {
                for z in 0..16 {
                    world.set(
                        elven_canopy_sim::types::VoxelCoord::new(x, y, z),
                        VoxelType::Dirt,
                    );
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
        use elven_canopy_sim::world::VoxelZone;

        let mut world = VoxelZone::new(16, 16, 16);
        world.set(
            elven_canopy_sim::types::VoxelCoord::new(8, 8, 8),
            VoxelType::Trunk,
        );
        world.set(
            elven_canopy_sim::types::VoxelCoord::new(9, 8, 8),
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
        world: &elven_canopy_sim::world::VoxelZone,
        chunk: crate::mesh_gen::ChunkCoord,
        border: i32,
    ) -> SmoothMesh {
        build_smooth_chunk_n(world, chunk, border, 3)
    }

    /// Build a smooth mesh for a single chunk with configurable iteration
    /// count. Used by `chunk_boundary_iteration_limit` to test how many
    /// iterations are safe for a given border radius.
    fn build_smooth_chunk_n(
        world: &elven_canopy_sim::world::VoxelZone,
        chunk: crate::mesh_gen::ChunkCoord,
        border: i32,
        iterations: usize,
    ) -> SmoothMesh {
        use crate::mesh_gen::CHUNK_SIZE;
        use elven_canopy_sim::types::VoxelCoord;

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

        mesh.deduplicate_neighbors();
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

    // --- Non-manifold resolution tests ---

    #[test]
    fn single_voxel_is_fully_manifold() {
        // Sanity check: a single voxel's subdivided mesh should be manifold.
        let mesh = build_subdivided_voxel_mesh(&[(0, 0, 0)]);
        assert_fully_manifold(&mesh, "single voxel");
    }

    #[test]
    fn two_diagonal_voxels_manifold_after_resolve() {
        // Two diagonally-adjacent voxels sharing a voxel edge at (1, y, 1).
        // Before resolve: non-manifold triangle-mesh edges at the shared
        // voxel-edge midpoint. After resolve: fully manifold.
        let mut mesh = build_subdivided_voxel_mesh(&[(0, 0, 0), (1, 0, 1)]);

        // Verify the mesh is non-manifold before resolving.
        let (_, nm_edges) = check_watertight(&mesh);
        assert!(
            !nm_edges.is_empty(),
            "diagonal voxels should produce non-manifold triangle-mesh edges before resolve"
        );

        // After resolve, should be fully manifold.
        mesh.resolve_non_manifold();
        assert_fully_manifold(&mesh, "two diagonal voxels after resolve");
    }

    #[test]
    fn two_corner_sharing_voxels_manifold_after_resolve() {
        // Two voxels sharing only corner (1,1,1): voxel A at (0,0,0)
        // occupies [0,1]^3, voxel B at (1,1,1) occupies [1,2]^3.
        // No shared voxel edge, so pass 1 finds nothing. But the corner
        // vertex has disconnected triangle fans — pass 2 should split it.
        let mut mesh = build_subdivided_voxel_mesh(&[(0, 0, 0), (1, 1, 1)]);

        // Before resolve: no non-manifold triangle-mesh edges (corner-only
        // sharing doesn't produce those), but the corner vertex is
        // non-manifold.
        let (_, nm_edges) = check_watertight(&mesh);
        assert!(
            nm_edges.is_empty(),
            "corner-sharing voxels should not have non-manifold triangle-mesh edges"
        );
        let nm_verts = check_manifold_vertices(&mesh);
        assert!(
            !nm_verts.is_empty(),
            "corner-sharing voxels should have non-manifold vertex before resolve"
        );

        mesh.resolve_non_manifold();
        assert_fully_manifold(&mesh, "two corner-sharing voxels after resolve");
    }

    #[test]
    fn diagonal_voxels_on_ground_manifold_after_resolve() {
        // Two diagonally-adjacent voxels at (0,0,0) and (1,0,1) sitting on
        // a row of ground voxels at y=-1. Pass 1 splits the voxel-edge
        // midpoint, but the bottom corner vertex of the shared voxel edge
        // (at position (1,0,1)) may still have disconnected fans where the
        // ground surface meets the side faces. Pass 2 catches this.
        let voxels: Vec<(i32, i32, i32)> = vec![
            // Ground layer at y=-1 under both diagonal voxels.
            (0, -1, 0),
            (1, -1, 0),
            (0, -1, 1),
            (1, -1, 1),
            // The two diagonal voxels.
            (0, 0, 0),
            (1, 0, 1),
        ];
        let mut mesh = build_subdivided_voxel_mesh(&voxels);
        mesh.resolve_non_manifold();
        assert_fully_manifold(&mesh, "diagonal voxels on ground after resolve");
    }

    /// Generate a smooth heightmap WITHOUT diagonal gap-filling. Adjacent
    /// columns differ by at most 1, but diagonal-only voxel adjacency is
    /// allowed. Used to test that `resolve_non_manifold` handles the
    /// non-manifold cases that gap-filling was previously needed to avoid.
    fn smooth_heightmap_no_gapfill(seed: u64, size: usize, base_h: i32) -> Vec<Vec<i32>> {
        let mut heights = vec![vec![base_h; size]; size];
        let mut state = seed.wrapping_add(1);
        for x in 0..size {
            for z in 0..size {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let neighbor_h = if x > 0 && z > 0 {
                    (heights[x - 1][z] + heights[x][z - 1]) / 2
                } else if x > 0 {
                    heights[x - 1][z]
                } else if z > 0 {
                    heights[x][z - 1]
                } else {
                    base_h
                };
                let delta = match state % 3 {
                    0 => -1,
                    1 => 0,
                    _ => 1,
                };
                heights[x][z] = (neighbor_h + delta).max(1);
            }
        }
        heights
    }

    /// Convert a heightmap to a list of voxel positions.
    fn heightmap_to_voxels(heights: &[Vec<i32>]) -> Vec<(i32, i32, i32)> {
        let mut voxels = Vec::new();
        for (x, col) in heights.iter().enumerate() {
            for (z, &h) in col.iter().enumerate() {
                for y in 0..h {
                    voxels.push((x as i32, y, z as i32));
                }
            }
        }
        voxels
    }

    #[test]
    fn fuzz_resolve_non_manifold_smooth_terrain() {
        // Fuzz test: generate smooth heightmaps WITHOUT gap-filling (which
        // allows diagonal-only voxel adjacency), run resolve_non_manifold,
        // and verify the result is fully manifold.
        for seed in 0..100 {
            let heights = smooth_heightmap_no_gapfill(seed, 12, 3);
            let voxels = heightmap_to_voxels(&heights);
            let mut mesh = build_subdivided_voxel_mesh(&voxels);
            mesh.resolve_non_manifold();
            assert_fully_manifold(&mesh, &format!("smooth terrain seed={seed}"));
        }
    }

    #[test]
    fn resolve_non_manifold_is_idempotent() {
        // Calling resolve twice should produce the same mesh as calling once.
        let mut mesh = build_subdivided_voxel_mesh(&[(0, 0, 0), (1, 0, 1)]);
        mesh.resolve_non_manifold();
        let verts_after_first = mesh.vertices.len();
        let tris_after_first = mesh.triangles.len();
        mesh.resolve_non_manifold();
        assert_eq!(mesh.vertices.len(), verts_after_first);
        assert_eq!(mesh.triangles.len(), tris_after_first);
        assert_fully_manifold(&mesh, "idempotent resolve");
    }

    #[test]
    fn resolve_non_manifold_preserves_triangle_count() {
        // Resolve only duplicates vertices, never adds or removes triangles.
        let voxels = vec![(0, 0, 0), (1, 0, 1)];
        let mesh_before = build_subdivided_voxel_mesh(&voxels);
        let tri_count_before = mesh_before.triangles.len();
        let mut mesh_after = build_subdivided_voxel_mesh(&voxels);
        mesh_after.resolve_non_manifold();
        assert_eq!(mesh_after.triangles.len(), tri_count_before);
        // Vertex count should increase (midpoint was split).
        assert!(
            mesh_after.vertices.len() > mesh_before.vertices.len(),
            "expected more vertices after resolve, got {} vs {}",
            mesh_after.vertices.len(),
            mesh_before.vertices.len()
        );
    }

    #[test]
    fn diagonal_voxels_all_axis_pairs() {
        // Test diagonal adjacency along all three axis pairs:
        // XZ (existing), XY, and YZ.
        let configs: &[(&str, Vec<(i32, i32, i32)>)] = &[
            ("XZ diagonal", vec![(0, 0, 0), (1, 0, 1)]),
            ("XY diagonal", vec![(0, 0, 0), (1, 1, 0)]),
            ("YZ diagonal", vec![(0, 0, 0), (0, 1, 1)]),
        ];
        for (label, voxels) in configs {
            let mut mesh = build_subdivided_voxel_mesh(voxels);
            let (_, nm_edges) = check_watertight(&mesh);
            assert!(
                !nm_edges.is_empty(),
                "{label}: should be non-manifold before resolve"
            );
            mesh.resolve_non_manifold();
            assert_fully_manifold(&mesh, label);
        }
    }

    #[test]
    fn three_diagonals_sharing_corner() {
        // Three voxels all meeting at corner (1,1,1) via different diagonal
        // voxel edges. Each pair shares a voxel edge, creating multiple
        // non-manifold midpoints plus a potentially non-manifold corner.
        let voxels = vec![(0, 0, 0), (1, 1, 0), (1, 0, 1)];
        let mut mesh = build_subdivided_voxel_mesh(&voxels);
        mesh.resolve_non_manifold();
        assert_fully_manifold(&mesh, "three diagonals sharing corner");
    }

    #[test]
    fn face_adjacent_voxels_no_splits() {
        // Two face-adjacent voxels should produce no splits — the mesh is
        // already manifold. Vertex count should not change.
        let voxels = vec![(0, 0, 0), (1, 0, 0)];
        let mesh_before = build_subdivided_voxel_mesh(&voxels);
        let vert_count_before = mesh_before.vertices.len();
        let mut mesh_after = build_subdivided_voxel_mesh(&voxels);
        mesh_after.resolve_non_manifold();
        assert_eq!(
            mesh_after.vertices.len(),
            vert_count_before,
            "face-adjacent voxels should not get any vertex splits"
        );
        assert_fully_manifold(&mesh_after, "face-adjacent voxels");
    }

    #[test]
    fn resolve_preserves_per_triangle_metadata_lengths() {
        // All per-triangle parallel arrays must remain the same length as
        // triangles after resolve (resolve doesn't add/remove triangles).
        let mut mesh = build_subdivided_voxel_mesh(&[(0, 0, 0), (1, 0, 1)]);
        mesh.resolve_non_manifold();
        let n = mesh.triangles.len();
        assert_eq!(mesh.triangle_tags.len(), n);
        assert_eq!(mesh.triangle_colors.len(), n);
        assert_eq!(mesh.triangle_voxel_pos.len(), n);
        assert_eq!(mesh.triangle_face_normals.len(), n);
        assert_eq!(mesh.triangle_face_midpoints.len(), n);
    }

    #[test]
    fn rebuild_neighbors_consistent_with_triangles() {
        // After resolve, every triangle-mesh edge should appear in both
        // vertices' neighbor lists, and vice versa.
        let mut mesh = build_subdivided_voxel_mesh(&[(0, 0, 0), (1, 0, 1)]);
        mesh.resolve_non_manifold();
        // Check: every triangle edge is in neighbor lists.
        for tri in &mesh.triangles {
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                assert!(
                    mesh.vertices[a as usize].neighbors.contains(&b),
                    "vertex {a} missing neighbor {b}"
                );
                assert!(
                    mesh.vertices[b as usize].neighbors.contains(&a),
                    "vertex {b} missing neighbor {a}"
                );
            }
        }
        // Check: every neighbor pair appears in at least one triangle.
        for (vi, v) in mesh.vertices.iter().enumerate() {
            for &ni in &v.neighbors {
                let vi32 = vi as u32;
                let in_tri = mesh
                    .triangles
                    .iter()
                    .any(|tri| tri.contains(&vi32) && tri.contains(&ni));
                assert!(
                    in_tri,
                    "neighbor edge ({vi}, {ni}) not found in any triangle"
                );
            }
        }
    }

    #[test]
    fn l_shaped_diagonal_manifold() {
        // L-shaped arrangement with a diagonal on a vertical face: voxels
        // stacked vertically at (0,0,0)+(0,1,0) with a diagonal at (1,0,1).
        let voxels = vec![(0, 0, 0), (0, 1, 0), (1, 0, 1)];
        let mut mesh = build_subdivided_voxel_mesh(&voxels);
        mesh.resolve_non_manifold();
        assert_fully_manifold(&mesh, "L-shaped diagonal");
    }

    // --- Mixed solid/leaf surface type tests ---

    #[test]
    fn leaf_diagonal_voxels_manifold_after_resolve() {
        // Two diagonally-adjacent leaf voxels. Exercises the leaf_only=true
        // code path in resolve_non_manifold_pass1 and pass2.
        let voxels = vec![((0, 0, 0), TAG_LEAF), ((1, 0, 1), TAG_LEAF)];
        let mut mesh = build_subdivided_typed_voxel_mesh(&voxels);
        mesh.resolve_non_manifold();
        assert_per_surface_manifold(&mesh, "leaf diagonal voxels");
    }

    #[test]
    fn mixed_solid_leaf_diagonal_no_spurious_solid_splits() {
        // A solid voxel and a leaf voxel diagonally adjacent. The shared
        // voxel-edge midpoint has solid faces from one voxel and leaf faces
        // from the other. The solid-only pass should NOT split the midpoint
        // (only 2 solid faces = manifold from solid's perspective). The
        // leaf-only pass should also NOT split (only 2 leaf faces).
        let voxels = vec![((0, 0, 0), TAG_BARK), ((1, 0, 1), TAG_LEAF)];
        let mesh_before = build_subdivided_typed_voxel_mesh(&voxels);
        let vert_count_before = mesh_before.vertices.len();

        let mut mesh_after = build_subdivided_typed_voxel_mesh(&voxels);
        mesh_after.resolve_non_manifold();

        // No vertices should be split — each surface type has only 2 faces
        // at the shared midpoint, which is manifold.
        assert_eq!(
            mesh_after.vertices.len(),
            vert_count_before,
            "mixed solid/leaf diagonal should not produce any vertex splits"
        );
        assert_per_surface_manifold(&mesh_after, "mixed solid/leaf diagonal");
    }

    #[test]
    fn two_solid_diagonal_with_leaf_neighbor_no_spurious_splits() {
        // Two solid voxels diagonally adjacent, plus a leaf voxel
        // face-adjacent to one of them. The solid-only pass should split
        // the midpoint (4 solid faces at the shared voxel edge). The leaf
        // faces should not interfere.
        let voxels = vec![
            ((0, 0, 0), TAG_BARK),
            ((1, 0, 1), TAG_BARK),
            ((2, 0, 1), TAG_LEAF), // face-adjacent to second solid
        ];
        let mut mesh = build_subdivided_typed_voxel_mesh(&voxels);
        mesh.resolve_non_manifold();
        assert_per_surface_manifold(&mesh, "solid diagonal + leaf neighbor");
    }

    #[test]
    fn diagonal_voxels_full_pipeline_manifold() {
        // Run the full pipeline (resolve + normalize + anchor + chamfer) on
        // diagonal voxels and verify the output is still manifold. Chamfer
        // moves vertices and could theoretically create issues.
        let mesh = build_chamfered_voxel_mesh(&[(0, 0, 0), (1, 0, 1)]);
        assert_fully_manifold(&mesh, "diagonal voxels full pipeline");
    }

    #[test]
    fn fuzz_resolve_non_manifold_random_3d_clusters() {
        // Fuzz test with random 3D voxel clusters (not just heightmaps).
        // Exercises configurations like overhangs, floating voxels, and
        // multi-axis diagonal adjacency that heightmaps can't produce.
        for seed in 0..50 {
            let mut state: u64 = seed + 1;
            let mut voxels = Vec::new();
            for _ in 0..30 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let x = (state % 6) as i32;
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let y = (state % 6) as i32;
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let z = (state % 6) as i32;
                voxels.push((x, y, z));
            }
            voxels.sort();
            voxels.dedup();
            let mut mesh = build_subdivided_voxel_mesh(&voxels);
            mesh.resolve_non_manifold();
            assert_fully_manifold(&mesh, &format!("3D cluster seed={seed}"));
        }
    }

    // -- dedup_preserve_order unit tests --

    #[test]
    fn dedup_preserve_order_empty() {
        let mut v: Vec<u32> = vec![];
        dedup_preserve_order(&mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn dedup_preserve_order_no_duplicates() {
        let mut v = vec![1, 2, 3, 4, 5];
        dedup_preserve_order(&mut v);
        assert_eq!(v, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn dedup_preserve_order_preserves_first_occurrence() {
        let mut v = vec![1, 2, 3, 2, 1, 4];
        dedup_preserve_order(&mut v);
        assert_eq!(v, vec![1, 2, 3, 4]);
    }

    #[test]
    fn dedup_preserve_order_all_same() {
        let mut v = vec![5, 5, 5, 5];
        dedup_preserve_order(&mut v);
        assert_eq!(v, vec![5]);
    }

    #[test]
    fn deduplicate_neighbors_idempotent() {
        let mut mesh = single_voxel_mesh(4.0, 4.0, 4.0, VoxelType::Trunk);
        let neighbors_after_first: Vec<Vec<u32>> =
            mesh.vertices.iter().map(|v| v.neighbors.clone()).collect();
        mesh.deduplicate_neighbors();
        let neighbors_after_second: Vec<Vec<u32>> =
            mesh.vertices.iter().map(|v| v.neighbors.clone()).collect();
        assert_eq!(neighbors_after_first, neighbors_after_second);
    }

    #[test]
    fn with_estimated_faces_zero() {
        let mesh =
            SmoothMesh::with_estimated_faces(0, crate::mesh_gen::MeshPipelineConfig::default());
        assert!(mesh.vertices.is_empty());
        assert!(mesh.triangles.is_empty());
        assert!(mesh.dedup.is_empty());
    }
}
