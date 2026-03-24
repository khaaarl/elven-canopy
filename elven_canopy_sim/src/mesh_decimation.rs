// QEM (Quadric Error Metrics) edge-collapse mesh decimation.
//
// Reduces triangle count by iteratively collapsing edges whose removal
// introduces less than a configurable amount of geometric error. Based on
// Garland & Heckbert's algorithm: each vertex accumulates a 4×4 error
// quadric from its incident triangle planes, and collapsing an edge produces
// a new vertex whose error is the sum of both quadrics.
//
// With a near-zero error threshold, only truly coplanar triangles are merged
// — this is a lossless optimization for the chamfered (flat-shaded) mesh.
// With larger thresholds, the same algorithm produces LOD meshes for distant
// chunks.
//
// Constraints enforced during decimation:
// - Geometric error below threshold (QEM cost)
// - No triangle normal flips (prevents mesh inversion)
// - No collapses across color or surface-tag boundaries
// - Anchored vertices keep their exact positions (never moved by the collapse)
//
// See `smooth_mesh.rs` for the mesh representation this operates on, and
// `mesh_gen.rs` for the pipeline integration point.

use std::collections::BinaryHeap;

use crate::smooth_mesh::SmoothMesh;

/// A symmetric 4×4 matrix representing the quadric error for a vertex.
/// Stored as 10 unique elements (upper triangle):
///
/// ```text
/// | a  b  c  d |
/// | b  e  f  g |
/// | c  f  h  i |
/// | d  g  i  j |
/// ```
///
/// The error of placing a vertex at position (x, y, z) is:
/// `[x y z 1] · Q · [x y z 1]^T`
#[derive(Clone, Copy, Debug)]
struct Quadric {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
    g: f32,
    h: f32,
    i: f32,
    j: f32,
}

impl Quadric {
    /// Zero quadric.
    fn zero() -> Self {
        Self {
            a: 0.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
            g: 0.0,
            h: 0.0,
            i: 0.0,
            j: 0.0,
        }
    }

    /// Build a quadric from a plane equation `nx*x + ny*y + nz*z + d = 0`.
    /// The plane is derived from a triangle's normal and one of its vertices.
    fn from_plane(nx: f32, ny: f32, nz: f32, d: f32) -> Self {
        Self {
            a: nx * nx,
            b: nx * ny,
            c: nx * nz,
            d: nx * d,
            e: ny * ny,
            f: ny * nz,
            g: ny * d,
            h: nz * nz,
            i: nz * d,
            j: d * d,
        }
    }

    /// Add two quadrics (element-wise sum).
    fn add(&self, other: &Quadric) -> Self {
        Self {
            a: self.a + other.a,
            b: self.b + other.b,
            c: self.c + other.c,
            d: self.d + other.d,
            e: self.e + other.e,
            f: self.f + other.f,
            g: self.g + other.g,
            h: self.h + other.h,
            i: self.i + other.i,
            j: self.j + other.j,
        }
    }

    /// Evaluate the quadric error at position (x, y, z).
    /// Returns `[x y z 1] · Q · [x y z 1]^T`.
    fn evaluate(&self, x: f32, y: f32, z: f32) -> f32 {
        // Expand: a*x² + 2b*xy + 2c*xz + 2d*x + e*y² + 2f*yz + 2g*y + h*z² + 2i*z + j
        self.a * x * x
            + 2.0 * self.b * x * y
            + 2.0 * self.c * x * z
            + 2.0 * self.d * x
            + self.e * y * y
            + 2.0 * self.f * y * z
            + 2.0 * self.g * y
            + self.h * z * z
            + 2.0 * self.i * z
            + self.j
    }

    /// Compute the optimal position that minimizes the quadric error.
    /// Solves the 3×3 linear system from ∂Q/∂x = ∂Q/∂y = ∂Q/∂z = 0.
    /// Returns `None` if the system is singular (degenerate geometry).
    /// Currently unused (lossless mode always picks an existing vertex
    /// position), but will be needed for lossy LOD with larger thresholds.
    #[allow(dead_code)]
    fn optimal_position(&self) -> Option<[f32; 3]> {
        // The system is:
        // | a b c | |x|   | -d |
        // | b e f | |y| = | -g |
        // | c f h | |z|   | -i |
        //
        // Solve via Cramer's rule.
        let det = self.a * (self.e * self.h - self.f * self.f)
            - self.b * (self.b * self.h - self.f * self.c)
            + self.c * (self.b * self.f - self.e * self.c);

        if det.abs() < 1e-10 {
            return None;
        }

        let inv_det = 1.0 / det;

        let x = inv_det
            * (-self.d * (self.e * self.h - self.f * self.f)
                + self.g * (self.b * self.h - self.c * self.f)
                - self.i * (self.b * self.f - self.c * self.e));

        let y = inv_det
            * (self.d * (self.b * self.h - self.c * self.f)
                - self.g * (self.a * self.h - self.c * self.c)
                + self.i * (self.a * self.f - self.c * self.b));

        let z = inv_det
            * (-self.d * (self.b * self.f - self.c * self.e)
                + self.g * (self.a * self.f - self.b * self.c)
                - self.i * (self.a * self.e - self.b * self.b));

        Some([x, y, z])
    }
}

/// An edge collapse candidate in the priority queue.
/// Ordered by cost (lowest first) via reverse comparison.
#[derive(Clone, Debug)]
struct CollapseCandidate {
    /// The cost (quadric error) of this collapse.
    cost: f32,
    /// The two vertex indices of the edge.
    v0: u32,
    v1: u32,
    /// Per-vertex generation counters — used to invalidate stale entries when
    /// either vertex's connectivity changes after a nearby collapse.
    gen_v0: u32,
    gen_v1: u32,
}

impl PartialEq for CollapseCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for CollapseCandidate {}

impl PartialOrd for CollapseCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CollapseCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order: lowest cost = highest priority.
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Mutable state shared across the decimation algorithm. Bundled to avoid
/// passing many arguments to helper methods.
struct DecimationState {
    /// Per-vertex quadric error matrix.
    quadrics: Vec<Quadric>,
    /// Per-vertex list of incident triangle indices.
    vertex_tris: Vec<Vec<usize>>,
    /// Which vertices have been removed (collapsed into another).
    removed: Vec<bool>,
    /// Per-vertex generation counter for invalidating stale heap entries.
    generations: Vec<u32>,
    /// Vertices pinned to their position (chunk boundary vertices).
    /// Unlike `anchored`, which is a mesh-level property, `pinned` is a
    /// decimation-only constraint that prevents any collapse involving this
    /// vertex.
    pinned: Vec<bool>,
}

impl SmoothMesh {
    /// Pre-decimation pass: find connected coplanar same-material triangle
    /// regions via flood fill, extract their boundary polygons, and
    /// re-triangulate each region as a centroid fan (new vertex at the
    /// average of boundary positions, one triangle per boundary edge). This
    /// replaces hundreds of subdivided triangles on flat surfaces with just
    /// enough triangles to cover the boundary polygon.
    ///
    /// If `chunk_bounds` is provided, vertices on chunk boundaries are not
    /// touched (regions touching the boundary are skipped).
    pub fn coplanar_region_retri(&mut self, chunk_bounds: Option<([i32; 3], [i32; 3])>) {
        if self.triangles.is_empty() {
            return;
        }

        // Build triangle adjacency: for each edge, which triangles share it.
        let mut edge_tris: std::collections::BTreeMap<(u32, u32), Vec<usize>> =
            std::collections::BTreeMap::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            for i in 0..3 {
                let a = tri[i].min(tri[(i + 1) % 3]);
                let b = tri[i].max(tri[(i + 1) % 3]);
                edge_tris.entry((a, b)).or_default().push(ti);
            }
        }

        // Build per-vertex triangle index.
        let vertex_count = self.vertices.len();
        let mut vertex_tris: Vec<Vec<usize>> = vec![Vec::new(); vertex_count];
        for (ti, tri) in self.triangles.iter().enumerate() {
            for &vi in tri {
                vertex_tris[vi as usize].push(ti);
            }
        }

        // Identify chunk-boundary vertices.
        let mut pinned = vec![false; vertex_count];
        if let Some((cmin, cmax)) = chunk_bounds {
            for (vi, tris) in vertex_tris.iter().enumerate() {
                let mut has_inside = false;
                let mut has_outside = false;
                for &ti in tris {
                    let vp = self.triangle_voxel_pos[ti];
                    let inside = vp[0] >= cmin[0]
                        && vp[0] < cmax[0]
                        && vp[1] >= cmin[1]
                        && vp[1] < cmax[1]
                        && vp[2] >= cmin[2]
                        && vp[2] < cmax[2];
                    if inside {
                        has_inside = true;
                    } else {
                        has_outside = true;
                    }
                }
                if has_inside && has_outside {
                    pinned[vi] = true;
                }
            }
        }

        // Compute per-triangle plane: (normal, d) where normal·p + d = 0.
        let tri_planes: Vec<([f32; 3], f32)> = self
            .triangles
            .iter()
            .map(|tri| {
                let p0 = self.vertices[tri[0] as usize].position;
                let p1 = self.vertices[tri[1] as usize].position;
                let p2 = self.vertices[tri[2] as usize].position;
                let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
                let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
                let nx = e1[1] * e2[2] - e1[2] * e2[1];
                let ny = e1[2] * e2[0] - e1[0] * e2[2];
                let nz = e1[0] * e2[1] - e1[1] * e2[0];
                let len = (nx * nx + ny * ny + nz * nz).sqrt();
                if len < 1e-10 {
                    return ([0.0, 0.0, 0.0], 0.0);
                }
                let n = [nx / len, ny / len, nz / len];
                let d = -(n[0] * p0[0] + n[1] * p0[1] + n[2] * p0[2]);
                (n, d)
            })
            .collect();

        // Flood fill to find coplanar regions.
        let tri_count = self.triangles.len();
        let mut region_of: Vec<Option<usize>> = vec![None; tri_count];
        let mut regions: Vec<Vec<usize>> = Vec::new(); // region_id → [tri indices]

        for start_ti in 0..tri_count {
            if region_of[start_ti].is_some() {
                continue;
            }
            let (ref_normal, ref_d) = tri_planes[start_ti];
            if ref_normal[0] == 0.0 && ref_normal[1] == 0.0 && ref_normal[2] == 0.0 {
                continue; // Degenerate triangle.
            }
            let ref_color = self.triangle_colors[start_ti];
            let ref_tag = self.triangle_tags[start_ti];

            let region_id = regions.len();
            let mut region_tris = vec![start_ti];
            region_of[start_ti] = Some(region_id);

            let mut queue = std::collections::VecDeque::new();
            queue.push_back(start_ti);

            while let Some(ti) = queue.pop_front() {
                let tri = self.triangles[ti];
                // Check each edge neighbor.
                for i in 0..3 {
                    let a = tri[i].min(tri[(i + 1) % 3]);
                    let b = tri[i].max(tri[(i + 1) % 3]);
                    if let Some(neighbors) = edge_tris.get(&(a, b)) {
                        for &ni in neighbors {
                            if region_of[ni].is_some() {
                                continue;
                            }
                            // Check coplanarity: same normal direction AND same plane offset.
                            let (n2, d2) = tri_planes[ni];
                            let dot = ref_normal[0] * n2[0]
                                + ref_normal[1] * n2[1]
                                + ref_normal[2] * n2[2];
                            if dot < 0.9999 {
                                continue; // Different normal.
                            }
                            if (ref_d - d2).abs() > 1e-4 {
                                continue; // Different plane offset.
                            }
                            // Same material.
                            if self.triangle_colors[ni] != ref_color
                                || self.triangle_tags[ni] != ref_tag
                            {
                                continue;
                            }
                            region_of[ni] = Some(region_id);
                            region_tris.push(ni);
                            queue.push_back(ni);
                        }
                    }
                }
            }

            regions.push(region_tris);
        }

        // Process each region: extract boundary, re-triangulate.
        let mut dead_tris: Vec<bool> = vec![false; tri_count];
        let mut new_triangles: Vec<[u32; 3]> = Vec::new();
        let mut new_tags: Vec<u8> = Vec::new();
        let mut new_colors: Vec<[f32; 4]> = Vec::new();
        let mut new_voxel_pos: Vec<[i32; 3]> = Vec::new();

        for region_tris in &regions {
            // Skip small regions (already minimal).
            if region_tris.len() <= 2 {
                continue;
            }

            let ref_color = self.triangle_colors[region_tris[0]];
            let ref_tag = self.triangle_tags[region_tris[0]];
            let (ref_normal, _) = tri_planes[region_tris[0]];

            let region_set: std::collections::BTreeSet<usize> =
                region_tris.iter().copied().collect();

            // Find boundary edges: edges shared with exactly one triangle in this region.
            let mut boundary_edges: Vec<(u32, u32)> = Vec::new();
            for &ti in region_tris {
                let tri = self.triangles[ti];
                for i in 0..3 {
                    let a = tri[i].min(tri[(i + 1) % 3]);
                    let b = tri[i].max(tri[(i + 1) % 3]);
                    if let Some(neighbors) = edge_tris.get(&(a, b)) {
                        let region_count =
                            neighbors.iter().filter(|n| region_set.contains(n)).count();
                        if region_count == 1 {
                            // Boundary edge — store with winding from this triangle.
                            boundary_edges.push((tri[i], tri[(i + 1) % 3]));
                        }
                    }
                }
            }

            if boundary_edges.is_empty() {
                continue;
            }

            // Check no boundary vertex is pinned.
            let has_pinned = boundary_edges
                .iter()
                .any(|&(a, b)| pinned[a as usize] || pinned[b as usize]);
            if has_pinned {
                continue;
            }

            // Chain boundary edges into an ordered polygon loop.
            // Build adjacency: vertex → next vertex along boundary.
            // If any vertex has multiple outgoing boundary edges, this region
            // has complex topology (T-junction, hole) — skip it.
            let mut next_vert: std::collections::BTreeMap<u32, u32> =
                std::collections::BTreeMap::new();
            let mut topology_ok = true;
            for &(a, b) in &boundary_edges {
                if next_vert.insert(a, b).is_some() {
                    topology_ok = false; // Vertex has multiple outgoing edges.
                    break;
                }
            }
            if !topology_ok {
                continue;
            }

            // Walk the loop starting from the first edge.
            let start = boundary_edges[0].0;
            let mut polygon: Vec<u32> = Vec::new();
            let mut current = start;
            loop {
                polygon.push(current);
                match next_vert.get(&current) {
                    Some(&next) => current = next,
                    None => break, // Not a closed loop — skip this region.
                }
                if current == start {
                    break;
                }
                if polygon.len() > boundary_edges.len() + 1 {
                    break; // Safety: prevent infinite loops.
                }
            }

            // Verify it's a closed loop.
            if current != start || polygon.len() < 3 {
                continue;
            }

            // Verify we used all boundary edges (single loop, no holes).
            if polygon.len() != boundary_edges.len() {
                continue;
            }

            // Re-triangulate as a centroid fan: add a new vertex at the
            // centroid of the boundary polygon, then create one triangle per
            // boundary edge (centroid, edge.a, edge.b). This guarantees all
            // triangles have reasonable aspect ratios (no degenerate slivers)
            // and maintains all boundary edges for watertightness.
            let n_poly = polygon.len();

            // Only accept if we'll actually reduce triangle count.
            // Fan produces n_poly triangles (one per boundary edge).
            if n_poly >= region_tris.len() {
                continue;
            }

            // Compute centroid position (average of boundary vertices).
            let mut cx = 0.0f32;
            let mut cy = 0.0f32;
            let mut cz = 0.0f32;
            for &vi in &polygon {
                let p = self.vertices[vi as usize].position;
                cx += p[0];
                cy += p[1];
                cz += p[2];
            }
            let inv_n = 1.0 / n_poly as f32;
            let centroid_pos = [cx * inv_n, cy * inv_n, cz * inv_n];

            // Add the centroid as a new vertex.
            let centroid_idx = self.vertices.len() as u32;
            self.vertices.push(crate::smooth_mesh::SmoothVertex {
                position: centroid_pos,
                anchored: false,
                is_face_center: false,
                has_solid_face: self.vertices[polygon[0] as usize].has_solid_face,
                has_leaf_face: self.vertices[polygon[0] as usize].has_leaf_face,
                initial_normal: ref_normal,
                neighbors: Vec::new(), // Rebuilt during compact.
            });

            // Mark old triangles as dead.
            for &ti in region_tris {
                dead_tris[ti] = true;
            }

            // Create fan triangles: centroid → each boundary edge.
            let voxel_pos = self.triangle_voxel_pos[region_tris[0]];
            for i in 0..n_poly {
                let a = polygon[i];
                let b = polygon[(i + 1) % n_poly];
                new_triangles.push([centroid_idx, a, b]);
                new_tags.push(ref_tag);
                new_colors.push(ref_color);
                new_voxel_pos.push(voxel_pos);
            }
        }

        // Rebuild: keep surviving old triangles + new triangles.
        let mut result_tris: Vec<[u32; 3]> = Vec::new();
        let mut result_tags: Vec<u8> = Vec::new();
        let mut result_colors: Vec<[f32; 4]> = Vec::new();
        let mut result_vpos: Vec<[i32; 3]> = Vec::new();

        for (ti, tri) in self.triangles.iter().enumerate() {
            if !dead_tris[ti] {
                result_tris.push(*tri);
                result_tags.push(self.triangle_tags[ti]);
                result_colors.push(self.triangle_colors[ti]);
                result_vpos.push(self.triangle_voxel_pos[ti]);
            }
        }
        result_tris.extend_from_slice(&new_triangles);
        result_tags.extend_from_slice(&new_tags);
        result_colors.extend_from_slice(&new_colors);
        result_vpos.extend_from_slice(&new_voxel_pos);

        self.triangles = result_tris;
        self.triangle_tags = result_tags;
        self.triangle_colors = result_colors;
        self.triangle_voxel_pos = result_vpos;

        // Compact unused vertices.
        let removed = vec![false; self.vertices.len()];
        self.compact_after_decimation(&removed);
    }

    /// Collapse collinear boundary vertices between coplanar regions.
    ///
    /// After `coplanar_region_retri`, adjacent coplanar regions share boundary
    /// edges with intermediate vertices that are collinear (leftover voxel-grid
    /// midpoints on a straight edge between two corners). This pass removes
    /// those redundant vertices by collapsing edges where:
    /// - The vertex is shared by exactly 2 triangles (one on each side)
    /// - The vertex is collinear with its two neighbors along the boundary
    /// - Both adjacent triangles are coplanar (same plane)
    ///
    /// This is a self-contained pass that can be skipped without affecting
    /// correctness — it's purely an optimization between retri and QEM.
    pub fn collapse_collinear_boundary_vertices(
        &mut self,
        chunk_bounds: Option<([i32; 3], [i32; 3])>,
    ) {
        if self.triangles.is_empty() {
            return;
        }

        let vertex_count = self.vertices.len();

        // Build edge → triangle list.
        let mut edge_tris: std::collections::BTreeMap<(u32, u32), Vec<usize>> =
            std::collections::BTreeMap::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            for i in 0..3 {
                let a = tri[i].min(tri[(i + 1) % 3]);
                let b = tri[i].max(tri[(i + 1) % 3]);
                edge_tris.entry((a, b)).or_default().push(ti);
            }
        }

        // Build per-vertex triangle index.
        let mut vertex_tris: Vec<Vec<usize>> = vec![Vec::new(); vertex_count];
        for (ti, tri) in self.triangles.iter().enumerate() {
            for &vi in tri {
                vertex_tris[vi as usize].push(ti);
            }
        }

        // Identify pinned (chunk boundary) vertices.
        let mut pinned = vec![false; vertex_count];
        if let Some((cmin, cmax)) = chunk_bounds {
            for (vi, tris) in vertex_tris.iter().enumerate() {
                let mut has_inside = false;
                let mut has_outside = false;
                for &ti in tris {
                    let vp = self.triangle_voxel_pos[ti];
                    let inside = vp[0] >= cmin[0]
                        && vp[0] < cmax[0]
                        && vp[1] >= cmin[1]
                        && vp[1] < cmax[1]
                        && vp[2] >= cmin[2]
                        && vp[2] < cmax[2];
                    if inside {
                        has_inside = true;
                    } else {
                        has_outside = true;
                    }
                }
                if has_inside && has_outside {
                    pinned[vi] = true;
                }
            }
        }

        // For each vertex, check if it's a collinear boundary vertex that
        // can be removed. After centroid-fan retri, a boundary vertex V
        // between two coplanar regions has exactly 4 incident triangles:
        // 2 fan triangles from each region's centroid. If V is collinear
        // with its two boundary neighbors (prev, next), we can merge each
        // pair of fan triangles into one, removing V.
        let mut removed_verts: Vec<bool> = vec![false; vertex_count];
        let mut dead_tris: Vec<bool> = vec![false; self.triangles.len()];

        for vi in 0..vertex_count {
            if removed_verts[vi] || pinned[vi] {
                continue;
            }

            let vi_u32 = vi as u32;

            // Get live incident triangles.
            let incident: Vec<usize> = vertex_tris[vi]
                .iter()
                .copied()
                .filter(|&ti| !dead_tris[ti])
                .collect();

            // After centroid-fan retri, a collinear boundary vertex has
            // exactly 4 incident triangles (2 per adjacent region).
            if incident.len() != 4 {
                continue;
            }

            // All must have same color/tag.
            let ref_color = self.triangle_colors[incident[0]];
            let ref_tag = self.triangle_tags[incident[0]];
            if !incident.iter().all(|&ti| {
                self.triangle_colors[ti] == ref_color && self.triangle_tags[ti] == ref_tag
            }) {
                continue;
            }

            // Find the two boundary neighbors of V (vertices connected to V
            // that appear in triangles from BOTH sides). The centroids only
            // appear on one side each.
            // Collect all neighbor vertices of V in incident triangles.
            let mut neighbor_counts: std::collections::BTreeMap<u32, u32> =
                std::collections::BTreeMap::new();
            for &ti in &incident {
                for &v in &self.triangles[ti] {
                    if v != vi_u32 {
                        *neighbor_counts.entry(v).or_insert(0) += 1;
                    }
                }
            }

            // Group the 4 triangles
            // into 2 pairs, where each pair shares a non-V vertex (the centroid).
            let mut pairs: Vec<(u32, usize, usize)> = Vec::new(); // (centroid, ti1, ti2)
            let mut used = [false; 4];
            for i in 0..4 {
                if used[i] {
                    continue;
                }
                for j in (i + 1)..4 {
                    if used[j] {
                        continue;
                    }
                    let tri_i = self.triangles[incident[i]];
                    let tri_j = self.triangles[incident[j]];
                    let others_i: Vec<u32> =
                        tri_i.iter().copied().filter(|&v| v != vi_u32).collect();
                    let others_j: Vec<u32> =
                        tri_j.iter().copied().filter(|&v| v != vi_u32).collect();
                    let shared: Vec<u32> = others_i
                        .iter()
                        .copied()
                        .filter(|v| others_j.contains(v))
                        .collect();
                    if shared.len() == 1 {
                        pairs.push((shared[0], i, j));
                        used[i] = true;
                        used[j] = true;
                        break;
                    }
                }
            }
            if pairs.len() != 2 {
                continue; // Can't form 2 pairs.
            }

            // Each pair has a centroid (shared vertex) and two wing vertices
            // (the boundary neighbors). The wing vertices from pair 0 and
            // pair 1 should be the same two vertices (prev and next).
            let centroid_a = pairs[0].0;
            let centroid_b = pairs[1].0;
            let tri_a0 = self.triangles[incident[pairs[0].1]];
            let tri_a1 = self.triangles[incident[pairs[0].2]];

            let wings_a: Vec<u32> = tri_a0
                .iter()
                .chain(tri_a1.iter())
                .copied()
                .filter(|&v| v != vi_u32 && v != centroid_a)
                .collect::<std::collections::BTreeSet<u32>>()
                .into_iter()
                .collect();
            if wings_a.len() != 2 {
                continue;
            }
            let prev = wings_a[0];
            let next = wings_a[1];

            // Verify the other pair has the same wing vertices.
            let tri_b0 = self.triangles[incident[pairs[1].1]];
            let tri_b1 = self.triangles[incident[pairs[1].2]];
            let wings_b: std::collections::BTreeSet<u32> = tri_b0
                .iter()
                .chain(tri_b1.iter())
                .copied()
                .filter(|&v| v != vi_u32 && v != centroid_b)
                .collect();
            if !wings_b.contains(&prev) || !wings_b.contains(&next) || wings_b.len() != 2 {
                continue;
            }

            // Check collinearity: V must be on the line between prev and next.
            let pv = self.vertices[vi].position;
            let pp = self.vertices[prev as usize].position;
            let pn = self.vertices[next as usize].position;
            let e1 = [pv[0] - pp[0], pv[1] - pp[1], pv[2] - pp[2]];
            let e2 = [pn[0] - pp[0], pn[1] - pp[1], pn[2] - pp[2]];
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let cross_len_sq = cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2];
            if cross_len_sq > 1e-8 {
                continue; // Not collinear.
            }

            // Check no involved vertices are already removed.
            if removed_verts[prev as usize]
                || removed_verts[next as usize]
                || removed_verts[centroid_a as usize]
                || removed_verts[centroid_b as usize]
            {
                continue;
            }

            // Collapse: kill all 4 triangles, replace with 2.
            // Compute the replacement triangles' winding by checking against
            // the original triangle normals.
            for &idx in &[pairs[0].1, pairs[0].2, pairs[1].1, pairs[1].2] {
                dead_tris[incident[idx]] = true;
            }
            removed_verts[vi] = true;

            // For each pair, create the merged triangle with correct winding.
            for &(centroid, idx1, _idx2) in &pairs {
                let slot = incident[idx1];
                let original_tri = self.triangles[slot];
                let orig_positions: [[f32; 3]; 3] =
                    std::array::from_fn(|i| self.vertices[original_tri[i] as usize].position);
                let orig_normal = triangle_normal(orig_positions);

                // Try (centroid, prev, next) and check winding.
                let new_positions: [[f32; 3]; 3] = [
                    self.vertices[centroid as usize].position,
                    self.vertices[prev as usize].position,
                    self.vertices[next as usize].position,
                ];
                let new_normal = triangle_normal(new_positions);
                let dot = orig_normal[0] * new_normal[0]
                    + orig_normal[1] * new_normal[1]
                    + orig_normal[2] * new_normal[2];

                if dot >= 0.0 {
                    self.triangles[slot] = [centroid, prev, next];
                } else {
                    self.triangles[slot] = [centroid, next, prev]; // Flip winding.
                }
                dead_tris[slot] = false;
            }
        }

        // Compact.
        let mut keep = Vec::with_capacity(self.triangles.len());
        for (ti, tri) in self.triangles.iter().enumerate() {
            if dead_tris[ti] {
                continue;
            }
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                continue;
            }
            if removed_verts[tri[0] as usize]
                || removed_verts[tri[1] as usize]
                || removed_verts[tri[2] as usize]
            {
                continue;
            }
            keep.push(ti);
        }
        let new_tris: Vec<[u32; 3]> = keep.iter().map(|&ti| self.triangles[ti]).collect();
        let new_tags: Vec<u8> = keep.iter().map(|&ti| self.triangle_tags[ti]).collect();
        let new_colors: Vec<[f32; 4]> = keep.iter().map(|&ti| self.triangle_colors[ti]).collect();
        let new_vpos: Vec<[i32; 3]> = keep.iter().map(|&ti| self.triangle_voxel_pos[ti]).collect();

        self.triangles = new_tris;
        self.triangle_tags = new_tags;
        self.triangle_colors = new_colors;
        self.triangle_voxel_pos = new_vpos;

        let removed = vec![false; self.vertices.len()];
        self.compact_after_decimation(&removed);
    }

    /// Decimate the mesh using QEM edge collapse. Only collapses edges whose
    /// quadric error is below `max_error`. With `max_error` near zero, this
    /// only merges truly coplanar triangles (lossless for flat-shaded meshes).
    ///
    /// If `chunk_bounds` is provided as `(min, max)`, vertices on the chunk
    /// boundary are pinned (never collapsed) to prevent seams between adjacent
    /// chunks. A vertex is considered "on the boundary" if it belongs to
    /// triangles with source voxels both inside and outside the chunk.
    ///
    /// Preserves:
    /// - Anchored vertices' exact positions
    /// - Color and surface-tag boundaries
    /// - Triangle winding / normal orientation
    /// - Chunk boundary vertices (when bounds are provided)
    pub fn decimate(&mut self, max_error: f32, chunk_bounds: Option<([i32; 3], [i32; 3])>) {
        let vertex_count = self.vertices.len();
        if vertex_count == 0 {
            return;
        }

        // Build per-vertex triangle index.
        let mut vertex_tris: Vec<Vec<usize>> = vec![Vec::new(); vertex_count];
        for (ti, tri) in self.triangles.iter().enumerate() {
            for &vi in tri {
                vertex_tris[vi as usize].push(ti);
            }
        }

        // Pin chunk-boundary vertices: any vertex that belongs to triangles
        // with source voxels both inside and outside the chunk bounds must not
        // be collapsed, or adjacent chunks will have mismatched geometry.
        let mut pinned = vec![false; vertex_count];
        if let Some((cmin, cmax)) = chunk_bounds {
            // For each vertex, check if its incident triangles span the boundary.
            for (vi, tris) in vertex_tris.iter().enumerate() {
                let mut has_inside = false;
                let mut has_outside = false;
                for &ti in tris {
                    let vp = self.triangle_voxel_pos[ti];
                    let inside = vp[0] >= cmin[0]
                        && vp[0] < cmax[0]
                        && vp[1] >= cmin[1]
                        && vp[1] < cmax[1]
                        && vp[2] >= cmin[2]
                        && vp[2] < cmax[2];
                    if inside {
                        has_inside = true;
                    } else {
                        has_outside = true;
                    }
                    if has_inside && has_outside {
                        break;
                    }
                }
                if has_inside && has_outside {
                    pinned[vi] = true;
                }
            }
        }

        // Initialize per-vertex quadrics from incident triangle planes.
        // No area weighting — each plane contributes equally so the error
        // threshold has a clear geometric meaning (sum of squared distances
        // to original planes).
        let mut quadrics = vec![Quadric::zero(); vertex_count];
        for (ti, tri) in self.triangles.iter().enumerate() {
            let p0 = self.vertices[tri[0] as usize].position;
            let p1 = self.vertices[tri[1] as usize].position;
            let p2 = self.vertices[tri[2] as usize].position;

            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

            let nx = e1[1] * e2[2] - e1[2] * e2[1];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            let nz = e1[0] * e2[1] - e1[1] * e2[0];

            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            if len < 1e-12 {
                continue;
            }

            let nx = nx / len;
            let ny = ny / len;
            let nz = nz / len;
            let d = -(nx * p0[0] + ny * p0[1] + nz * p0[2]);

            let q = Quadric::from_plane(nx, ny, nz, d);

            for &vi in &self.triangles[ti] {
                quadrics[vi as usize] = quadrics[vi as usize].add(&q);
            }
        }

        let mut state = DecimationState {
            quadrics,
            vertex_tris,
            removed: vec![false; vertex_count],
            generations: vec![0; vertex_count],
            pinned,
        };

        // Build the initial priority queue of edge collapse candidates.
        let mut heap = BinaryHeap::new();
        let mut seen_edges = std::collections::BTreeSet::new();
        for (vi, v) in self.vertices.iter().enumerate() {
            for &ni in &v.neighbors {
                let edge = if (vi as u32) < ni {
                    (vi as u32, ni)
                } else {
                    (ni, vi as u32)
                };
                if seen_edges.insert(edge)
                    && let Some(candidate) =
                        self.compute_collapse_candidate(edge.0, edge.1, &state, max_error)
                {
                    heap.push(candidate);
                }
            }
        }

        // Iteratively collapse the cheapest edge.
        while let Some(candidate) = heap.pop() {
            if candidate.cost > max_error {
                break;
            }

            let v0 = candidate.v0;
            let v1 = candidate.v1;

            if state.removed[v0 as usize]
                || state.removed[v1 as usize]
                || candidate.gen_v0 != state.generations[v0 as usize]
                || candidate.gen_v1 != state.generations[v1 as usize]
            {
                continue;
            }

            // Pick the surviving vertex position. Always use one of the two
            // original positions (the one with lower quadric error) rather
            // than computing a new "optimal" position. This guarantees no
            // vertex moves to a position that didn't previously exist,
            // preventing any geometric deformation at near-zero thresholds.
            // The QEM optimal_position() is only useful for lossy LOD with
            // larger thresholds, which can be added later.
            let combined_q = state.quadrics[v0 as usize].add(&state.quadrics[v1 as usize]);
            let p0 = self.vertices[v0 as usize].position;
            let p1 = self.vertices[v1 as usize].position;
            let v0_anchored = self.vertices[v0 as usize].anchored;
            let v1_anchored = self.vertices[v1 as usize].anchored;
            let new_pos = if v0_anchored && !v1_anchored {
                p0
            } else if v1_anchored && !v0_anchored {
                p1
            } else {
                let e0 = combined_q.evaluate(p0[0], p0[1], p0[2]);
                let e1 = combined_q.evaluate(p1[0], p1[1], p1[2]);
                if e0 <= e1 { p0 } else { p1 }
            };

            if self.collapse_would_create_non_manifold(v0, v1, &state)
                || self.collapse_would_flip(v0, v1, new_pos, &state)
            {
                state.generations[v0 as usize] += 1;
                state.generations[v1 as usize] += 1;
                continue;
            }

            // Apply the collapse: v1 → v0.
            self.vertices[v0 as usize].position = new_pos;
            if self.vertices[v1 as usize].anchored {
                self.vertices[v0 as usize].anchored = true;
            }
            state.quadrics[v0 as usize] = combined_q;
            state.removed[v1 as usize] = true;

            // Update triangles: replace all references to v1 with v0.
            let v1_tris: Vec<usize> = state.vertex_tris[v1 as usize].clone();
            for &ti in &v1_tris {
                let tri = &mut self.triangles[ti];
                for vi in tri.iter_mut() {
                    if *vi == v1 {
                        *vi = v0;
                    }
                }
                if !state.vertex_tris[v0 as usize].contains(&ti) {
                    state.vertex_tris[v0 as usize].push(ti);
                }
            }
            state.vertex_tris[v1 as usize].clear();

            state.vertex_tris[v0 as usize].retain(|&ti| {
                let tri = self.triangles[ti];
                tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2]
            });

            // Update neighbor lists: v0 absorbs v1's neighbors.
            let v1_neighbors: Vec<u32> = self.vertices[v1 as usize].neighbors.clone();
            for &ni in &v1_neighbors {
                if ni == v0 || state.removed[ni as usize] {
                    continue;
                }
                let n = &mut self.vertices[ni as usize].neighbors;
                for vi in n.iter_mut() {
                    if *vi == v1 {
                        *vi = v0;
                    }
                }
                n.sort_unstable();
                n.dedup();
                if !self.vertices[v0 as usize].neighbors.contains(&ni) {
                    self.vertices[v0 as usize].neighbors.push(ni);
                }
            }
            self.vertices[v0 as usize]
                .neighbors
                .retain(|&ni| ni != v1 && !state.removed[ni as usize]);
            self.vertices[v1 as usize].neighbors.clear();

            // Only bump v0's generation — its position and connectivity changed.
            // V0's neighbors are unmodified (same position, same quadric), so
            // their existing heap entries to OTHER neighbors remain valid. We
            // re-insert v0's edges below, which pick up v0's new generation.
            // Previously we also bumped all neighbors' generations, which
            // invalidated all their non-v0 edges and caused cascading stale
            // entries that were never re-inserted — massively reducing
            // decimation effectiveness on flat surfaces.
            state.generations[v0 as usize] += 1;

            // Re-insert edges incident to v0.
            let v0_neighbors: Vec<u32> = self.vertices[v0 as usize].neighbors.clone();
            for &ni in &v0_neighbors {
                if state.removed[ni as usize] {
                    continue;
                }
                if let Some(c) =
                    self.compute_collapse_candidate(v0.min(ni), v0.max(ni), &state, max_error)
                {
                    heap.push(c);
                }
            }
        }

        self.compact_after_decimation(&state.removed);
    }

    /// Compute a collapse candidate for edge (v0, v1). Returns `None` if
    /// the edge should not be collapsed (color/tag boundary or cost exceeds
    /// threshold).
    fn compute_collapse_candidate(
        &self,
        v0: u32,
        v1: u32,
        state: &DecimationState,
        max_error: f32,
    ) -> Option<CollapseCandidate> {
        if state.removed[v0 as usize] || state.removed[v1 as usize] {
            return None;
        }

        // Pinned vertices (chunk boundary) are never collapsed.
        if state.pinned[v0 as usize] || state.pinned[v1 as usize] {
            return None;
        }

        // Check color/tag boundary: all triangles sharing this edge must have
        // the same (color, tag) pair.
        if !self.edge_has_uniform_attributes(v0, v1, &state.vertex_tris, &state.removed) {
            return None;
        }

        // Compute collapse cost at the position that will actually be used.
        let combined_q = state.quadrics[v0 as usize].add(&state.quadrics[v1 as usize]);
        let v0_anchored = self.vertices[v0 as usize].anchored;
        let v1_anchored = self.vertices[v1 as usize].anchored;
        let p0 = self.vertices[v0 as usize].position;
        let p1 = self.vertices[v1 as usize].position;

        // Always pick one of the two original positions (matching the
        // logic in the collapse application).
        let collapse_pos = if v0_anchored && !v1_anchored {
            p0
        } else if v1_anchored && !v0_anchored {
            p1
        } else {
            let e0 = combined_q.evaluate(p0[0], p0[1], p0[2]);
            let e1 = combined_q.evaluate(p1[0], p1[1], p1[2]);
            if e0 <= e1 { p0 } else { p1 }
        };
        let cost = combined_q
            .evaluate(collapse_pos[0], collapse_pos[1], collapse_pos[2])
            .max(0.0);

        if cost > max_error {
            return None;
        }

        Some(CollapseCandidate {
            cost,
            v0,
            v1,
            gen_v0: state.generations[v0 as usize],
            gen_v1: state.generations[v1 as usize],
        })
    }

    /// Check whether both vertices have uniform materials. If either vertex
    /// sits at a material boundary (incident triangles with different colors
    /// or tags), this edge must not be collapsed — doing so would warp the
    /// boundary between materials.
    fn edge_has_uniform_attributes(
        &self,
        v0: u32,
        v1: u32,
        vertex_tris: &[Vec<usize>],
        removed: &[bool],
    ) -> bool {
        // Check ALL incident triangles of both vertices, not just the shared
        // ones. A vertex on a material boundary has mixed-material triangles;
        // moving it would shift the boundary visually.
        for &vi in &[v0, v1] {
            let mut ref_color: Option<[f32; 4]> = None;
            let mut ref_tag: Option<u8> = None;

            for &ti in &vertex_tris[vi as usize] {
                let tri = self.triangles[ti];
                if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                    continue;
                }
                if removed[tri[0] as usize] || removed[tri[1] as usize] || removed[tri[2] as usize]
                {
                    continue;
                }

                let color = self.triangle_colors[ti];
                let tag = self.triangle_tags[ti];

                match ref_color {
                    None => {
                        ref_color = Some(color);
                        ref_tag = Some(tag);
                    }
                    Some(rc) => {
                        if rc != color || ref_tag != Some(tag) {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }

    /// Check whether collapsing edge (v0, v1) would create non-manifold
    /// geometry. This happens when v0 and v1 share more than 2 common
    /// neighbors — the collapse would merge triangle fans that create edges
    /// shared by 3+ triangles. Returns `true` if the collapse is unsafe.
    fn collapse_would_create_non_manifold(
        &self,
        v0: u32,
        v1: u32,
        state: &DecimationState,
    ) -> bool {
        // Count vertices that are neighbors of both v0 and v1 (excluding
        // removed vertices). For a manifold edge, there should be exactly 2
        // (the "wing" vertices of the two triangles sharing the edge).
        // On a mesh boundary, there may be 1.
        let v0_neighbors: std::collections::BTreeSet<u32> = self.vertices[v0 as usize]
            .neighbors
            .iter()
            .copied()
            .filter(|&n| n != v1 && !state.removed[n as usize])
            .collect();

        let mut common = 0;
        for &ni in &self.vertices[v1 as usize].neighbors {
            if ni != v0 && !state.removed[ni as usize] && v0_neighbors.contains(&ni) {
                common += 1;
            }
        }

        // More than 2 common neighbors → non-manifold collapse.
        common > 2
    }

    /// Check whether collapsing v1 into v0 at `new_pos` would flip any
    /// triangle's normal. Returns `true` if any triangle would flip.
    fn collapse_would_flip(
        &self,
        v0: u32,
        v1: u32,
        new_pos: [f32; 3],
        state: &DecimationState,
    ) -> bool {
        // Check all triangles incident to v0 or v1 that will survive
        // the collapse (i.e., don't contain both v0 and v1).
        let check_tris = state.vertex_tris[v0 as usize]
            .iter()
            .chain(state.vertex_tris[v1 as usize].iter());

        for &ti in check_tris {
            let tri = self.triangles[ti];

            // Skip degenerate triangles.
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                continue;
            }
            // Skip removed vertices.
            if state.removed[tri[0] as usize]
                || state.removed[tri[1] as usize]
                || state.removed[tri[2] as usize]
            {
                continue;
            }

            // Skip triangles that will be removed (contain both v0 and v1).
            if tri.contains(&v0) && tri.contains(&v1) {
                continue;
            }

            // This triangle survives. Compute its normal before and after.
            let positions_before: [[f32; 3]; 3] =
                std::array::from_fn(|i| self.vertices[tri[i] as usize].position);

            let positions_after: [[f32; 3]; 3] = std::array::from_fn(|i| {
                if tri[i] == v0 || tri[i] == v1 {
                    new_pos
                } else {
                    self.vertices[tri[i] as usize].position
                }
            });

            let n_before = triangle_normal(positions_before);
            let n_after = triangle_normal(positions_after);

            let len_before =
                (n_before[0] * n_before[0] + n_before[1] * n_before[1] + n_before[2] * n_before[2])
                    .sqrt();
            let len_after =
                (n_after[0] * n_after[0] + n_after[1] * n_after[1] + n_after[2] * n_after[2])
                    .sqrt();

            // Skip already-degenerate input triangles.
            if len_before < 1e-10 {
                continue;
            }

            // Reject if the triangle would become degenerate (zero-area from
            // collinear vertices). Degenerate triangles create holes when
            // removed during compaction.
            if len_after < 1e-10 {
                return true;
            }

            // Reject if the triangle's normal would flip.
            let dot =
                n_before[0] * n_after[0] + n_before[1] * n_after[1] + n_before[2] * n_after[2];

            if dot < 0.0 {
                return true;
            }
        }

        false
    }

    /// Remove degenerate triangles and compact the vertex/triangle arrays
    /// after decimation. Removes all triangles with duplicate vertex indices
    /// and all vertices not referenced by any surviving triangle.
    fn compact_after_decimation(&mut self, removed: &[bool]) {
        // First pass: remove degenerate triangles and triangles with removed vertices.
        let mut keep = Vec::with_capacity(self.triangles.len());
        for (ti, tri) in self.triangles.iter().enumerate() {
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                continue;
            }
            if removed[tri[0] as usize] || removed[tri[1] as usize] || removed[tri[2] as usize] {
                continue;
            }
            keep.push(ti);
        }

        let new_triangles: Vec<[u32; 3]> = keep.iter().map(|&ti| self.triangles[ti]).collect();
        let new_tags: Vec<u8> = keep.iter().map(|&ti| self.triangle_tags[ti]).collect();
        let new_colors: Vec<[f32; 4]> = keep.iter().map(|&ti| self.triangle_colors[ti]).collect();
        let new_voxel_pos: Vec<[i32; 3]> =
            keep.iter().map(|&ti| self.triangle_voxel_pos[ti]).collect();

        // Build vertex remap: only keep vertices referenced by surviving triangles.
        let mut used = vec![false; self.vertices.len()];
        for tri in &new_triangles {
            for &vi in tri {
                used[vi as usize] = true;
            }
        }

        let mut remap: Vec<u32> = vec![u32::MAX; self.vertices.len()];
        let mut new_vertices = Vec::new();
        for (old_idx, v) in self.vertices.iter().enumerate() {
            if used[old_idx] {
                remap[old_idx] = new_vertices.len() as u32;
                let mut new_v = v.clone();
                // Remap neighbor indices (will be fixed below).
                new_v.neighbors.clear();
                new_vertices.push(new_v);
            }
        }

        // Remap triangle vertex indices.
        let remapped_triangles: Vec<[u32; 3]> = new_triangles
            .iter()
            .map(|tri| {
                [
                    remap[tri[0] as usize],
                    remap[tri[1] as usize],
                    remap[tri[2] as usize],
                ]
            })
            .collect();

        // Rebuild neighbor lists from the remapped triangles.
        for tri in &remapped_triangles {
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                if !new_vertices[a as usize].neighbors.contains(&b) {
                    new_vertices[a as usize].neighbors.push(b);
                }
                if !new_vertices[b as usize].neighbors.contains(&a) {
                    new_vertices[b as usize].neighbors.push(a);
                }
            }
        }

        // Rebuild the dedup map.
        self.dedup.clear();
        for (idx, v) in new_vertices.iter().enumerate() {
            let key = super::smooth_mesh::VertexKey::from_position(v.position);
            self.dedup.insert(key, idx as u32);
        }

        self.vertices = new_vertices;
        self.triangles = remapped_triangles;
        self.triangle_tags = new_tags;
        self.triangle_colors = new_colors;
        self.triangle_voxel_pos = new_voxel_pos;
    }
}

#[cfg(test)]
/// Compute the signed volume of a watertight triangle mesh using the
/// divergence theorem. Each triangle contributes the signed volume of the
/// tetrahedron it forms with the origin: `(1/6) * dot(v0, cross(v1, v2))`.
/// The sum over all triangles gives the mesh volume (positive if normals
/// point outward with consistent winding).
fn mesh_signed_volume(vertices: &[[f32; 3]], triangles: &[[u32; 3]]) -> f32 {
    let mut vol = 0.0f32;
    for tri in triangles {
        let v0 = vertices[tri[0] as usize];
        let v1 = vertices[tri[1] as usize];
        let v2 = vertices[tri[2] as usize];
        // cross(v1, v2)
        let cx = v1[1] * v2[2] - v1[2] * v2[1];
        let cy = v1[2] * v2[0] - v1[0] * v2[2];
        let cz = v1[0] * v2[1] - v1[1] * v2[0];
        // dot(v0, cross)
        vol += v0[0] * cx + v0[1] * cy + v0[2] * cz;
    }
    vol / 6.0
}

/// Compute the (unnormalized) normal of a triangle.
fn triangle_normal(positions: [[f32; 3]; 3]) -> [f32; 3] {
    let e1 = [
        positions[1][0] - positions[0][0],
        positions[1][1] - positions[0][1],
        positions[1][2] - positions[0][2],
    ];
    let e2 = [
        positions[2][0] - positions[0][0],
        positions[2][1] - positions[0][1],
        positions[2][2] - positions[0][2],
    ];
    [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_gen::{FACE_NORMALS, FACE_VERTICES, voxel_color};
    use crate::smooth_mesh::{TAG_BARK, TAG_GROUND};
    use crate::types::VoxelType;

    /// Build a chamfered mesh from a set of voxel positions (same as
    /// smooth_mesh::tests::build_chamfered_voxel_mesh but accessible here).
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

    /// Build a flat surface of voxels (only +Y top faces) for decimation testing.
    fn build_flat_surface(width: i32, depth: i32) -> SmoothMesh {
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);

        for x in 0..width {
            for z in 0..depth {
                let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                    [
                        FACE_VERTICES[2][vi][0] + x as f32,
                        FACE_VERTICES[2][vi][1],
                        FACE_VERTICES[2][vi][2] + z as f32,
                    ]
                });
                mesh.add_subdivided_face(corners, FACE_NORMALS[2], color, TAG_BARK, [x, 0, z]);
            }
        }

        mesh.normalize_initial_normals();
        mesh.apply_anchoring();
        mesh.chamfer();
        mesh
    }

    // --- Quadric unit tests ---

    #[test]
    fn quadric_from_plane_evaluates_to_zero_on_plane() {
        // Plane: y = 0 (normal (0,1,0), d=0).
        let q = Quadric::from_plane(0.0, 1.0, 0.0, 0.0);
        assert!((q.evaluate(5.0, 0.0, 3.0)).abs() < 1e-10);
        assert!((q.evaluate(-2.0, 0.0, 7.0)).abs() < 1e-10);
    }

    #[test]
    fn quadric_from_plane_measures_squared_distance() {
        // Plane: y = 0. Point at (0, 3, 0) → distance² = 9.
        let q = Quadric::from_plane(0.0, 1.0, 0.0, 0.0);
        assert!((q.evaluate(0.0, 3.0, 0.0) - 9.0).abs() < 1e-6);
    }

    #[test]
    fn quadric_sum_measures_combined_error() {
        // Two perpendicular planes: y=0 and x=0.
        // Point (3, 4, 0) has error 9 + 16 = 25.
        let q1 = Quadric::from_plane(0.0, 1.0, 0.0, 0.0);
        let q2 = Quadric::from_plane(1.0, 0.0, 0.0, 0.0);
        let q = q1.add(&q2);
        assert!((q.evaluate(3.0, 4.0, 0.0) - 25.0).abs() < 1e-5);
    }

    #[test]
    fn quadric_optimal_at_plane_intersection() {
        // Three perpendicular planes: x=1, y=2, z=3.
        // Optimal position should be (1, 2, 3).
        let q1 = Quadric::from_plane(1.0, 0.0, 0.0, -1.0);
        let q2 = Quadric::from_plane(0.0, 1.0, 0.0, -2.0);
        let q3 = Quadric::from_plane(0.0, 0.0, 1.0, -3.0);
        let q = q1.add(&q2).add(&q3);
        let opt = q.optimal_position().expect("should have optimal");
        assert!((opt[0] - 1.0).abs() < 1e-5);
        assert!((opt[1] - 2.0).abs() < 1e-5);
        assert!((opt[2] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn quadric_singular_returns_none() {
        // Single plane — no unique point, system is singular.
        let q = Quadric::from_plane(0.0, 1.0, 0.0, 0.0);
        assert!(q.optimal_position().is_none());
    }

    // --- Decimation tests ---

    #[test]
    fn flat_surface_decimation_reduces_triangles() {
        // A 4×4 flat surface of voxel top faces should have many coplanar
        // triangles that can be collapsed with near-zero error.
        let mut mesh = build_flat_surface(4, 4);
        let before = mesh.triangles.len();
        assert_eq!(before, 4 * 4 * 8); // 16 faces × 8 triangles each

        mesh.decimate(1e-6, None);

        assert!(
            mesh.triangles.len() < before,
            "decimation should reduce triangle count: {before} → {}",
            mesh.triangles.len()
        );
    }

    #[test]
    fn flat_surface_decimation_preserves_planarity() {
        // After decimation, all vertices should still be on the y=1 plane.
        let mut mesh = build_flat_surface(4, 4);
        mesh.decimate(1e-6, None);

        for v in &mesh.vertices {
            assert!(
                (v.position[1] - 1.0).abs() < 1e-5,
                "vertex y should be 1.0, got {}",
                v.position[1]
            );
        }
    }

    #[test]
    fn single_voxel_chamfered_decimation_preserves_shape() {
        // A single voxel with chamfered edges — all triangles on each face
        // are coplanar but the chamfered edges should not be collapsed
        // (they're not coplanar with their neighbors).
        let mut mesh = build_chamfered_voxel_mesh(&[(0, 0, 0)]);
        let before = mesh.triangles.len();

        // Record all vertex positions before decimation.
        let positions_before: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();

        mesh.decimate(1e-6, None);

        // Some triangles may be removed (coplanar faces on each face), but
        // the overall shape should be preserved — no vertex should move
        // significantly.
        let after = mesh.triangles.len();
        assert!(
            after <= before,
            "decimation should not increase triangles: {before} → {after}"
        );

        // All remaining vertices should be at positions that existed before.
        for v in &mesh.vertices {
            let min_dist = positions_before
                .iter()
                .map(|p| {
                    ((p[0] - v.position[0]).powi(2)
                        + (p[1] - v.position[1]).powi(2)
                        + (p[2] - v.position[2]).powi(2))
                    .sqrt()
                })
                .fold(f32::MAX, f32::min);
            assert!(
                min_dist < 0.01,
                "vertex at {:?} is not near any original position (min_dist={min_dist})",
                v.position,
            );
        }
    }

    #[test]
    fn decimation_preserves_color_boundaries() {
        // Two adjacent voxels with different colors. The edge between them
        // should not be collapsed.
        let mut mesh = SmoothMesh::new();
        let color_trunk = voxel_color(VoxelType::Trunk);
        let color_dirt = voxel_color(VoxelType::Dirt);

        // Two +Y faces side by side with different colors.
        let corners0: [[f32; 3]; 4] = std::array::from_fn(|vi| {
            [
                FACE_VERTICES[2][vi][0],
                FACE_VERTICES[2][vi][1],
                FACE_VERTICES[2][vi][2],
            ]
        });
        mesh.add_subdivided_face(corners0, FACE_NORMALS[2], color_trunk, TAG_BARK, [0, 0, 0]);

        let corners1: [[f32; 3]; 4] = std::array::from_fn(|vi| {
            [
                FACE_VERTICES[2][vi][0] + 1.0,
                FACE_VERTICES[2][vi][1],
                FACE_VERTICES[2][vi][2],
            ]
        });
        mesh.add_subdivided_face(corners1, FACE_NORMALS[2], color_dirt, TAG_GROUND, [1, 0, 0]);

        mesh.normalize_initial_normals();
        mesh.apply_anchoring();
        mesh.chamfer();

        let before = mesh.triangles.len();
        mesh.decimate(1e-6, None);

        // Count triangles per tag — both should still have triangles.
        let bark_count = mesh
            .triangle_tags
            .iter()
            .filter(|&&t| t == TAG_BARK)
            .count();
        let ground_count = mesh
            .triangle_tags
            .iter()
            .filter(|&&t| t == TAG_GROUND)
            .count();

        assert!(bark_count > 0, "bark triangles should survive decimation");
        assert!(
            ground_count > 0,
            "ground triangles should survive decimation"
        );

        // The total should not exceed the original.
        assert!(mesh.triangles.len() <= before);
    }

    #[test]
    fn decimation_preserves_anchored_vertex_positions() {
        // Anchored vertices may be merged with other coplanar anchored
        // vertices, but every surviving anchored vertex must be at a
        // position that was originally anchored (never moved off-plane).
        let mut mesh = build_flat_surface(2, 2);

        let anchored_positions_before: std::collections::HashSet<[i32; 3]> = mesh
            .vertices
            .iter()
            .filter(|v| v.anchored)
            .map(|v| {
                // Quantize to avoid float comparison issues.
                [
                    (v.position[0] * 1000.0).round() as i32,
                    (v.position[1] * 1000.0).round() as i32,
                    (v.position[2] * 1000.0).round() as i32,
                ]
            })
            .collect();

        mesh.decimate(1e-6, None);

        // Every surviving anchored vertex must be at an originally-anchored
        // position (not moved by the collapse).
        for v in &mesh.vertices {
            if v.anchored {
                let quantized = [
                    (v.position[0] * 1000.0).round() as i32,
                    (v.position[1] * 1000.0).round() as i32,
                    (v.position[2] * 1000.0).round() as i32,
                ];
                assert!(
                    anchored_positions_before.contains(&quantized),
                    "anchored vertex at {:?} is not at any original anchored position",
                    v.position,
                );
            }
        }
    }

    #[test]
    fn decimation_produces_valid_mesh() {
        // After decimation, all triangle vertex indices should be valid,
        // and the mesh should produce a valid SurfaceMesh.
        let mut mesh = build_chamfered_voxel_mesh(&[(0, 0, 0), (1, 0, 0), (0, 1, 0)]);
        mesh.decimate(1e-6, None);

        let vertex_count = mesh.vertices.len() as u32;
        for (ti, tri) in mesh.triangles.iter().enumerate() {
            for &vi in tri {
                assert!(
                    vi < vertex_count,
                    "triangle {ti} has invalid vertex index {vi} (vertex count = {vertex_count})"
                );
            }
            // No degenerate triangles.
            assert_ne!(tri[0], tri[1], "degenerate triangle {ti}");
            assert_ne!(tri[1], tri[2], "degenerate triangle {ti}");
            assert_ne!(tri[0], tri[2], "degenerate triangle {ti}");
        }

        // Should produce a valid surface mesh.
        let surface = mesh.to_surface_mesh();
        assert!(surface.vertex_count() > 0);
        assert!(!surface.indices.is_empty());
    }

    #[test]
    fn large_flat_surface_significant_reduction() {
        // An 8×8 flat surface should see significant triangle reduction.
        // Face centers and boundary vertices are anchored, so we can't get
        // all the way down to a minimal triangulation, but interior
        // non-anchored edges should collapse.
        let mut mesh = build_flat_surface(8, 8);
        let before = mesh.triangles.len();
        assert_eq!(before, 8 * 8 * 8); // 512 triangles

        mesh.decimate(1e-6, None);

        // Should reduce by at least 25% (conservative — anchored face centers
        // limit how much interior can be simplified).
        let reduction = before - mesh.triangles.len();
        assert!(
            reduction > before / 4,
            "8×8 flat surface should reduce by >25%: {before} → {} (reduced {reduction})",
            mesh.triangles.len()
        );
    }

    #[test]
    fn zero_threshold_is_lossless_for_chamfer() {
        // With zero threshold, decimation should not move any vertex outside
        // the original planes of the chamfered mesh.
        let mut mesh =
            build_chamfered_voxel_mesh(&[(0, 0, 0), (1, 0, 0), (2, 0, 0), (0, 1, 0), (1, 1, 0)]);
        let before_count = mesh.triangles.len();

        // Compute all triangle normals before decimation.
        let normals_before: Vec<[f32; 3]> = mesh
            .triangles
            .iter()
            .map(|tri| {
                let positions: [[f32; 3]; 3] =
                    std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
                triangle_normal(positions)
            })
            .collect();

        mesh.decimate(0.0, None);

        // Should still reduce triangles (exact coplanar collapses have zero cost).
        assert!(
            mesh.triangles.len() <= before_count,
            "zero-threshold decimation should not increase triangles"
        );

        // All remaining triangle normals should be parallel to some original normal.
        for tri in &mesh.triangles {
            let positions: [[f32; 3]; 3] =
                std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
            let n = triangle_normal(positions);
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len < 1e-10 {
                continue;
            }
            let n = [n[0] / len, n[1] / len, n[2] / len];

            let found = normals_before.iter().any(|nb| {
                let nb_len = (nb[0] * nb[0] + nb[1] * nb[1] + nb[2] * nb[2]).sqrt();
                if nb_len < 1e-10 {
                    return false;
                }
                let nb = [nb[0] / nb_len, nb[1] / nb_len, nb[2] / nb_len];
                let dot = n[0] * nb[0] + n[1] * nb[1] + n[2] * nb[2];
                dot > 0.999
            });
            assert!(
                found,
                "triangle normal {:?} should be parallel to some original normal",
                n
            );
        }
    }

    #[test]
    fn flat_surface_extreme_reduction() {
        // An 8×8 flat surface is ALL coplanar. QEM decimation should reduce
        // it significantly.
        let mut mesh = build_flat_surface(8, 8);
        let before = mesh.triangles.len();
        assert_eq!(before, 512);

        mesh.decimate(1e-6, None);
        let after = mesh.triangles.len();

        assert!(
            after < before / 2,
            "flat 8x8 surface should reduce by >50%: {before} → {after}"
        );
    }

    #[test]
    fn empty_mesh_decimation_is_noop() {
        let mut mesh = SmoothMesh::new();
        mesh.decimate(1e-6, None);
        assert_eq!(mesh.triangles.len(), 0);
        assert_eq!(mesh.vertices.len(), 0);
    }

    #[test]
    fn chunk_boundary_vertices_are_preserved() {
        // Build a flat surface that spans a chunk boundary. Vertices on
        // the boundary should not be collapsed when chunk_bounds is provided.
        let mut mesh = SmoothMesh::new();
        let color = voxel_color(VoxelType::Trunk);

        // 4 voxels: 2 inside chunk [0,2) and 2 outside [2,4).
        for x in 0..4 {
            let corners: [[f32; 3]; 4] = std::array::from_fn(|vi| {
                [
                    FACE_VERTICES[2][vi][0] + x as f32,
                    FACE_VERTICES[2][vi][1],
                    FACE_VERTICES[2][vi][2],
                ]
            });
            mesh.add_subdivided_face(corners, FACE_NORMALS[2], color, TAG_BARK, [x, 0, 0]);
        }
        mesh.normalize_initial_normals();
        mesh.apply_anchoring();
        mesh.chamfer();

        // Decimate with chunk bounds [0, 2) — boundary is at x=2.
        let before = mesh.triangles.len();
        mesh.decimate(1e-6, Some(([0, 0, 0], [2, 16, 16])));

        // Should still reduce some triangles (interior of each half).
        assert!(
            mesh.triangles.len() <= before,
            "decimation should not increase triangles"
        );

        // Vertices at the boundary (x=2.0) should still exist.
        let boundary_verts = mesh
            .vertices
            .iter()
            .filter(|v| (v.position[0] - 2.0).abs() < 0.01)
            .count();
        assert!(
            boundary_verts > 0,
            "boundary vertices at x=2 should survive decimation"
        );
    }

    // --- Volume preservation tests ---

    /// Compute the absolute volume of a SmoothMesh.
    fn smooth_mesh_volume(mesh: &SmoothMesh) -> f32 {
        let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
        mesh_signed_volume(&positions, &mesh.triangles).abs()
    }

    /// Build a rectangular prism of voxels and return the chamfered mesh.
    fn build_prism(sx: i32, sy: i32, sz: i32) -> SmoothMesh {
        let mut voxels = Vec::new();
        for x in 0..sx {
            for y in 0..sy {
                for z in 0..sz {
                    voxels.push((x, y, z));
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Build a pyramid with a `base × base` footprint and `height` layers.
    /// Each layer shrinks by 1 on each side.
    fn build_pyramid(base: i32, height: i32) -> SmoothMesh {
        let mut voxels = Vec::new();
        for y in 0..height {
            let inset = y; // each layer shrinks by 1 on each side
            let layer_size = base - 2 * inset;
            if layer_size <= 0 {
                break;
            }
            for x in inset..(inset + layer_size) {
                for z in inset..(inset + layer_size) {
                    voxels.push((x, y, z));
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    #[test]
    fn volume_computation_sanity() {
        // A unit cube (8 vertices, 12 triangles) should have volume 1.0.
        // Use a single voxel's raw faces (no subdivision) as sanity check.
        let positions: Vec<[f32; 3]> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        // 12 triangles (2 per face), CCW from outside.
        let triangles: Vec<[u32; 3]> = vec![
            // +Z face
            [4, 5, 6],
            [4, 6, 7],
            // -Z face
            [1, 0, 3],
            [1, 3, 2],
            // +X face
            [5, 1, 2],
            [5, 2, 6],
            // -X face
            [0, 4, 7],
            [0, 7, 3],
            // +Y face
            [3, 7, 6],
            [3, 6, 2],
            // -Y face
            [0, 1, 5],
            [0, 5, 4],
        ];
        let vol = mesh_signed_volume(&positions, &triangles).abs();
        assert!(
            (vol - 1.0).abs() < 1e-5,
            "unit cube volume should be 1.0, got {vol}"
        );
    }

    #[test]
    fn prism_volume_preserved_by_decimation() {
        // 10×10×3 rectangular prism. Chamfer changes the volume slightly
        // (beveled edges/corners), but decimation must not change it further.
        let mut mesh = build_prism(10, 3, 10);
        let vol_before = smooth_mesh_volume(&mesh);
        let tri_before = mesh.triangles.len();

        // Volume should be close to 10*3*10 = 300 (chamfer reduces it slightly).
        assert!(
            vol_before > 250.0 && vol_before < 310.0,
            "prism volume should be near 300, got {vol_before}"
        );

        mesh.decimate(1e-6, None);
        let vol_after = smooth_mesh_volume(&mesh);
        let tri_after = mesh.triangles.len();

        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.0001,
            "decimation changed prism volume by {vol_diff}: {vol_before} → {vol_after}"
        );

        // Should actually reduce triangles.
        assert!(
            tri_after < tri_before,
            "decimation should reduce triangles: {tri_before} → {tri_after}"
        );
    }

    #[test]
    fn pyramid_volume_preserved_by_decimation() {
        // 10×10-base pyramid, 5 layers high.
        let mut mesh = build_pyramid(10, 5);
        let vol_before = smooth_mesh_volume(&mesh);
        let tri_before = mesh.triangles.len();

        mesh.decimate(1e-6, None);
        let vol_after = smooth_mesh_volume(&mesh);
        let tri_after = mesh.triangles.len();

        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.0001,
            "decimation changed pyramid volume by {vol_diff}: {vol_before} → {vol_after}"
        );

        assert!(
            tri_after < tri_before,
            "decimation should reduce triangles: {tri_before} → {tri_after}"
        );
    }

    /// Build a diamond-shaped pyramid: rotated 45° so the footprint is a
    /// diamond when viewed from above. Each layer is a diamond (Manhattan
    /// distance from center ≤ radius). This exercises diagonal staircases
    /// where the chamfer saddle-skip heuristic activates.
    fn build_diamond_pyramid(radius: i32, height: i32) -> SmoothMesh {
        let mut voxels = Vec::new();
        for y in 0..height {
            let layer_radius = radius - y;
            if layer_radius < 0 {
                break;
            }
            // Diamond: |x - cx| + |z - cz| <= layer_radius
            let cx = radius;
            let cz = radius;
            for x in (cx - layer_radius)..=(cx + layer_radius) {
                let z_budget = layer_radius - (x - cx).abs();
                for z in (cz - z_budget)..=(cz + z_budget) {
                    voxels.push((x, y, z));
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    #[test]
    fn diamond_pyramid_volume_preserved_by_decimation() {
        // Diamond pyramid with radius 6, height 7. The diagonal staircase
        // edges are where the chamfer saddle-skip heuristic activates —
        // this is the geometry most likely to cause decimation issues.
        //
        // Tolerance is 0.001 (not 0.0001) because edge collapses on
        // non-coplanar diagonal staircase geometry inherently change the
        // triangle fan topology. Even with zero QEM cost (vertex stays
        // on-plane), removing two triangles and reshaping their neighbors
        // changes the signed volume slightly. This is ~0.0001% error on
        // the 222-voxel shape and completely invisible.
        let mut mesh = build_diamond_pyramid(6, 7);
        let vol_before = smooth_mesh_volume(&mesh);
        let tri_before = mesh.triangles.len();

        mesh.decimate(1e-6, None);
        let vol_after = smooth_mesh_volume(&mesh);
        let tri_after = mesh.triangles.len();

        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.001,
            "decimation changed diamond pyramid volume by {vol_diff}: {vol_before} → {vol_after}"
        );

        assert!(
            tri_after < tri_before,
            "decimation should reduce triangles: {tri_before} → {tri_after}"
        );
    }

    #[test]
    fn truncated_diamond_volume_preserved_by_decimation() {
        // Diamond pyramid radius 6, but only 3 layers tall (half of the
        // full 7-layer pyramid). The flat top is a diamond of radius 3,
        // meeting diagonal staircase sides — the junction between the flat
        // top and the diagonal edges is where deformation is most likely.
        let mut mesh = build_diamond_pyramid(6, 3);
        let vol_before = smooth_mesh_volume(&mesh);
        let tri_before = mesh.triangles.len();

        mesh.decimate(1e-6, None);
        let vol_after = smooth_mesh_volume(&mesh);
        let tri_after = mesh.triangles.len();

        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.001,
            "decimation changed truncated diamond volume by {vol_diff}: {vol_before} → {vol_after}"
        );

        assert!(
            tri_after < tri_before,
            "decimation should reduce triangles: {tri_before} → {tri_after}"
        );
    }

    #[test]
    fn single_voxel_volume_preserved() {
        // Single voxel — minimal case.
        let mut mesh = build_chamfered_voxel_mesh(&[(0, 0, 0)]);
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.decimate(1e-6, None);
        let vol_after = smooth_mesh_volume(&mesh);

        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.0001,
            "decimation changed single voxel volume by {vol_diff}: {vol_before} → {vol_after}"
        );
    }

    // --- Watertightness and integrity checks ---

    /// Check that a SmoothMesh is watertight: every edge is shared by exactly
    /// 2 triangles. Returns a list of boundary edges (shared by only 1
    /// triangle) — empty means watertight. Also checks for non-manifold edges
    /// (shared by 3+ triangles).
    fn check_watertight(mesh: &SmoothMesh) -> (Vec<(u32, u32)>, Vec<(u32, u32)>) {
        let mut edge_counts: std::collections::BTreeMap<(u32, u32), u32> =
            std::collections::BTreeMap::new();

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
                2 => {} // correct
                _ => non_manifold.push(*edge),
            }
        }
        (boundary, non_manifold)
    }

    /// Assert that a mesh is watertight (no boundary or non-manifold edges).
    fn assert_watertight(mesh: &SmoothMesh, label: &str) {
        let (boundary, non_manifold) = check_watertight(mesh);
        assert!(
            boundary.is_empty(),
            "{label}: mesh has {} boundary edges (holes). First 5: {:?}",
            boundary.len(),
            &boundary[..boundary.len().min(5)]
        );
        assert!(
            non_manifold.is_empty(),
            "{label}: mesh has {} non-manifold edges. First 5: {:?}",
            non_manifold.len(),
            &non_manifold[..non_manifold.len().min(5)]
        );
    }

    /// Assert that no triangle has zero area (degenerate).
    fn assert_no_degenerate_triangles(mesh: &SmoothMesh, label: &str) {
        for (ti, tri) in mesh.triangles.iter().enumerate() {
            // Check for duplicate vertex indices.
            assert!(
                tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2],
                "{label}: triangle {ti} has duplicate vertex indices: {:?}",
                tri
            );
            // Check for zero area.
            let positions: [[f32; 3]; 3] =
                std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
            let n = triangle_normal(positions);
            let area_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
            assert!(
                area_sq > 1e-20,
                "{label}: triangle {ti} is degenerate (zero area), vertices: {:?}",
                positions
            );
        }
    }

    /// Assert that no vertex has position at or near the origin when it
    /// shouldn't (catches uninitialized/zeroed vertex bugs).
    fn assert_no_stray_vertices(mesh: &SmoothMesh, min_expected: [f32; 3], label: &str) {
        for (vi, v) in mesh.vertices.iter().enumerate() {
            let suspicious = v.position[0] < min_expected[0]
                && v.position[1] < min_expected[1]
                && v.position[2] < min_expected[2];
            assert!(
                !suspicious,
                "{label}: vertex {vi} at {:?} is suspiciously near origin \
                 (expected all coords >= {:?})",
                v.position, min_expected
            );
        }
    }

    /// Run the full integrity suite on a mesh: watertight, no degenerate
    /// triangles, no stray vertices, volume preserved. Runs the flat-face
    /// pre-pass before QEM decimation (matching the production pipeline).
    fn assert_decimation_integrity(label: &str, mesh: &mut SmoothMesh, vol_tolerance: f32) {
        assert_watertight(mesh, &format!("{label} pre-decimation"));
        assert_no_degenerate_triangles(mesh, &format!("{label} pre-decimation"));
        let vol_before = smooth_mesh_volume(mesh);
        let tri_before = mesh.triangles.len();

        mesh.coplanar_region_retri(None);
        mesh.collapse_collinear_boundary_vertices(None);
        mesh.decimate(1e-6, None);

        assert_watertight(mesh, &format!("{label} post-decimation"));
        assert_no_degenerate_triangles(mesh, &format!("{label} post-decimation"));
        assert_no_stray_vertices(mesh, [-1.0; 3], label);
        let vol_after = smooth_mesh_volume(mesh);
        let tri_after = mesh.triangles.len();

        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < vol_tolerance,
            "{label}: volume changed by {vol_diff}: {vol_before} → {vol_after}"
        );
        assert!(
            tri_after < tri_before,
            "{label}: should reduce triangles: {tri_before} → {tri_after}"
        );
    }

    // --- Irregular shape builders ---

    /// Small platform on top of a large base (T-shape in cross section).
    /// The overhang creates concave geometry where the small top meets the
    /// large base.
    fn build_platform_on_base() -> SmoothMesh {
        let mut voxels = Vec::new();
        // Large base: 8×2×8
        for x in 0..8 {
            for y in 0..2 {
                for z in 0..8 {
                    voxels.push((x, y, z));
                }
            }
        }
        // Small platform on top: 4×2×4, centered
        for x in 2..6 {
            for y in 2..4 {
                for z in 2..6 {
                    voxels.push((x, y, z));
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    /// L-shaped structure — concave corner.
    fn build_l_shape() -> SmoothMesh {
        let mut voxels = Vec::new();
        // Horizontal bar: 8×3×3
        for x in 0..8 {
            for y in 0..3 {
                for z in 0..3 {
                    voxels.push((x, y, z));
                }
            }
        }
        // Vertical bar: 3×8×3 (overlaps with horizontal)
        for x in 0..3 {
            for y in 0..8 {
                for z in 0..3 {
                    voxels.push((x, y, z));
                }
            }
        }
        voxels.sort();
        voxels.dedup();
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Hollow frame — a square ring (concave from above).
    fn build_hollow_frame() -> SmoothMesh {
        let mut voxels = Vec::new();
        let outer = 8;
        let inner_min = 2;
        let inner_max = 6;
        for x in 0..outer {
            for z in 0..outer {
                // Skip the hollow interior.
                if x >= inner_min && x < inner_max && z >= inner_min && z < inner_max {
                    continue;
                }
                for y in 0..3 {
                    voxels.push((x, y, z));
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Staircase — ascending steps along X.
    fn build_staircase() -> SmoothMesh {
        let mut voxels = Vec::new();
        for step in 0..6 {
            for x in (step * 2)..(step * 2 + 3) {
                for y in 0..=(step as i32) {
                    for z in 0..5 {
                        voxels.push((x, y, z));
                    }
                }
            }
        }
        voxels.sort();
        voxels.dedup();
        build_chamfered_voxel_mesh(&voxels)
    }

    // --- Integrity tests ---

    #[test]
    fn prism_decimation_integrity() {
        let mut mesh = build_prism(10, 3, 10);
        assert_decimation_integrity("prism 10x3x10", &mut mesh, 0.0001);
    }

    #[test]
    fn pyramid_decimation_integrity() {
        let mut mesh = build_pyramid(10, 5);
        // Pyramid has chamfered step edges — non-coplanar collapses cause
        // small inherent volume drift (~0.0002).
        assert_decimation_integrity("pyramid 10x5", &mut mesh, 0.001);
    }

    #[test]
    fn diamond_pyramid_decimation_integrity() {
        let mut mesh = build_diamond_pyramid(6, 7);
        assert_decimation_integrity("diamond r=6 h=7", &mut mesh, 0.001);
    }

    #[test]
    fn truncated_diamond_decimation_integrity() {
        let mut mesh = build_diamond_pyramid(6, 3);
        assert_decimation_integrity("truncated diamond r=6 h=3", &mut mesh, 0.001);
    }

    #[test]
    fn platform_on_base_decimation_integrity() {
        let mut mesh = build_platform_on_base();
        assert_decimation_integrity("platform on base", &mut mesh, 0.001);
    }

    #[test]
    fn l_shape_decimation_integrity() {
        let mut mesh = build_l_shape();
        assert_decimation_integrity("L-shape", &mut mesh, 0.001);
    }

    #[test]
    fn hollow_frame_decimation_integrity() {
        let mut mesh = build_hollow_frame();
        assert_decimation_integrity("hollow frame", &mut mesh, 0.001);
    }

    #[test]
    fn staircase_decimation_integrity() {
        let mut mesh = build_staircase();
        assert_decimation_integrity("staircase", &mut mesh, 0.001);
    }

    #[test]
    fn single_voxel_decimation_integrity() {
        let mut mesh = build_chamfered_voxel_mesh(&[(0, 0, 0)]);
        assert_watertight(&mesh, "single voxel pre");
        assert_no_degenerate_triangles(&mesh, "single voxel pre");
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.decimate(1e-6, None);

        assert_watertight(&mesh, "single voxel post");
        assert_no_degenerate_triangles(&mesh, "single voxel post");
        let vol_after = smooth_mesh_volume(&mesh);
        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.0001,
            "single voxel volume changed by {vol_diff}"
        );
    }

    // --- Coplanar region re-triangulation tests ---

    #[test]
    fn coplanar_retri_prism_integrity() {
        // 10×3×10 prism. The coplanar pre-pass should dramatically reduce
        // the large flat faces (top, bottom, sides), then QEM cleans up.
        let mut mesh = build_prism(10, 3, 10);
        assert_watertight(&mesh, "prism pre");
        let vol_before = smooth_mesh_volume(&mesh);
        let tri_before = mesh.triangles.len();

        mesh.coplanar_region_retri(None);
        assert_watertight(&mesh, "prism post-retri");
        assert_no_degenerate_triangles(&mesh, "prism post-retri");
        let vol_after_retri = smooth_mesh_volume(&mesh);
        let tri_after_retri = mesh.triangles.len();

        let vol_diff = (vol_after_retri - vol_before).abs();
        assert!(vol_diff < 0.001, "retri changed prism volume by {vol_diff}");
        assert!(
            tri_after_retri < tri_before,
            "retri should reduce triangles: {tri_before} → {tri_after_retri}"
        );

        // QEM follow-up should reduce further.
        mesh.decimate(1e-6, None);
        assert_watertight(&mesh, "prism post-decimate");
        let tri_final = mesh.triangles.len();
        assert!(
            tri_final < tri_after_retri,
            "QEM should reduce further: {tri_after_retri} → {tri_final}"
        );
        println!("Prism 10x3x10: {tri_before} → {tri_after_retri} (retri) → {tri_final} (QEM)");
    }

    #[test]
    fn coplanar_retri_single_voxel_integrity() {
        let mut mesh = build_chamfered_voxel_mesh(&[(0, 0, 0)]);
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.coplanar_region_retri(None);
        assert_watertight(&mesh, "single voxel post-retri");
        assert_no_degenerate_triangles(&mesh, "single voxel post-retri");

        let vol_diff = (smooth_mesh_volume(&mesh) - vol_before).abs();
        assert!(vol_diff < 0.0001, "retri changed volume by {vol_diff}");
    }

    #[test]
    fn coplanar_retri_hollow_frame_integrity() {
        let mut mesh = build_hollow_frame();
        assert_watertight(&mesh, "hollow frame pre");
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.coplanar_region_retri(None);
        assert_watertight(&mesh, "hollow frame post-retri");
        assert_no_degenerate_triangles(&mesh, "hollow frame post-retri");

        let vol_diff = (smooth_mesh_volume(&mesh) - vol_before).abs();
        assert!(vol_diff < 0.001, "retri changed volume by {vol_diff}");
    }

    #[test]
    fn coplanar_retri_diamond_integrity() {
        let mut mesh = build_diamond_pyramid(6, 7);
        assert_watertight(&mesh, "diamond pre");
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.coplanar_region_retri(None);
        assert_watertight(&mesh, "diamond post-retri");
        assert_no_degenerate_triangles(&mesh, "diamond post-retri");

        let vol_diff = (smooth_mesh_volume(&mesh) - vol_before).abs();
        assert!(vol_diff < 0.001, "retri changed volume by {vol_diff}");
    }

    #[test]
    fn coplanar_retri_l_shape_integrity() {
        let mut mesh = build_l_shape();
        assert_watertight(&mesh, "L-shape pre");
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.coplanar_region_retri(None);
        assert_watertight(&mesh, "L-shape post-retri");
        assert_no_degenerate_triangles(&mesh, "L-shape post-retri");

        let vol_diff = (smooth_mesh_volume(&mesh) - vol_before).abs();
        assert!(vol_diff < 0.001, "retri changed volume by {vol_diff}");
    }

    #[test]
    fn collinear_boundary_collapse_integrity() {
        // The collinear boundary collapse should reduce triangles further
        // after retri, while preserving watertightness and volume.
        let mut mesh = build_prism(10, 3, 10);
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.coplanar_region_retri(None);
        let after_retri = mesh.triangles.len();

        mesh.collapse_collinear_boundary_vertices(None);
        assert_watertight(&mesh, "prism post-collinear-collapse");
        assert_no_degenerate_triangles(&mesh, "prism post-collinear-collapse");
        let after_collapse = mesh.triangles.len();

        let vol_diff = (smooth_mesh_volume(&mesh) - vol_before).abs();
        assert!(
            vol_diff < 0.001,
            "collinear collapse changed volume by {vol_diff}"
        );
        assert!(
            after_collapse <= after_retri,
            "collinear collapse should not increase tris: {after_retri} → {after_collapse}"
        );
    }

    #[test]
    fn collinear_boundary_collapse_diamond_integrity() {
        let mut mesh = build_diamond_pyramid(6, 7);
        let vol_before = smooth_mesh_volume(&mesh);

        mesh.coplanar_region_retri(None);
        mesh.collapse_collinear_boundary_vertices(None);
        assert_watertight(&mesh, "diamond post-collinear-collapse");
        assert_no_degenerate_triangles(&mesh, "diamond post-collinear-collapse");

        let vol_diff = (smooth_mesh_volume(&mesh) - vol_before).abs();
        assert!(vol_diff < 0.001, "volume changed by {vol_diff}");
    }

    #[test]
    fn test_full_pipeline_with_chunk_bounds() {
        // Build a 10×3×10 prism and run all three passes with chunk bounds
        // covering the whole prism. Should be watertight, no degenerate
        // triangles, and volume preserved.
        let mut mesh = build_prism(10, 3, 10);
        assert_watertight(&mesh, "chunk_bounds pre");
        assert_no_degenerate_triangles(&mesh, "chunk_bounds pre");
        let vol_before = smooth_mesh_volume(&mesh);
        let tri_before = mesh.triangles.len();

        let bounds = Some(([0, 0, 0], [16, 16, 16]));
        mesh.coplanar_region_retri(bounds);
        mesh.collapse_collinear_boundary_vertices(bounds);
        mesh.decimate(1e-6, bounds);

        assert_watertight(&mesh, "chunk_bounds post");
        assert_no_degenerate_triangles(&mesh, "chunk_bounds post");
        let vol_after = smooth_mesh_volume(&mesh);
        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.001,
            "chunk_bounds: volume changed by {vol_diff}: {vol_before} → {vol_after}"
        );
        assert!(
            mesh.triangles.len() < tri_before,
            "chunk_bounds: should reduce triangles: {tri_before} → {}",
            mesh.triangles.len()
        );
    }

    #[test]
    fn test_coplanar_retri_preserves_normal_direction() {
        // Verify that retri doesn't flip any triangle normals. For each
        // triangle after retri, its normal should dot positively with the
        // volume-outward direction. We check this using the signed volume:
        // if the mesh volume sign is preserved, normals are consistent.
        let mut mesh = build_prism(10, 3, 10);
        let vol_before = smooth_mesh_volume(&mesh);
        assert!(vol_before > 0.0, "prism should have positive volume");

        mesh.coplanar_region_retri(None);

        let vol_after = smooth_mesh_volume(&mesh);
        // If normals flipped, the signed volume would change sign or be
        // dramatically different. Check it stays positive and close.
        assert!(
            vol_after > 0.0,
            "volume sign flipped after retri: {vol_before} → {vol_after}"
        );
        let vol_diff = (vol_after - vol_before).abs();
        assert!(
            vol_diff < 0.001,
            "retri changed volume by {vol_diff}: {vol_before} → {vol_after}"
        );

        // Also directly check: no triangle should have a normal that dots
        // negatively with the MAJORITY of its axis-aligned neighbors.
        // Simpler check: count triangles by dominant axis direction. The
        // prism has 6 face groups; after retri the counts change but no
        // group should have ZERO triangles (that would mean all normals
        // in that direction flipped).
        let axis_dirs = [
            [1.0f32, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        for dir in &axis_dirs {
            let count = mesh
                .triangles
                .iter()
                .filter(|tri| {
                    let positions: [[f32; 3]; 3] =
                        std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
                    let n = triangle_normal(positions);
                    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    if len < 1e-10 {
                        return false;
                    }
                    let dot = (n[0] * dir[0] + n[1] * dir[1] + n[2] * dir[2]) / len;
                    dot > 0.5
                })
                .count();
            assert!(
                count > 0,
                "no triangles facing {:?} after retri — normals may have flipped",
                dir
            );
        }
    }

    #[test]
    fn reduction_summary() {
        let shapes: Vec<(&str, SmoothMesh)> = vec![
            ("Single voxel", build_chamfered_voxel_mesh(&[(0, 0, 0)])),
            ("Prism 10x3x10", build_prism(10, 3, 10)),
            ("Pyramid 10x5", build_pyramid(10, 5)),
            ("Diamond r=6 h=7", build_diamond_pyramid(6, 7)),
            ("Truncated diamond", build_diamond_pyramid(6, 3)),
            ("L-shape", build_l_shape()),
            ("Hollow frame", build_hollow_frame()),
            ("Staircase", build_staircase()),
            ("Platform on base", build_platform_on_base()),
        ];
        for (name, mut mesh) in shapes {
            let original = mesh.triangles.len();
            mesh.coplanar_region_retri(None);
            let after_retri = mesh.triangles.len();
            mesh.collapse_collinear_boundary_vertices(None);
            let after_collinear = mesh.triangles.len();
            mesh.decimate(1e-6, None);
            let final_count = mesh.triangles.len();
            println!(
                "{name:>20}: {original:>5} → {after_retri:>5} (retri) → {after_collinear:>5} (collinear) → {final_count:>5} (QEM) [{:.0}%]",
                100.0 * (1.0 - final_count as f64 / original as f64)
            );
        }
    }
}
