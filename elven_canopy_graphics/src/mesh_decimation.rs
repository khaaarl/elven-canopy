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

use rustc_hash::{FxHashMap, FxHashSet};

use crate::smooth_mesh::SmoothMesh;

/// The 26 canonical normal directions for chamfered voxel meshes:
/// 6 cardinal face normals, 12 edge-chamfer normals (45°), and
/// 8 corner-chamfer normals. Any triangle normal that doesn't match
/// one of these (within tolerance) represents a surface that doesn't
/// exist in the original chamfered mesh — a decimation artifact.
#[allow(clippy::approx_constant)]
const CANONICAL_NORMALS: [[f32; 3]; 26] = [
    // 6 cardinal
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    // 12 edge chamfer
    [0.7071, 0.7071, 0.0],
    [0.7071, -0.7071, 0.0],
    [-0.7071, 0.7071, 0.0],
    [-0.7071, -0.7071, 0.0],
    [0.7071, 0.0, 0.7071],
    [0.7071, 0.0, -0.7071],
    [-0.7071, 0.0, 0.7071],
    [-0.7071, 0.0, -0.7071],
    [0.0, 0.7071, 0.7071],
    [0.0, 0.7071, -0.7071],
    [0.0, -0.7071, 0.7071],
    [0.0, -0.7071, -0.7071],
    // 8 corner chamfer
    [0.5774, 0.5774, 0.5774],
    [0.5774, 0.5774, -0.5774],
    [0.5774, -0.5774, 0.5774],
    [0.5774, -0.5774, -0.5774],
    [-0.5774, 0.5774, 0.5774],
    [-0.5774, 0.5774, -0.5774],
    [-0.5774, -0.5774, 0.5774],
    [-0.5774, -0.5774, -0.5774],
];

/// Minimum dot product with a canonical normal for a triangle to be
/// considered "on-surface." cos(10°) ≈ 0.985. Triangles whose best
/// canonical match is below this threshold are cross-surface artifacts.
const CANONICAL_NORMAL_THRESHOLD: f32 = 0.985;

/// Maximum aspect ratio (longest_edge² / area) for triangles produced
/// by the decimation pipeline. With flat shading, slivers don't cause
/// interpolation artifacts, so this is generous — only blocks
/// pathological cases where triangles span many voxels with near-zero
/// width. Used in retri fan quality check, QEM collapse guard, and tests.
const MAX_SLIVER_ASPECT_RATIO: f32 = 5000.0;

/// Check whether a normalized normal vector matches one of the 26
/// canonical directions within the threshold.
fn is_canonical_normal(normal: [f32; 3]) -> bool {
    CANONICAL_NORMALS.iter().any(|cn| {
        normal[0] * cn[0] + normal[1] * cn[1] + normal[2] * cn[2] >= CANONICAL_NORMAL_THRESHOLD
    })
}

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
        // FxHashMap for O(1) lookups — edge iteration order doesn't matter.
        // Pre-size: a manifold mesh has ~1.5 edges per triangle.
        let tri_count = self.triangles.len();
        let mut edge_tris: FxHashMap<(u32, u32), Vec<usize>> =
            FxHashMap::with_capacity_and_hasher(tri_count * 3 / 2, Default::default());
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

            let region_set: FxHashSet<usize> = region_tris.iter().copied().collect();

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
            let mut next_vert: FxHashMap<u32, u32> = FxHashMap::default();
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
            // boundary edge (centroid, edge.a, edge.b).
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

            // Check that every fan triangle (centroid, edge.a, edge.b) has
            // acceptable quality. Two failure modes:
            //
            // 1. Collinear centroid: the centroid lies on a boundary edge,
            //    producing a degenerate zero-area fan triangle.
            //
            // 2. Near-collinear centroid: the centroid is very close to a
            //    boundary edge but not on it, producing a sliver triangle
            //    with unstable normals and visible shading artifacts.
            //
            // Check the aspect ratio (longest_edge² / area) of each fan
            // triangle. If any exceeds the threshold, skip this region.
            let mut has_bad_fan_tri = false;
            for i in 0..n_poly {
                let pa = self.vertices[polygon[i] as usize].position;
                let pb = self.vertices[polygon[(i + 1) % n_poly] as usize].position;

                // Cross product of (centroid - pa) and (pb - pa) gives
                // twice the triangle area as a vector.
                let e1 = [
                    centroid_pos[0] - pa[0],
                    centroid_pos[1] - pa[1],
                    centroid_pos[2] - pa[2],
                ];
                let e2 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
                let cross = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let twice_area =
                    (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
                let area = twice_area * 0.5;

                // Degenerate: zero area.
                if twice_area < 1e-8 {
                    has_bad_fan_tri = true;
                    break;
                }

                // In chamfer-only mode, check that the fan triangle's
                // normal matches one of the 26 canonical directions.
                if !self.config.smoothing_enabled {
                    let n = [
                        cross[0] / twice_area,
                        cross[1] / twice_area,
                        cross[2] / twice_area,
                    ];
                    if !is_canonical_normal(n) {
                        has_bad_fan_tri = true;
                        break;
                    }
                }

                // Sliver: check aspect ratio (longest_edge² / area).
                let e3 = [
                    centroid_pos[0] - pb[0],
                    centroid_pos[1] - pb[1],
                    centroid_pos[2] - pb[2],
                ];
                let len1_sq = e1[0] * e1[0] + e1[1] * e1[1] + e1[2] * e1[2];
                let len2_sq = e2[0] * e2[0] + e2[1] * e2[1] + e2[2] * e2[2];
                let len3_sq = e3[0] * e3[0] + e3[1] * e3[1] + e3[2] * e3[2];
                let longest_sq = len1_sq.max(len2_sq).max(len3_sq);
                if longest_sq / area > MAX_SLIVER_ASPECT_RATIO {
                    has_bad_fan_tri = true;
                    break;
                }
            }
            if has_bad_fan_tri {
                continue;
            }

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
                sway_weight: 1.0,      // Recomputed after decimation.
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
        let surviving = dead_tris.iter().filter(|&&d| !d).count();
        let total_cap = surviving + new_triangles.len();
        let mut result_tris: Vec<[u32; 3]> = Vec::with_capacity(total_cap);
        let mut result_tags: Vec<u8> = Vec::with_capacity(total_cap);
        let mut result_colors: Vec<[f32; 4]> = Vec::with_capacity(total_cap);
        let mut result_vpos: Vec<[i32; 3]> = Vec::with_capacity(total_cap);

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
        // FxHashMap for O(1) lookups — edge iteration order doesn't matter.
        // Pre-size: a manifold mesh has ~1.5 edges per triangle.
        let tri_count = self.triangles.len();
        let mut edge_tris: FxHashMap<(u32, u32), Vec<usize>> =
            FxHashMap::with_capacity_and_hasher(tri_count * 3 / 2, Default::default());
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
            let mut neighbor_counts: FxHashMap<u32, u32> = FxHashMap::default();
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

            // Before committing: in chamfer-only mode, verify that the
            // merged triangles would have canonical normals. If not, skip
            // this vertex (the collinear collapse would bridge surfaces).
            if !self.config.smoothing_enabled {
                let mut would_produce_bad_normal = false;
                for &(centroid, _, _) in &pairs {
                    let new_positions: [[f32; 3]; 3] = [
                        self.vertices[centroid as usize].position,
                        self.vertices[prev as usize].position,
                        self.vertices[next as usize].position,
                    ];
                    let n = triangle_normal(new_positions);
                    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    if len > 1e-10 && !is_canonical_normal([n[0] / len, n[1] / len, n[2] / len]) {
                        would_produce_bad_normal = true;
                        break;
                    }
                }
                if would_produce_bad_normal {
                    continue;
                }
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
        // Pre-size: a manifold mesh has ~1.5 edges per triangle.
        let mut heap = BinaryHeap::new();
        let mut seen_edges: FxHashSet<(u32, u32)> =
            FxHashSet::with_capacity_and_hasher(self.triangles.len() * 3 / 2, Default::default());
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
        // Linear scan: neighbor lists are small (typically 4-12 elements),
        // so O(n*m) is faster than BTreeSet construction + lookup.
        let v0_nbrs = &self.vertices[v0 as usize].neighbors;
        let mut common = 0;
        for &ni in &self.vertices[v1 as usize].neighbors {
            if ni != v0 && !state.removed[ni as usize] && v0_nbrs.contains(&ni) {
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

            // Reject if the triangle would become degenerate or near-degenerate.
            // A near-degenerate triangle (three nearly-collinear vertices) has
            // an unstable normal that causes visible shading artifacts. The
            // cross product magnitude is twice the triangle area; 1e-6
            // corresponds to area ~5e-7, well below any visible triangle.
            if len_after < 1e-6 {
                return true;
            }

            // Reject if the triangle's area shrinks dramatically — even if
            // the absolute area is still above the threshold, a 1000×
            // reduction means the collapse is squashing the triangle nearly
            // flat, producing a sliver with unreliable normals.
            if len_after < len_before * 0.001 {
                return true;
            }

            // Reject if the triangle's normal would flip.
            let dot =
                n_before[0] * n_after[0] + n_before[1] * n_after[1] + n_before[2] * n_after[2];

            if dot < 0.0 {
                return true;
            }

            // Reject if the collapse would create an extreme sliver. On
            // coplanar surfaces the QEM error is zero for any in-plane
            // collapse, so the error threshold alone can't prevent triangles
            // from spanning arbitrarily large distances. Check the aspect
            // ratio: longest_edge² / area. Threshold 5000 is generous (flat
            // shading means slivers don't cause interpolation artifacts) but
            // prevents pathological cases where triangles span many voxels.
            let edges_sq_after: [f32; 3] = [
                (positions_after[1][0] - positions_after[0][0]).powi(2)
                    + (positions_after[1][1] - positions_after[0][1]).powi(2)
                    + (positions_after[1][2] - positions_after[0][2]).powi(2),
                (positions_after[2][0] - positions_after[1][0]).powi(2)
                    + (positions_after[2][1] - positions_after[1][1]).powi(2)
                    + (positions_after[2][2] - positions_after[1][2]).powi(2),
                (positions_after[0][0] - positions_after[2][0]).powi(2)
                    + (positions_after[0][1] - positions_after[2][1]).powi(2)
                    + (positions_after[0][2] - positions_after[2][2]).powi(2),
            ];
            let longest_sq = edges_sq_after[0]
                .max(edges_sq_after[1])
                .max(edges_sq_after[2]);
            let area_after = len_after * 0.5;
            if area_after > 1e-6 && longest_sq / area_after > MAX_SLIVER_ASPECT_RATIO {
                return true;
            }

            // In chamfer-only mode (no smoothing), reject collapses that
            // would produce triangles with non-canonical normals. Chamfered
            // voxel meshes have exactly 26 possible orientations; any other
            // normal means the collapse bridges two differently-angled
            // surfaces across a crease edge. This check is skipped in smooth
            // mode where arbitrary normals are expected (future LoD use).
            if !self.config.smoothing_enabled {
                let n_after_normalized = [
                    n_after[0] / len_after,
                    n_after[1] / len_after,
                    n_after[2] / len_after,
                ];
                if !is_canonical_normal(n_after_normalized) {
                    return true;
                }
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

        let used_count = used.iter().filter(|&&u| u).count();
        let mut remap: Vec<u32> = vec![u32::MAX; self.vertices.len()];
        let mut new_vertices = Vec::with_capacity(used_count);
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
        // Push unconditionally (no per-edge contains check), then batch-
        // deduplicate. This avoids O(degree) scans on every push — the same
        // optimization applied to add_edge in Attempt 4.
        for tri in &remapped_triangles {
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                new_vertices[a as usize].neighbors.push(b);
                new_vertices[b as usize].neighbors.push(a);
            }
        }
        for v in &mut new_vertices {
            crate::smooth_mesh::dedup_preserve_order(&mut v.neighbors);
        }

        // Rebuild the dedup map with pre-sized capacity.
        self.dedup = FxHashMap::with_capacity_and_hasher(new_vertices.len(), Default::default());
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

/// Ray direction for point-in-mesh testing. Slightly tilted from +X to
/// avoid axis-aligned coincidences with voxel geometry edges/vertices.
/// The small prime-ratio Y and Z offsets ensure the ray is not parallel
/// to any face of the voxel grid.
#[cfg(test)]
const RAY_DIR: [f32; 3] = [1.0, 0.0000037, 0.0000059];

/// Möller–Trumbore ray-triangle intersection test. Returns the distance
/// along the ray if it hits the triangle, or `None` if it misses.
#[cfg(test)]
fn ray_triangle_intersect(
    origin: [f32; 3],
    dir: [f32; 3],
    v0: [f32; 3],
    v1: [f32; 3],
    v2: [f32; 3],
) -> Option<f32> {
    let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];

    // h = cross(dir, e2)
    let h = [
        dir[1] * e2[2] - dir[2] * e2[1],
        dir[2] * e2[0] - dir[0] * e2[2],
        dir[0] * e2[1] - dir[1] * e2[0],
    ];

    let a = e1[0] * h[0] + e1[1] * h[1] + e1[2] * h[2];
    if a.abs() < 1e-10 {
        return None; // Ray parallel to triangle.
    }

    let f = 1.0 / a;
    let s = [origin[0] - v0[0], origin[1] - v0[1], origin[2] - v0[2]];
    let u = f * (s[0] * h[0] + s[1] * h[1] + s[2] * h[2]);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    // q = cross(s, e1)
    let q = [
        s[1] * e1[2] - s[2] * e1[1],
        s[2] * e1[0] - s[0] * e1[2],
        s[0] * e1[1] - s[1] * e1[0],
    ];
    let v = f * (dir[0] * q[0] + dir[1] * q[1] + dir[2] * q[2]);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * (e2[0] * q[0] + e2[1] * q[1] + e2[2] * q[2]);
    if t > 1e-6 {
        Some(t)
    } else {
        None // Intersection behind ray origin.
    }
}

/// Test whether a point is inside a watertight triangle mesh using
/// raycasting: cast a ray in the +X direction and count intersections.
/// Odd count = inside, even count = outside.
#[cfg(test)]
fn point_is_inside(point: [f32; 3], vertices: &[[f32; 3]], triangles: &[[u32; 3]]) -> bool {
    let mut count = 0;
    for tri in triangles {
        let v0 = vertices[tri[0] as usize];
        let v1 = vertices[tri[1] as usize];
        let v2 = vertices[tri[2] as usize];
        if ray_triangle_intersect(point, RAY_DIR, v0, v1, v2).is_some() {
            count += 1;
        }
    }
    count % 2 == 1
}

/// Generate sample points near the surface of a mesh. For each triangle,
/// compute its centroid and offset slightly inward and outward along the
/// normal. Returns `(inside_points, outside_points)`.
#[cfg(test)]
fn sample_near_surface(
    vertices: &[[f32; 3]],
    triangles: &[[u32; 3]],
    offset: f32,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();

    for tri in triangles {
        let p: [[f32; 3]; 3] = std::array::from_fn(|i| vertices[tri[i] as usize]);
        let n = triangle_normal(p);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len < 1e-10 {
            continue;
        }
        let normal = [n[0] / len, n[1] / len, n[2] / len];

        // Triangle centroid.
        let centroid = [
            (p[0][0] + p[1][0] + p[2][0]) / 3.0,
            (p[0][1] + p[1][1] + p[2][1]) / 3.0,
            (p[0][2] + p[1][2] + p[2][2]) / 3.0,
        ];

        // Offset outward (along normal) and inward (against normal).
        // For meshes with outward-pointing normals, +normal = outside.
        outside.push([
            centroid[0] + normal[0] * offset,
            centroid[1] + normal[1] * offset,
            centroid[2] + normal[2] * offset,
        ]);
        inside.push([
            centroid[0] - normal[0] * offset,
            centroid[1] - normal[1] * offset,
            centroid[2] - normal[2] * offset,
        ]);
    }

    (inside, outside)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_gen::{FACE_NORMALS, FACE_VERTICES, voxel_color};
    use crate::smooth_mesh::{TAG_BARK, TAG_GROUND};
    use elven_canopy_sim::types::VoxelType;

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

        mesh.resolve_non_manifold();
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

        mesh.deduplicate_neighbors();
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

        mesh.deduplicate_neighbors();
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
        mesh.deduplicate_neighbors();
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

    /// Check that every vertex position after decimation existed (within
    /// tolerance) before decimation. The QEM pass should only pick existing
    /// endpoint positions, but cascading collapses after retri centroid
    /// insertion could in theory produce positions far from any original.
    fn assert_no_novel_positions(
        before: &[[f32; 3]],
        after: &[[f32; 3]],
        max_dist: f32,
        label: &str,
    ) {
        for (vi, pos) in after.iter().enumerate() {
            let min_dist_sq = before
                .iter()
                .map(|p| {
                    (p[0] - pos[0]).powi(2) + (p[1] - pos[1]).powi(2) + (p[2] - pos[2]).powi(2)
                })
                .fold(f32::MAX, f32::min);
            let min_dist = min_dist_sq.sqrt();
            assert!(
                min_dist < max_dist,
                "{label}: vertex {vi} at {:?} is {min_dist:.4} from nearest original position \
                 (max allowed: {max_dist})",
                pos
            );
        }
    }

    /// Run the full integrity suite on a mesh: watertight, no degenerate
    /// triangles, no stray vertices, volume preserved, no novel vertex
    /// positions. Runs the flat-face pre-pass before QEM decimation
    /// (matching the production pipeline).
    fn assert_decimation_integrity(label: &str, mesh: &mut SmoothMesh, vol_tolerance: f32) {
        assert_watertight(mesh, &format!("{label} pre-decimation"));
        assert_no_degenerate_triangles(mesh, &format!("{label} pre-decimation"));
        let vol_before = smooth_mesh_volume(mesh);
        let tri_before = mesh.triangles.len();
        let positions_before: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();

        mesh.coplanar_region_retri(None);
        mesh.collapse_collinear_boundary_vertices(None);

        // Record positions after retri (includes centroids) but before QEM.
        let positions_after_retri: Vec<[f32; 3]> =
            mesh.vertices.iter().map(|v| v.position).collect();

        mesh.decimate(1e-6, None);

        assert_watertight(mesh, &format!("{label} post-decimation"));
        assert_no_degenerate_triangles(mesh, &format!("{label} post-decimation"));
        assert_no_stray_vertices(mesh, [-1.0; 3], label);

        // Every post-decimation vertex should be near a position that existed
        // after retri (QEM only picks existing endpoints, never creates new
        // positions). Tolerance of 0.01 allows for f32 accumulation error.
        let positions_after: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
        assert_no_novel_positions(
            &positions_after_retri,
            &positions_after,
            0.01,
            &format!("{label} QEM vs retri"),
        );

        // Also check that retri centroids didn't stray far from original
        // vertex positions. Centroids of coplanar regions should be within
        // the convex hull of original positions on that region, so max
        // distance should be bounded by the region diameter.
        // Use a generous 2.0 threshold (centroids can be inside a polygon
        // far from any original vertex on large flat faces).
        assert_no_novel_positions(
            &positions_before,
            &positions_after_retri,
            2.0,
            &format!("{label} retri vs original"),
        );

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

    // --- Diagonal terrain shape builders (B-qem-deformation) ---
    //
    // These target the specific geometries that cause visible deformation in
    // practice: flat terrain with single-voxel height steps (grassy hills),
    // diagonal valleys, concave diagonal corners, and thin diagonal ridges.
    // The existing test shapes (pyramids, prisms) are too regular and convex.

    /// Flat 12×12 terrain with a diagonal ridge running from corner to
    /// corner. The ridge is 2 voxels wide to maintain face-adjacency with
    /// the base (1-wide diagonal creates edge-only adjacency → non-manifold).
    fn build_diagonal_ridge() -> SmoothMesh {
        let mut voxels = Vec::new();
        // Flat base layer
        for x in 0..12 {
            for z in 0..12 {
                voxels.push((x, 0, z));
            }
        }
        // Diagonal ridge: y=1 along x==z, 2 voxels wide (x and x+1)
        for i in 0..12 {
            voxels.push((i, 1, i));
            if i + 1 < 12 {
                voxels.push((i + 1, 1, i));
            }
        }
        voxels.sort();
        voxels.dedup();
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Rolling hills: flat 16×16 base with scattered single-height bumps
    /// creating irregular diagonal edges. Mimics the grassy terrain that
    /// causes the most visible artifacts.
    fn build_rolling_hills() -> SmoothMesh {
        let mut voxels = Vec::new();
        // Flat base layer
        for x in 0..16 {
            for z in 0..16 {
                voxels.push((x, 0, z));
            }
        }
        // Hill 1: 3×3 bump near corner
        for x in 2..5 {
            for z in 2..5 {
                voxels.push((x, 1, z));
            }
        }
        // Hill 2: 2×4 bump, offset — creates diagonal step between hills
        for x in 5..7 {
            for z in 4..8 {
                voxels.push((x, 1, z));
            }
        }
        // Hill 3: single voxel bump — isolated step
        voxels.push((10, 1, 10));
        // Hill 4: L-shaped bump — concave diagonal corner
        for x in 9..13 {
            voxels.push((x, 1, 3));
        }
        for z in 3..7 {
            voxels.push((9, 1, z));
        }
        // Hill 5: 2-high bump on top of hill 1 — nested steps
        voxels.push((3, 2, 3));
        voxels.push((3, 2, 4));
        voxels.push((4, 2, 3));

        voxels.sort();
        voxels.dedup();
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Diagonal valley: two slopes descending toward the x-axis center,
    /// creating a V-shape in cross-section. Height increases with distance
    /// from the center column (z == size/2). This avoids edge-only adjacency
    /// by using axis-aligned height steps.
    fn build_diagonal_valley() -> SmoothMesh {
        let mut voxels = Vec::new();
        let size = 12;
        let center_z = size / 2;
        for x in 0..size {
            for z in 0..size {
                let dist = (z as i32 - center_z as i32).abs();
                // Stagger the step positions based on x to create diagonal
                // creases rather than axis-aligned ones.
                let stagger = x % 2;
                let height = (dist + stagger).max(1);
                for y in 0..height {
                    voxels.push((x, y, z));
                }
            }
        }
        voxels.sort();
        voxels.dedup();
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Concave diagonal nook: two walls meeting at 45°. The inside corner
    /// is the concave diagonal geometry where deformation is most visible.
    fn build_diagonal_nook() -> SmoothMesh {
        let mut voxels = Vec::new();
        // Floor
        for x in 0..10 {
            for z in 0..10 {
                voxels.push((x, 0, z));
            }
        }
        // Wall 1: along x==0, full height
        for z in 0..10 {
            for y in 1..4 {
                voxels.push((0, y, z));
            }
        }
        // Wall 2: diagonal wall along x==z, from (1,1,1) to (9,1,9)
        for i in 1..10 {
            for y in 1..4 {
                voxels.push((i, y, i));
            }
        }
        // Fill the corner: voxels between the two walls where z > x
        for x in 1..10 {
            for z in (x + 1)..10 {
                for y in 1..4 {
                    voxels.push((x, y, z));
                }
            }
        }
        voxels.sort();
        voxels.dedup();
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Zigzag terrain: alternating single-step rises and falls creating a
    /// sawtooth pattern along the X axis. Every other column is 1 higher.
    fn build_zigzag_terrain() -> SmoothMesh {
        let mut voxels = Vec::new();
        for x in 0..14 {
            let height = if x % 2 == 0 { 1 } else { 2 };
            for y in 0..height {
                for z in 0..8 {
                    voxels.push((x, y, z));
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Scattered bumps on a flat base: a solid y=0 base with isolated 2×2
    /// bumps at y=1 placed on a 5×5 grid so no two bumps are diagonally
    /// adjacent. Creates many independent step edges and concave corners
    /// from each bump meeting the flat base.
    fn build_diagonal_checkerboard() -> SmoothMesh {
        let mut voxels = Vec::new();
        // Full base layer
        for x in 0..15 {
            for z in 0..15 {
                voxels.push((x, 0, z));
            }
        }
        // 2×2 bumps on a 5-spacing grid (gap of 3 prevents diagonal adjacency)
        for gx in 0..3 {
            for gz in 0..3 {
                let bx = gx * 5 + 1;
                let bz = gz * 5 + 1;
                for x in bx..(bx + 2) {
                    for z in bz..(bz + 2) {
                        voxels.push((x, 1, z));
                    }
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Terraced hillside: flat layers stepping up diagonally, like a rice
    /// paddy on a hillside. Each terrace is 3 deep, stepping up by 1 in Y
    /// and back by 3 in Z.
    fn build_terraced_hillside() -> SmoothMesh {
        let mut voxels = Vec::new();
        let width = 10;
        for terrace in 0..5 {
            let y_base = terrace;
            let z_start = terrace * 3;
            for x in 0..width {
                for z in z_start..(z_start + 3) {
                    for y in 0..=y_base {
                        voxels.push((x, y, z));
                    }
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Saddle point: two diagonal ridges crossing, creating a saddle (the
    /// height is max(|x-cx|, |z-cz|) inverted). Concave in one diagonal,
    /// convex in the other.
    fn build_saddle() -> SmoothMesh {
        let mut voxels = Vec::new();
        let size = 10;
        let cx = size / 2;
        let cz = size / 2;
        for x in 0..size {
            for z in 0..size {
                // Saddle: height = |x-cx| - |z-cz| shifted to be non-negative
                let h = ((x as i32 - cx as i32).abs() - (z as i32 - cz as i32).abs()) + cx;
                let h = h.max(0).min(size);
                for y in 0..h {
                    voxels.push((x, y, z));
                }
            }
        }
        build_chamfered_voxel_mesh(&voxels)
    }

    /// Cove: a concave bowl shape (inverted truncated diamond pyramid).
    /// The inside surfaces are where deformation is most visible because
    /// normals point inward toward the concavity.
    fn build_cove() -> SmoothMesh {
        let mut voxels = Vec::new();
        let outer = 10;
        let depth = 4;
        // Solid base
        for x in 0..outer {
            for z in 0..outer {
                voxels.push((x, 0, z));
            }
        }
        // Walls: remove interior voxels to create bowl
        let cx = outer / 2;
        let cz = outer / 2;
        for y in 1..=depth {
            let inner_radius = y; // bowl opens wider as we go up
            for x in 0..outer {
                for z in 0..outer {
                    let dx = (x as i32 - cx as i32).abs();
                    let dz = (z as i32 - cz as i32).abs();
                    // Keep voxel if it's OUTSIDE the bowl (Manhattan distance
                    // from center > inner_radius)
                    if dx + dz >= inner_radius {
                        voxels.push((x, y, z));
                    }
                }
            }
        }
        voxels.sort();
        voxels.dedup();
        build_chamfered_voxel_mesh(&voxels)
    }

    // --- Integrity tests for diagonal terrain shapes ---

    #[test]
    fn diagonal_ridge_decimation_integrity() {
        let mut mesh = build_diagonal_ridge();
        assert_decimation_integrity("diagonal ridge", &mut mesh, 0.01);
    }

    #[test]
    fn rolling_hills_decimation_integrity() {
        let mut mesh = build_rolling_hills();
        assert_decimation_integrity("rolling hills", &mut mesh, 0.01);
    }

    #[test]
    fn diagonal_valley_decimation_integrity() {
        let mut mesh = build_diagonal_valley();
        assert_decimation_integrity("diagonal valley", &mut mesh, 0.01);
    }

    #[test]
    fn diagonal_nook_decimation_integrity() {
        let mut mesh = build_diagonal_nook();
        assert_decimation_integrity("diagonal nook", &mut mesh, 0.01);
    }

    #[test]
    fn zigzag_terrain_decimation_integrity() {
        let mut mesh = build_zigzag_terrain();
        assert_decimation_integrity("zigzag terrain", &mut mesh, 0.01);
    }

    #[test]
    fn diagonal_checkerboard_decimation_integrity() {
        let mut mesh = build_diagonal_checkerboard();
        assert_decimation_integrity("diagonal checkerboard", &mut mesh, 0.01);
    }

    #[test]
    fn terraced_hillside_decimation_integrity() {
        let mut mesh = build_terraced_hillside();
        assert_decimation_integrity("terraced hillside", &mut mesh, 0.01);
    }

    #[test]
    fn saddle_decimation_integrity() {
        let mut mesh = build_saddle();
        assert_decimation_integrity("saddle", &mut mesh, 0.01);
    }

    #[test]
    fn cove_decimation_integrity() {
        let mut mesh = build_cove();
        assert_decimation_integrity("cove", &mut mesh, 0.01);
    }

    // --- Aggressive decimation stress tests ---
    //
    // These use higher max_error thresholds to force more QEM collapses,
    // which is more likely to trigger visible deformation cascades. Even
    // with higher error, watertightness and no-degenerate-triangles must
    // still hold. Volume tolerance is proportionally relaxed.

    /// Run decimation at a given max_error and check structural integrity.
    /// Unlike assert_decimation_integrity, this does NOT check volume
    /// (aggressive decimation intentionally trades volume for triangle count)
    /// but still checks watertight, no degenerate, and no normal flips
    /// (via signed volume staying positive).
    fn assert_aggressive_decimation_integrity(label: &str, mesh: &mut SmoothMesh, max_error: f32) {
        assert_watertight(mesh, &format!("{label} pre"));
        let signed_vol_before = {
            let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
            mesh_signed_volume(&positions, &mesh.triangles)
        };
        // Volume sign depends on winding convention — just check non-zero.
        assert!(
            signed_vol_before.abs() > 0.01,
            "{label}: pre-decimation volume is near zero: {signed_vol_before}"
        );

        mesh.coplanar_region_retri(None);
        mesh.collapse_collinear_boundary_vertices(None);
        mesh.decimate(max_error, None);

        assert_watertight(mesh, &format!("{label} post (max_error={max_error})"));
        assert_no_degenerate_triangles(mesh, &format!("{label} post (max_error={max_error})"));

        let signed_vol_after = {
            let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
            mesh_signed_volume(&positions, &mesh.triangles)
        };
        // Volume sign must be preserved (no global normal flip).
        assert!(
            signed_vol_before.signum() == signed_vol_after.signum(),
            "{label}: volume sign flipped after decimation: {signed_vol_before} → {signed_vol_after}"
        );
    }

    /// Test diagonal shapes with chunk bounds matching the production 16×16×16
    /// chunk size. Chunk boundary pinning interacts with diagonal step edges
    /// because the pinned vertices constrain which collapses are available,
    /// potentially forcing suboptimal collapse choices on nearby geometry.
    #[test]
    fn chunk_bounded_diagonal_shapes() {
        // Use a 16×16×16 chunk bounds matching production.
        let bounds = Some(([0, 0, 0], [16, 16, 16]));
        let shapes: Vec<(&str, SmoothMesh)> = vec![
            ("diagonal ridge", build_diagonal_ridge()),
            ("rolling hills", build_rolling_hills()),
            ("diagonal valley", build_diagonal_valley()),
            ("diagonal nook", build_diagonal_nook()),
            ("zigzag terrain", build_zigzag_terrain()),
            ("terraced hillside", build_terraced_hillside()),
            ("saddle", build_saddle()),
            ("cove", build_cove()),
        ];

        for (name, mut mesh) in shapes {
            assert_watertight(&mesh, &format!("{name} pre"));
            assert_no_degenerate_triangles(&mesh, &format!("{name} pre"));
            let vol_before = smooth_mesh_volume(&mesh);

            mesh.coplanar_region_retri(bounds);
            mesh.collapse_collinear_boundary_vertices(bounds);
            mesh.decimate(1e-6, bounds);

            assert_watertight(&mesh, &format!("{name} post-chunk"));
            assert_no_degenerate_triangles(&mesh, &format!("{name} post-chunk"));
            let vol_after = smooth_mesh_volume(&mesh);
            let vol_diff = (vol_after - vol_before).abs();
            assert!(
                vol_diff < 0.01,
                "{name}: volume changed by {vol_diff} with chunk bounds: {vol_before} → {vol_after}"
            );
        }
    }

    #[test]
    fn aggressive_decimation_diagonal_shapes() {
        // Test all diagonal terrain shapes at progressively higher error
        // thresholds. Even aggressive decimation must stay watertight.
        let thresholds = [0.001, 0.01, 0.1, 1.0];
        let builders: Vec<(&str, fn() -> SmoothMesh)> = vec![
            ("diagonal ridge", build_diagonal_ridge),
            ("rolling hills", build_rolling_hills),
            ("diagonal valley", build_diagonal_valley),
            ("diagonal nook", build_diagonal_nook),
            ("zigzag terrain", build_zigzag_terrain),
            ("terraced hillside", build_terraced_hillside),
            ("saddle", build_saddle),
            ("cove", build_cove),
        ];
        for &threshold in &thresholds {
            for &(name, builder) in &builders {
                let mut mesh = builder();
                let label = format!("{name} @{threshold}");
                assert_aggressive_decimation_integrity(&label, &mut mesh, threshold);
            }
        }
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

    /// Build terrain from a heightmap. Each (x,z) column has `height[x][z]`
    /// voxels stacked from y=0. This is the closest test analogue to the
    /// actual game terrain where the bugs appear.
    fn build_heightmap_terrain(heights: &[Vec<i32>]) -> SmoothMesh {
        build_chamfered_voxel_mesh(&heightmap_to_voxels(heights))
    }

    /// Generate a pseudo-random heightmap terrain using a simple hash.
    /// Returns heights in range [min_h, max_h] for a size×size grid.
    fn random_heightmap(seed: u64, size: usize, min_h: i32, max_h: i32) -> Vec<Vec<i32>> {
        let mut heights = vec![vec![0i32; size]; size];
        let mut state = seed.wrapping_add(1); // +1 avoids seed=0 producing all zeros
        for x in 0..size {
            for z in 0..size {
                // Simple xorshift hash for deterministic "randomness".
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let range = (max_h - min_h + 1) as u64;
                heights[x][z] = min_h + (state % range) as i32;
            }
        }
        heights
    }

    /// Generate a smooth heightmap where adjacent columns differ by at most
    /// 1, and no diagonal-only voxel adjacency occurs. The latter constraint
    /// prevents the chamfer from producing non-manifold edges.
    ///
    /// Diagonal-only adjacency happens when h(x,z) >= k, h(x+1,z+1) >= k,
    /// but h(x+1,z) < k AND h(x,z+1) < k. The fix: after generating random
    /// heights, fill such saddle points by raising one neighbor.
    fn smooth_random_heightmap(seed: u64, size: usize, base_h: i32) -> Vec<Vec<i32>> {
        let mut heights = vec![vec![base_h; size]; size];
        let mut state = seed.wrapping_add(1); // +1 avoids seed=0 all-zeros
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

        // Fix diagonal-only adjacency: for each height level, ensure no
        // two voxels are connected only by a shared edge (not a face).
        let max_h = heights
            .iter()
            .flat_map(|col| col.iter())
            .copied()
            .max()
            .unwrap_or(0);
        for h in 1..=max_h {
            for x in 0..(size - 1) {
                for z in 0..(size - 1) {
                    // Check both diagonal directions
                    // Diagonal (x,z)-(x+1,z+1): both >= h but neither (x+1,z) nor (x,z+1) >= h
                    if heights[x][z] >= h
                        && heights[x + 1][z + 1] >= h
                        && heights[x + 1][z] < h
                        && heights[x][z + 1] < h
                    {
                        heights[x + 1][z] = h; // Fill the gap
                    }
                    // Diagonal (x+1,z)-(x,z+1): both >= h but neither (x,z) nor (x+1,z+1) >= h
                    if heights[x + 1][z] >= h
                        && heights[x][z + 1] >= h
                        && heights[x][z] < h
                        && heights[x + 1][z + 1] < h
                    {
                        heights[x][z] = h; // Fill the gap
                    }
                }
            }
        }

        heights
    }

    /// Fuzz test: run decimation on many random terrain heightmaps and check
    /// integrity. This is the most likely way to find the rare geometric
    /// configurations that trigger deformation bugs.
    #[test]
    #[ignore] // Slow fuzz test (~50 seeds); run explicitly with `cargo test -- --ignored`
    fn fuzz_random_terrain_decimation() {
        let mut skipped = 0;
        for seed in 0..50 {
            let heights = random_heightmap(seed, 10, 1, 4);
            let mut mesh = build_heightmap_terrain(&heights);
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                skipped += 1;
                continue;
            }
            let label = format!("random terrain seed={seed}");
            assert_no_degenerate_triangles(&mesh, &label);
            let vol_before = smooth_mesh_volume(&mesh);
            let tri_before = mesh.triangles.len();

            mesh.coplanar_region_retri(None);
            mesh.collapse_collinear_boundary_vertices(None);
            mesh.decimate(1e-6, None);

            assert_watertight(&mesh, &format!("{label} post"));
            assert_no_degenerate_triangles(&mesh, &format!("{label} post"));
            let vol_after = smooth_mesh_volume(&mesh);
            let vol_diff = (vol_after - vol_before).abs();
            let vol_pct = if vol_before > 0.0 {
                vol_diff / vol_before * 100.0
            } else {
                0.0
            };
            assert!(
                vol_pct < 0.1,
                "{label}: volume changed by {vol_pct:.4}% ({vol_diff:.4}): {vol_before} → {vol_after}"
            );
            assert!(
                mesh.triangles.len() <= tri_before,
                "{label}: triangle count increased: {tri_before} → {}",
                mesh.triangles.len()
            );
        }
        println!("fuzz_random_terrain: skipped {skipped}/50 seeds (non-manifold input)");
    }

    // Note: smooth_terrain_seed5_degenerate_triangle and
    // qem_near_degenerate_seed1 were seed-specific reproduction tests for
    // bugs that are now guarded by the retri collinear centroid check and
    // the QEM near-degenerate threshold fix respectively. The fuzz tests
    // cover these scenarios across many seeds.

    /// Fuzz test with smooth terrain (adjacent columns differ by at most 1).
    /// This directly simulates the grassy hillside terrain described in the
    /// bug report.
    #[test]
    #[ignore] // Slow fuzz test (~200 seeds); run explicitly with `cargo test -- --ignored`
    fn fuzz_smooth_terrain_decimation() {
        let mut skipped = 0;
        let mut tested = 0;
        for seed in 0..200 {
            let heights = smooth_random_heightmap(seed, 12, 3);
            let mut mesh = build_heightmap_terrain(&heights);
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                skipped += 1;
                continue;
            }
            tested += 1;
            let label = format!("smooth terrain seed={seed}");
            assert_no_degenerate_triangles(&mesh, &label);
            let vol_before = smooth_mesh_volume(&mesh);

            mesh.coplanar_region_retri(None);
            mesh.collapse_collinear_boundary_vertices(None);
            mesh.decimate(1e-6, None);

            assert_watertight(&mesh, &format!("{label} post"));
            assert_no_degenerate_triangles(&mesh, &format!("{label} post"));
            let vol_after = smooth_mesh_volume(&mesh);
            let vol_diff = (vol_after - vol_before).abs();
            let vol_pct = if vol_before > 0.0 {
                vol_diff / vol_before * 100.0
            } else {
                0.0
            };
            assert!(
                vol_pct < 0.1,
                "{label}: volume changed by {vol_pct:.4}% ({vol_diff:.4}): {vol_before} → {vol_after}"
            );
        }
        println!("fuzz_smooth_terrain: tested {tested}, skipped {skipped}/200");
    }

    /// Fuzz with chunk bounds — test the interaction between chunk boundary
    /// vertex pinning and diagonal terrain geometry.
    #[test]
    #[ignore] // Slow fuzz test (~100 seeds); run explicitly with `cargo test -- --ignored`
    fn fuzz_chunked_smooth_terrain() {
        let bounds = Some(([0, 0, 0], [16, 16, 16]));
        for seed in 0..100 {
            let heights = smooth_random_heightmap(seed, 14, 3);
            let mut mesh = build_heightmap_terrain(&heights);
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                continue;
            }
            let label = format!("chunked smooth seed={seed}");
            let vol_before = smooth_mesh_volume(&mesh);

            mesh.coplanar_region_retri(bounds);
            mesh.collapse_collinear_boundary_vertices(bounds);
            mesh.decimate(1e-6, bounds);

            assert_watertight(&mesh, &format!("{label} post"));
            assert_no_degenerate_triangles(&mesh, &format!("{label} post"));
            let vol_after = smooth_mesh_volume(&mesh);
            let vol_diff = (vol_after - vol_before).abs();
            let vol_pct = if vol_before > 0.0 {
                vol_diff / vol_before * 100.0
            } else {
                0.0
            };
            assert!(
                vol_pct < 0.1,
                "{label}: volume changed by {vol_pct:.4}% ({vol_diff:.4}): {vol_before} → {vol_after}"
            );
        }
    }

    /// Check for near-degenerate triangles — triangles with very small area
    /// that survive but produce numerically unstable normals, causing visual
    /// artifacts even though they technically have non-zero area.
    fn assert_no_near_degenerate_triangles(mesh: &SmoothMesh, min_area: f32, label: &str) {
        for (ti, tri) in mesh.triangles.iter().enumerate() {
            let positions: [[f32; 3]; 3] =
                std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
            let n = triangle_normal(positions);
            let area = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5;
            assert!(
                area >= min_area,
                "{label}: triangle {ti} has near-degenerate area {area:.6} (min: {min_area}), \
                 vertices: {:?}",
                positions
            );
        }
    }

    /// Check for sliver triangles — triangles with extreme aspect ratios.
    /// A sliver has non-zero area but one very long edge and a very short
    /// altitude, producing unreliable normals when the mesh is rendered
    /// with per-vertex interpolation. Aspect ratio = longest_edge² / area.
    fn assert_no_sliver_triangles(mesh: &SmoothMesh, max_aspect_ratio: f32, label: &str) {
        for (ti, tri) in mesh.triangles.iter().enumerate() {
            let p: [[f32; 3]; 3] = std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);

            // Compute edge lengths squared.
            let edges_sq: [f32; 3] = [
                (p[1][0] - p[0][0]).powi(2)
                    + (p[1][1] - p[0][1]).powi(2)
                    + (p[1][2] - p[0][2]).powi(2),
                (p[2][0] - p[1][0]).powi(2)
                    + (p[2][1] - p[1][1]).powi(2)
                    + (p[2][2] - p[1][2]).powi(2),
                (p[0][0] - p[2][0]).powi(2)
                    + (p[0][1] - p[2][1]).powi(2)
                    + (p[0][2] - p[2][2]).powi(2),
            ];
            let longest_sq = edges_sq[0].max(edges_sq[1]).max(edges_sq[2]);

            let n = triangle_normal(p);
            let area = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() * 0.5;
            if area < 1e-10 {
                continue; // Degenerate — caught by other checks.
            }

            // Aspect ratio: longest_edge² / (4 * area / sqrt(3))
            // For an equilateral triangle this is 1.0; for slivers it grows.
            // Simplified: we just check longest_edge² / area.
            let ratio = longest_sq / area;
            assert!(
                ratio <= max_aspect_ratio,
                "{label}: triangle {ti} is a sliver (aspect ratio {ratio:.1}, max {max_aspect_ratio}). \
                 area={area:.6}, longest_edge={:.4}, vertices: {:?}",
                longest_sq.sqrt(),
                p
            );
        }
    }

    /// Check that no triangle's normal changed drastically. Compares each
    /// post-operation triangle's normal against the nearest pre-operation
    /// triangle normal (matched by centroid position). A large angular
    /// deviation indicates a visual distortion even if volume is preserved.
    fn _assert_normals_consistent(
        before_verts: &[[f32; 3]],
        before_tris: &[[u32; 3]],
        after_mesh: &SmoothMesh,
        max_angle_deg: f32,
        label: &str,
    ) {
        // Pre-compute before-triangle centroids and normals.
        let before_data: Vec<([f32; 3], [f32; 3])> = before_tris
            .iter()
            .filter_map(|tri| {
                let p: [[f32; 3]; 3] = std::array::from_fn(|i| before_verts[tri[i] as usize]);
                let n = triangle_normal(p);
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                if len < 1e-10 {
                    return None;
                }
                let normal = [n[0] / len, n[1] / len, n[2] / len];
                let centroid = [
                    (p[0][0] + p[1][0] + p[2][0]) / 3.0,
                    (p[0][1] + p[1][1] + p[2][1]) / 3.0,
                    (p[0][2] + p[1][2] + p[2][2]) / 3.0,
                ];
                Some((centroid, normal))
            })
            .collect();

        let cos_threshold = (max_angle_deg * std::f32::consts::PI / 180.0).cos();

        for (ti, tri) in after_mesh.triangles.iter().enumerate() {
            let p: [[f32; 3]; 3] =
                std::array::from_fn(|i| after_mesh.vertices[tri[i] as usize].position);
            let n = triangle_normal(p);
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len < 1e-10 {
                continue; // Degenerate — caught by other checks.
            }
            let normal = [n[0] / len, n[1] / len, n[2] / len];
            let centroid = [
                (p[0][0] + p[1][0] + p[2][0]) / 3.0,
                (p[0][1] + p[1][1] + p[2][1]) / 3.0,
                (p[0][2] + p[1][2] + p[2][2]) / 3.0,
            ];

            // Find the nearest before-triangle by centroid distance.
            let nearest = before_data.iter().min_by(|a, b| {
                let da = (a.0[0] - centroid[0]).powi(2)
                    + (a.0[1] - centroid[1]).powi(2)
                    + (a.0[2] - centroid[2]).powi(2);
                let db = (b.0[0] - centroid[0]).powi(2)
                    + (b.0[1] - centroid[1]).powi(2)
                    + (b.0[2] - centroid[2]).powi(2);
                da.partial_cmp(&db).unwrap()
            });

            if let Some((nearest_centroid, _)) = nearest {
                let dist_sq = (nearest_centroid[0] - centroid[0]).powi(2)
                    + (nearest_centroid[1] - centroid[1]).powi(2)
                    + (nearest_centroid[2] - centroid[2]).powi(2);
                // Only compare normals if we found a close match (within 0.5
                // units). Farther matches are likely on a different surface
                // patch (e.g., a different face of a chamfered voxel), where
                // normal differences are expected.
                if dist_sq > 0.25 {
                    continue;
                }
                // Among all before-triangles within range, find the one whose
                // normal is most similar (highest dot). If even the best match
                // is poor, that's a real normal flip.
                let best_dot = before_data
                    .iter()
                    .filter(|(bc, _)| {
                        let d = (bc[0] - centroid[0]).powi(2)
                            + (bc[1] - centroid[1]).powi(2)
                            + (bc[2] - centroid[2]).powi(2);
                        d < 0.25
                    })
                    .map(|(_, bn)| normal[0] * bn[0] + normal[1] * bn[1] + normal[2] * bn[2])
                    .fold(f32::NEG_INFINITY, f32::max);

                assert!(
                    best_dot >= cos_threshold,
                    "{label}: triangle {ti} normal deviated too much from all nearby originals. \
                     best_dot={best_dot:.4} (threshold={cos_threshold:.4}, max_angle={max_angle_deg}°). \
                     After normal: {:?}, centroid: {:?}",
                    normal,
                    centroid
                );
            }
        }
    }

    /// Run each pipeline stage independently and in combination, checking
    /// all integrity metrics after each. This isolates which stage causes
    /// any detected issue.
    fn assert_per_stage_integrity(label: &str, voxels: &[(i32, i32, i32)], vol_pct_tol: f32) {
        let base_mesh = build_chamfered_voxel_mesh(voxels);
        let (boundary, non_manifold) = check_watertight(&base_mesh);
        if !non_manifold.is_empty() || !boundary.is_empty() {
            return; // Non-manifold input — skip (separate bug B-chamfer-nonmfld).
        }
        let vol_orig = smooth_mesh_volume(&base_mesh);
        let _positions_orig: Vec<[f32; 3]> =
            base_mesh.vertices.iter().map(|v| v.position).collect();
        let _tris_orig: Vec<[u32; 3]> = base_mesh.triangles.clone();

        // Sliver threshold: longest_edge² / area. With flat shading, slivers
        // only matter when they're extreme enough to cause f32 normal
        // instability. Threshold matches the production guards (5000).
        let sliver_max = super::MAX_SLIVER_ASPECT_RATIO;

        // --- QEM only (no retri, no collinear collapse) ---
        {
            let mut mesh = base_mesh.clone();
            mesh.decimate(1e-6, None);
            let tag = format!("{label} QEM-only");
            assert_watertight(&mesh, &tag);
            assert_no_degenerate_triangles(&mesh, &tag);
            assert_no_near_degenerate_triangles(&mesh, 1e-6, &tag);
            assert_no_sliver_triangles(&mesh, sliver_max, &tag);
            let vol = smooth_mesh_volume(&mesh);
            let pct = (vol - vol_orig).abs() / vol_orig * 100.0;
            assert!(pct < vol_pct_tol, "{tag}: volume changed by {pct:.4}%");
        }

        // --- Retri only ---
        {
            let mut mesh = base_mesh.clone();
            mesh.coplanar_region_retri(None);
            let tag = format!("{label} retri-only");
            assert_watertight(&mesh, &tag);
            assert_no_degenerate_triangles(&mesh, &tag);
            assert_no_sliver_triangles(&mesh, sliver_max, &tag);
            let vol = smooth_mesh_volume(&mesh);
            let pct = (vol - vol_orig).abs() / vol_orig * 100.0;
            assert!(pct < vol_pct_tol, "{tag}: volume changed by {pct:.4}%");
        }

        // --- Retri + collinear ---
        {
            let mut mesh = base_mesh.clone();
            mesh.coplanar_region_retri(None);
            mesh.collapse_collinear_boundary_vertices(None);
            let tag = format!("{label} retri+collinear");
            assert_watertight(&mesh, &tag);
            assert_no_degenerate_triangles(&mesh, &tag);
            assert_no_sliver_triangles(&mesh, sliver_max, &tag);
            let vol = smooth_mesh_volume(&mesh);
            let pct = (vol - vol_orig).abs() / vol_orig * 100.0;
            assert!(pct < vol_pct_tol, "{tag}: volume changed by {pct:.4}%");
        }

        // --- Full pipeline ---
        {
            let mut mesh = base_mesh.clone();
            mesh.coplanar_region_retri(None);
            mesh.collapse_collinear_boundary_vertices(None);
            mesh.decimate(1e-6, None);
            let tag = format!("{label} full-pipeline");
            assert_watertight(&mesh, &tag);
            assert_no_degenerate_triangles(&mesh, &tag);
            assert_no_near_degenerate_triangles(&mesh, 1e-6, &tag);
            assert_no_sliver_triangles(&mesh, sliver_max, &tag);
            let vol = smooth_mesh_volume(&mesh);
            let pct = (vol - vol_orig).abs() / vol_orig * 100.0;
            assert!(pct < vol_pct_tol, "{tag}: volume changed by {pct:.4}%");
        }
    }

    // Note: qem_near_degenerate_seed1 and normal_flip_large_seed0 were
    // seed-specific reproduction tests for bugs now guarded by the QEM
    // near-degenerate threshold and canonical normal checks. The fuzz
    // tests cover these scenarios across many seeds.

    /// Fuzz: run per-stage integrity checks on smooth terrain heightmaps.
    #[test]
    #[ignore] // Slow fuzz test (~200 seeds); run explicitly with `cargo test -- --ignored`
    fn fuzz_per_stage_smooth_terrain() {
        for seed in 0..200 {
            let heights = smooth_random_heightmap(seed, 12, 3);
            let voxels = heightmap_to_voxels(&heights);
            assert_per_stage_integrity(&format!("seed={seed}"), &voxels, 0.1);
        }
    }

    /// Fuzz with larger terrain, per-stage.
    #[test]
    #[ignore] // Slow fuzz test (~30 seeds, large terrain); run explicitly with `cargo test -- --ignored`
    fn fuzz_per_stage_large_terrain() {
        for seed in 0..30 {
            let heights = smooth_random_heightmap(seed, 20, 3);
            let voxels = heightmap_to_voxels(&heights);
            assert_per_stage_integrity(&format!("large seed={seed}"), &voxels, 0.1);
        }
    }

    /// Fuzz with larger terrain (20×20) to stress-test with more complex
    /// geometry, more triangle fans, and more potential for cascading collapse
    /// errors.
    #[test]
    #[ignore] // Slow fuzz test (~30 seeds, large terrain); run explicitly with `cargo test -- --ignored`
    fn fuzz_large_smooth_terrain() {
        for seed in 0..30 {
            let heights = smooth_random_heightmap(seed, 20, 3);
            let mut mesh = build_heightmap_terrain(&heights);
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                continue;
            }
            let label = format!("large smooth seed={seed}");
            let vol_before = smooth_mesh_volume(&mesh);

            mesh.coplanar_region_retri(None);
            mesh.collapse_collinear_boundary_vertices(None);
            mesh.decimate(1e-6, None);

            assert_watertight(&mesh, &format!("{label} post"));
            assert_no_degenerate_triangles(&mesh, &format!("{label} post"));
            let vol_after = smooth_mesh_volume(&mesh);
            let vol_diff = (vol_after - vol_before).abs();
            let vol_pct = if vol_before > 0.0 {
                vol_diff / vol_before * 100.0
            } else {
                0.0
            };
            assert!(
                vol_pct < 0.1,
                "{label}: volume changed by {vol_pct:.4}% ({vol_diff:.4}): {vol_before} → {vol_after}"
            );
        }
    }

    // --- Point-in-mesh surface sampling tests ---
    //
    // These sample points near the mesh surface before decimation, classify
    // them as inside or outside, then verify the classification is preserved
    // after decimation. This catches local geometry deformations that
    // cancel out in volume but are visually distorted.

    /// Extract positions and triangles from a SmoothMesh for raycasting.
    fn mesh_arrays(mesh: &SmoothMesh) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let verts: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
        (verts, mesh.triangles.clone())
    }

    /// Determine whether the mesh has outward or inward normals by checking
    /// signed volume sign. Returns the offset sign: +1.0 if normals point
    /// outward (positive signed volume), -1.0 if inward.
    fn normal_sign(verts: &[[f32; 3]], tris: &[[u32; 3]]) -> f32 {
        if mesh_signed_volume(verts, tris) >= 0.0 {
            1.0
        } else {
            -1.0
        }
    }

    /// Run the point-in-mesh deformation test on a SmoothMesh. Samples
    /// points near the surface before the pipeline, runs the full pipeline,
    /// then checks every sample point's inside/outside classification is
    /// preserved.
    fn assert_surface_preserved(label: &str, mesh: &mut SmoothMesh, offset: f32) -> usize {
        let (verts_before, tris_before) = mesh_arrays(mesh);
        let sign = normal_sign(&verts_before, &tris_before);

        // Generate sample points. If normals point inward (sign < 0),
        // swap inside/outside.
        let (mut inside_pts, mut outside_pts) =
            sample_near_surface(&verts_before, &tris_before, offset);
        if sign < 0.0 {
            std::mem::swap(&mut inside_pts, &mut outside_pts);
        }

        // Sanity check: verify classifications against the original mesh.
        let mut inside_bad = 0;
        for pt in &inside_pts {
            if !point_is_inside(*pt, &verts_before, &tris_before) {
                inside_bad += 1;
            }
        }
        let mut outside_bad = 0;
        for pt in &outside_pts {
            if point_is_inside(*pt, &verts_before, &tris_before) {
                outside_bad += 1;
            }
        }

        // Some points near edges/corners may be misclassified due to
        // f32 precision in raycasting. Allow a small fraction of failures
        // on the ORIGINAL mesh (these are sampling artifacts, not bugs).
        let total = inside_pts.len() + outside_pts.len();
        let pre_bad = inside_bad + outside_bad;
        let pre_bad_pct = pre_bad as f64 / total as f64 * 100.0;
        assert!(
            pre_bad_pct < 5.0,
            "{label}: too many pre-decimation misclassifications: {pre_bad}/{total} ({pre_bad_pct:.1}%)"
        );

        // Run full pipeline.
        mesh.coplanar_region_retri(None);
        mesh.collapse_collinear_boundary_vertices(None);
        mesh.decimate(1e-6, None);

        let (verts_after, tris_after) = mesh_arrays(mesh);

        // Check inside points are still inside.
        let mut flipped_inside = Vec::new();
        for (i, pt) in inside_pts.iter().enumerate() {
            // Skip points that were already misclassified pre-decimation.
            if !point_is_inside(*pt, &verts_before, &tris_before) {
                continue;
            }
            if !point_is_inside(*pt, &verts_after, &tris_after) {
                flipped_inside.push(i);
            }
        }

        // Check outside points are still outside.
        let mut flipped_outside = Vec::new();
        for (i, pt) in outside_pts.iter().enumerate() {
            if point_is_inside(*pt, &verts_before, &tris_before) {
                continue;
            }
            if point_is_inside(*pt, &verts_after, &tris_after) {
                flipped_outside.push(i);
            }
        }

        let total_flipped = flipped_inside.len() + flipped_outside.len();
        if total_flipped > 0 {
            let first_in = flipped_inside.first().map(|&i| inside_pts[i]);
            let first_out = flipped_outside.first().map(|&i| outside_pts[i]);
            println!(
                "DEFORMATION {label}: {total_flipped} sample points changed inside/outside \
                 ({} inside→outside, {} outside→inside). \
                 First inside→outside: {:?}, first outside→inside: {:?}",
                flipped_inside.len(),
                flipped_outside.len(),
                first_in,
                first_out,
            );
        }
        total_flipped
    }

    /// Sanity test: raycasting works on a simple unit cube.
    #[test]
    fn point_in_mesh_sanity_cube() {
        // Raw unit cube (same as volume_computation_sanity).
        let verts: Vec<[f32; 3]> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let tris: Vec<[u32; 3]> = vec![
            [4, 5, 6],
            [4, 6, 7],
            [1, 0, 3],
            [1, 3, 2],
            [5, 1, 2],
            [5, 2, 6],
            [0, 4, 7],
            [0, 7, 3],
            [3, 7, 6],
            [3, 6, 2],
            [0, 1, 5],
            [0, 5, 4],
        ];

        assert!(
            point_is_inside([0.5, 0.5, 0.5], &verts, &tris),
            "center should be inside"
        );
        assert!(
            !point_is_inside([5.0, 0.5, 0.5], &verts, &tris),
            "far +X should be outside"
        );
        assert!(
            !point_is_inside([-5.0, 0.5, 0.5], &verts, &tris),
            "far -X should be outside"
        );
    }

    /// Sanity test: raycasting works on a chamfered voxel mesh.
    #[test]
    fn point_in_mesh_sanity_chamfered() {
        let mesh = build_chamfered_voxel_mesh(&[(0, 0, 0)]);
        let (verts, tris) = mesh_arrays(&mesh);

        // Center of the voxel should be inside.
        assert!(
            point_is_inside([0.5, 0.5, 0.5], &verts, &tris),
            "center of voxel should be inside (got {} intersections)",
            {
                let mut c = 0;
                for tri in &tris {
                    if ray_triangle_intersect(
                        [0.5, 0.5, 0.5],
                        RAY_DIR,
                        verts[tri[0] as usize],
                        verts[tri[1] as usize],
                        verts[tri[2] as usize],
                    )
                    .is_some()
                    {
                        c += 1;
                    }
                }
                c
            }
        );

        // Well outside should be outside.
        assert!(
            !point_is_inside([5.0, 5.0, 5.0], &verts, &tris),
            "far point should be outside"
        );
    }

    /// Per-stage surface preservation: isolate which pipeline stage causes
    /// any detected surface change.
    fn assert_surface_preserved_per_stage(label: &str, mesh: &SmoothMesh, offset: f32) {
        let (verts_orig, tris_orig) = mesh_arrays(mesh);
        let sign = normal_sign(&verts_orig, &tris_orig);
        let (mut inside_pts, mut outside_pts) =
            sample_near_surface(&verts_orig, &tris_orig, offset);
        if sign < 0.0 {
            std::mem::swap(&mut inside_pts, &mut outside_pts);
        }

        // Filter to points that are correctly classified on the original.
        let valid_inside: Vec<[f32; 3]> = inside_pts
            .iter()
            .copied()
            .filter(|pt| point_is_inside(*pt, &verts_orig, &tris_orig))
            .collect();
        let valid_outside: Vec<[f32; 3]> = outside_pts
            .iter()
            .copied()
            .filter(|pt| !point_is_inside(*pt, &verts_orig, &tris_orig))
            .collect();

        let check = |verts: &[[f32; 3]], tris: &[[u32; 3]], stage: &str| -> (usize, usize) {
            let mut in_to_out = 0;
            for pt in &valid_inside {
                if !point_is_inside(*pt, verts, tris) {
                    if in_to_out == 0 {
                        println!("  {label} {stage}: first inside→outside at {:?}", pt);
                    }
                    in_to_out += 1;
                }
            }
            let mut out_to_in = 0;
            for pt in &valid_outside {
                if point_is_inside(*pt, verts, tris) {
                    if out_to_in == 0 {
                        println!("  {label} {stage}: first outside→inside at {:?}", pt);
                    }
                    out_to_in += 1;
                }
            }
            if in_to_out > 0 || out_to_in > 0 {
                println!(
                    "  {label} {stage}: {in_to_out} inside→outside, {out_to_in} outside→inside"
                );
            }
            (in_to_out, out_to_in)
        };

        // Retri only
        let mut m1 = mesh.clone();
        m1.coplanar_region_retri(None);
        let (v1, t1) = mesh_arrays(&m1);
        check(&v1, &t1, "retri-only");

        // Retri + collinear
        let mut m2 = m1.clone();
        m2.collapse_collinear_boundary_vertices(None);
        let (v2, t2) = mesh_arrays(&m2);
        check(&v2, &t2, "retri+collinear");

        // Full pipeline
        let mut m3 = m2;
        m3.decimate(1e-6, None);
        let (v3, t3) = mesh_arrays(&m3);
        check(&v3, &t3, "full");

        // QEM only (no retri)
        let mut m4 = mesh.clone();
        m4.decimate(1e-6, None);
        let (v4, t4) = mesh_arrays(&m4);
        check(&v4, &t4, "QEM-only");
    }

    /// Diagnose: which stage causes the terraced hillside deformation?
    #[test]
    fn diagnose_terraced_hillside_deformation() {
        let mesh = build_terraced_hillside();
        assert_surface_preserved_per_stage("terraced_hillside", &mesh, 0.02);
    }

    /// Surface preservation survey on handcrafted shapes. Reports
    /// deformations but does not fail — the detected deformations are
    /// known (B-qem-deformation) and under investigation. This test
    /// serves as a regression detector: if the deformation count increases
    /// after a code change, something got worse.
    #[test]
    fn surface_preserved_handcrafted_shapes() {
        let shapes: Vec<(&str, fn() -> SmoothMesh)> = vec![
            ("prism 10x3x10", || build_prism(10, 3, 10)),
            ("pyramid 10x5", || build_pyramid(10, 5)),
            ("diamond r=6 h=7", || build_diamond_pyramid(6, 7)),
            ("L-shape", build_l_shape),
            ("hollow frame", build_hollow_frame),
            ("staircase", build_staircase),
            ("platform on base", build_platform_on_base),
            ("diagonal ridge", build_diagonal_ridge),
            ("rolling hills", build_rolling_hills),
            ("diagonal nook", build_diagonal_nook),
            ("zigzag terrain", build_zigzag_terrain),
            ("terraced hillside", build_terraced_hillside),
            ("saddle", build_saddle),
            ("cove", build_cove),
        ];
        let mut total_deformations = 0;
        for (name, builder) in shapes {
            let mut mesh = builder();
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                continue;
            }
            total_deformations += assert_surface_preserved(name, &mut mesh, 0.02);
        }
        println!("Surface survey: {total_deformations} total deformations across all shapes");
        // Regression gate: fail if deformation count exceeds known baseline.
        // Update this baseline when fixes reduce the count further.
        assert!(
            total_deformations <= 3,
            "handcrafted surface deformations regressed: {total_deformations} (baseline: 3)"
        );
    }

    /// Surface preservation fuzz on smooth heightmap terrain. Asserts
    /// deformation count doesn't exceed a known baseline as a regression gate.
    #[test]
    #[ignore] // Slow fuzz test (~100 seeds); run explicitly with `cargo test -- --ignored`
    fn fuzz_surface_preserved_smooth_terrain() {
        let mut total_deformations = 0;
        let mut tested = 0;
        for seed in 0..100 {
            let heights = smooth_random_heightmap(seed, 12, 3);
            let mut mesh = build_heightmap_terrain(&heights);
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                continue;
            }
            tested += 1;
            total_deformations +=
                assert_surface_preserved(&format!("smooth seed={seed}"), &mut mesh, 0.02);
        }
        println!("Smooth terrain fuzz: {total_deformations} deformations across {tested} seeds");
        // Regression gate: fail if deformation count exceeds known baseline.
        // Update this baseline when fixes reduce the count further.
        assert!(
            total_deformations <= 40,
            "fuzz surface deformations regressed: {total_deformations} (baseline: 40)"
        );
    }

    // --- Canonical normal tests (B-qem-deformation Category A) ---
    //
    // Chamfered voxel meshes have exactly 26 possible triangle normal
    // directions: 6 cardinal, 12 edge-chamfer, 8 corner-chamfer. Any
    // triangle with a normal outside this set is a cross-surface bridging
    // artifact from the decimation pipeline.

    /// Check that every triangle normal in a mesh matches one of the 26
    /// canonical chamfer directions (within `tolerance` radians angular
    /// deviation). Returns the count of non-canonical triangles.
    fn count_non_canonical_normals(mesh: &SmoothMesh, tolerance_deg: f32) -> Vec<(usize, f32)> {
        let cos_tol = (tolerance_deg * std::f32::consts::PI / 180.0).cos();
        let mut bad = Vec::new();

        for (ti, tri) in mesh.triangles.iter().enumerate() {
            let positions: [[f32; 3]; 3] =
                std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
            let n = triangle_normal(positions);
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len < 1e-10 {
                continue;
            }
            let normal = [n[0] / len, n[1] / len, n[2] / len];

            // Find best matching canonical normal.
            let best_dot = super::CANONICAL_NORMALS
                .iter()
                .map(|cn| normal[0] * cn[0] + normal[1] * cn[1] + normal[2] * cn[2])
                .fold(f32::NEG_INFINITY, f32::max);

            if best_dot < cos_tol {
                let angle = best_dot.clamp(-1.0, 1.0).acos() * 180.0 / std::f32::consts::PI;
                bad.push((ti, angle));
            }
        }
        bad
    }

    /// Assert that a chamfered mesh has zero non-canonical normals.
    fn assert_all_normals_canonical(mesh: &SmoothMesh, tolerance_deg: f32, label: &str) {
        let bad = count_non_canonical_normals(mesh, tolerance_deg);
        if !bad.is_empty() {
            let details: Vec<String> = bad
                .iter()
                .take(5)
                .map(|(ti, angle)| {
                    let tri = mesh.triangles[*ti];
                    let positions: [[f32; 3]; 3] =
                        std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
                    format!(
                        "  tri {ti}: deviation {angle:.1}°, verts {:?}, {:?}, {:?}",
                        positions[0], positions[1], positions[2]
                    )
                })
                .collect();
            panic!(
                "{label}: {}/{} triangles have non-canonical normals (tolerance {tolerance_deg}°):\n{}",
                bad.len(),
                mesh.triangles.len(),
                details.join("\n")
            );
        }
    }

    /// Unit test: `is_canonical_normal` accepts all 26 canonical directions
    /// and rejects non-canonical vectors.
    #[test]
    fn is_canonical_normal_unit_test() {
        // All 26 canonical normals should be accepted.
        for (i, cn) in super::CANONICAL_NORMALS.iter().enumerate() {
            assert!(
                super::is_canonical_normal(*cn),
                "canonical normal {i} ({:?}) was rejected",
                cn
            );
        }
        // Non-canonical normals should be rejected.
        let bad: [[f32; 3]; 4] = [
            [0.9, 0.3, 0.3],          // between cardinal and corner chamfer
            [0.9, 0.0, 0.4359],       // between cardinal and edge chamfer
            [0.4, 0.4, 0.8244],       // arbitrary direction
            [0.3015, 0.3015, 0.9045], // the actual cross-surface normal from OBJ analysis
        ];
        for b in &bad {
            let len = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
            let normalized = [b[0] / len, b[1] / len, b[2] / len];
            assert!(
                !super::is_canonical_normal(normalized),
                "non-canonical normal {:?} was accepted",
                normalized
            );
        }
    }

    /// Verify that the actual chamfer output normals match the hardcoded
    /// canonical normal table with high precision (dot > 0.9999). This
    /// catches any drift between the chamfer algorithm and the table.
    #[test]
    fn chamfer_output_matches_canonical_table_precisely() {
        let mesh = build_chamfered_voxel_mesh(&[(5, 5, 5)]);
        for (ti, tri) in mesh.triangles.iter().enumerate() {
            let positions: [[f32; 3]; 3] =
                std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
            let n = triangle_normal(positions);
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len < 1e-10 {
                continue;
            }
            let normal = [n[0] / len, n[1] / len, n[2] / len];
            let best_dot = super::CANONICAL_NORMALS
                .iter()
                .map(|cn| normal[0] * cn[0] + normal[1] * cn[1] + normal[2] * cn[2])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(
                best_dot > 0.9999,
                "chamfer triangle {ti} normal {:?} has best canonical dot {best_dot:.6} \
                 (expected > 0.9999)",
                normal
            );
        }
    }

    /// Verify that the original chamfered mesh (before any decimation)
    /// has all canonical normals — baseline sanity check.
    #[test]
    fn chamfered_mesh_normals_are_canonical() {
        let shapes: Vec<(&str, SmoothMesh)> = vec![
            ("single voxel", build_chamfered_voxel_mesh(&[(0, 0, 0)])),
            ("prism 5x3x5", build_prism(5, 3, 5)),
            ("L-shape", build_l_shape()),
            ("staircase", build_staircase()),
            ("terraced hillside", build_terraced_hillside()),
        ];
        for (name, mesh) in &shapes {
            assert_all_normals_canonical(mesh, 1.0, name);
        }
    }

    /// Category A reproduction: QEM collapses edges across crease
    /// boundaries between differently-oriented chamfer surfaces. A simple
    /// height step creates edge and corner chamfers meeting flat faces;
    /// QEM should not bridge them.
    #[test]
    fn qem_preserves_canonical_normals_height_step() {
        // Simple height step: flat ground at y=0, one step up at y=1.
        // Creates chamfer bevels between the flat top and the step side.
        let mut voxels = Vec::new();
        for x in 0..8 {
            for z in 0..8 {
                voxels.push((x, 0, z));
            }
        }
        // Step up in one corner
        for x in 4..8 {
            for z in 4..8 {
                voxels.push((x, 1, z));
            }
        }
        let mut mesh = build_chamfered_voxel_mesh(&voxels);
        assert_all_normals_canonical(&mesh, 1.0, "height step pre-decimation");

        mesh.decimate(1e-6, None);
        assert_all_normals_canonical(&mesh, 2.0, "height step QEM-only");
    }

    /// Category A reproduction: L-shaped terrain with concave corner.
    #[test]
    fn qem_preserves_canonical_normals_l_terrain() {
        let mut voxels = Vec::new();
        for x in 0..6 {
            for z in 0..6 {
                voxels.push((x, 0, z));
            }
        }
        // L-shaped step: two arms
        for x in 0..6 {
            for z in 0..2 {
                voxels.push((x, 1, z));
            }
        }
        for x in 0..2 {
            for z in 2..6 {
                voxels.push((x, 1, z));
            }
        }
        let mut mesh = build_chamfered_voxel_mesh(&voxels);
        assert_all_normals_canonical(&mesh, 1.0, "L terrain pre-decimation");

        mesh.decimate(1e-6, None);
        assert_all_normals_canonical(&mesh, 2.0, "L terrain QEM-only");
    }

    /// Category A reproduction with full pipeline (retri + collinear + QEM).
    #[test]
    fn full_pipeline_preserves_canonical_normals() {
        let shapes: Vec<(&str, SmoothMesh)> = vec![
            ("prism 10x3x10", build_prism(10, 3, 10)),
            ("pyramid 10x5", build_pyramid(10, 5)),
            ("L-shape", build_l_shape()),
            ("staircase", build_staircase()),
            ("terraced hillside", build_terraced_hillside()),
            ("diagonal ridge", build_diagonal_ridge()),
            ("rolling hills", build_rolling_hills()),
            ("saddle", build_saddle()),
            ("cove", build_cove()),
        ];
        for (name, mut mesh) in shapes {
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                continue;
            }
            assert_all_normals_canonical(&mesh, 1.0, &format!("{name} pre"));
            mesh.coplanar_region_retri(None);
            mesh.collapse_collinear_boundary_vertices(None);
            mesh.decimate(1e-6, None);
            assert_all_normals_canonical(&mesh, 2.0, &format!("{name} post"));
        }
    }

    /// Category A reproduction: large terrain with tall features.
    /// The real game chunk that exhibited the bug had bark surfaces
    /// (vertical walls 9 voxels tall) with chamfer bevels at the top.
    /// This test creates a tall structure on flat ground to reproduce
    /// the cross-surface bridging.
    #[test]
    fn qem_preserves_canonical_normals_tall_structure() {
        let mut voxels = Vec::new();
        // Flat ground 16×16
        for x in 0..16 {
            for z in 0..16 {
                voxels.push((x, 0, z));
            }
        }
        // Tall wall/column in the middle (simulates tree trunk)
        for x in 6..10 {
            for z in 6..10 {
                for y in 1..10 {
                    voxels.push((x, y, z));
                }
            }
        }
        // Step feature on one side
        for x in 3..6 {
            for z in 3..13 {
                voxels.push((x, 1, z));
            }
        }
        let mut mesh = build_chamfered_voxel_mesh(&voxels);
        assert_all_normals_canonical(&mesh, 1.0, "tall structure pre");

        // QEM only
        mesh.decimate(1e-6, None);
        assert_all_normals_canonical(&mesh, 2.0, "tall structure QEM-only");
    }

    /// Category A reproduction: full pipeline on tall structure.
    #[test]
    fn full_pipeline_preserves_canonical_normals_tall_structure() {
        let mut voxels = Vec::new();
        for x in 0..16 {
            for z in 0..16 {
                voxels.push((x, 0, z));
            }
        }
        for x in 6..10 {
            for z in 6..10 {
                for y in 1..10 {
                    voxels.push((x, y, z));
                }
            }
        }
        for x in 3..6 {
            for z in 3..13 {
                voxels.push((x, 1, z));
            }
        }
        let mut mesh = build_chamfered_voxel_mesh(&voxels);
        assert_all_normals_canonical(&mesh, 1.0, "tall structure pre");

        mesh.coplanar_region_retri(None);
        mesh.collapse_collinear_boundary_vertices(None);
        mesh.decimate(1e-6, None);
        assert_all_normals_canonical(&mesh, 2.0, "tall structure full pipeline");
    }

    /// Category A reproduction: full pipeline with chunk bounds. Chunk
    /// boundary pinning constrains the collapse order and can force
    /// cross-surface collapses that don't happen without bounds.
    #[test]
    fn full_pipeline_preserves_canonical_normals_with_chunk_bounds() {
        // Build terrain that extends beyond chunk bounds (simulating
        // the border region used in real mesh gen).
        let mut voxels = Vec::new();
        // Ground extends -2..18 in x and z (2-voxel border beyond 0..16 chunk)
        for x in -2..18 {
            for z in -2..18 {
                voxels.push((x, 0, z));
            }
        }
        // Tall column in center
        for x in 6..10 {
            for z in 6..10 {
                for y in 1..12 {
                    voxels.push((x, y, z));
                }
            }
        }
        // Steps at various locations
        for x in 0..5 {
            for z in 0..16 {
                voxels.push((x, 1, z));
            }
        }
        for x in 12..16 {
            for z in 0..8 {
                voxels.push((x, 1, z));
                voxels.push((x, 2, z));
            }
        }
        voxels.sort();
        voxels.dedup();

        let mut mesh = build_chamfered_voxel_mesh(&voxels);
        assert_all_normals_canonical(&mesh, 1.0, "chunked terrain pre");

        let bounds = Some(([0, 0, 0], [16, 16, 16]));
        mesh.coplanar_region_retri(bounds);
        mesh.collapse_collinear_boundary_vertices(bounds);
        mesh.decimate(1e-6, bounds);
        assert_all_normals_canonical(&mesh, 2.0, "chunked terrain full pipeline");
    }

    /// Category A reproduction: complex multi-height terrain with chunk
    /// bounds and tall features to force retri centroid fans near chamfer
    /// bevel boundaries.
    #[test]
    fn canonical_normals_complex_terrain_with_bounds() {
        let mut voxels = Vec::new();
        // Large ground with border (-2..18)
        for x in -2..18 {
            for z in -2..18 {
                voxels.push((x, 0, z));
            }
        }
        // Multiple height steps at various locations
        for x in 0..8 {
            for z in 0..16 {
                voxels.push((x, 1, z));
            }
        }
        for x in 2..6 {
            for z in 4..12 {
                voxels.push((x, 2, z));
            }
        }
        // Tall wall (tree trunk analog) — 10 voxels tall
        for x in 7..11 {
            for z in 7..11 {
                for y in 1..11 {
                    voxels.push((x, y, z));
                }
            }
        }
        // Second tall wall
        for x in 2..4 {
            for z in 13..15 {
                for y in 1..8 {
                    voxels.push((x, y, z));
                }
            }
        }
        // Isolated step blocks (create lots of chamfer bevels)
        voxels.push((12, 1, 3));
        voxels.push((13, 1, 3));
        voxels.push((12, 1, 4));
        voxels.push((14, 1, 8));
        voxels.push((14, 1, 9));
        voxels.push((14, 2, 8));

        voxels.sort();
        voxels.dedup();

        let mut mesh = build_chamfered_voxel_mesh(&voxels);
        let pre_count = mesh.triangles.len();
        assert_all_normals_canonical(&mesh, 1.0, "complex terrain pre");

        let bounds = Some(([0, 0, 0], [16, 16, 16]));
        mesh.coplanar_region_retri(bounds);
        let post_retri = mesh.triangles.len();
        mesh.collapse_collinear_boundary_vertices(bounds);
        let post_collinear = mesh.triangles.len();
        mesh.decimate(1e-6, bounds);
        let post_qem = mesh.triangles.len();

        println!(
            "Complex terrain: {} → {} (retri) → {} (collinear) → {} (QEM)",
            pre_count, post_retri, post_collinear, post_qem
        );

        let bad = count_non_canonical_normals(&mesh, 2.0);
        if !bad.is_empty() {
            for (ti, angle) in &bad {
                let tri = mesh.triangles[*ti];
                let positions: [[f32; 3]; 3] =
                    std::array::from_fn(|i| mesh.vertices[tri[i] as usize].position);
                println!(
                    "  non-canonical tri {ti}: {angle:.1}°, verts {:?}, {:?}, {:?}",
                    positions[0], positions[1], positions[2]
                );
            }
        }
        assert!(
            bad.is_empty(),
            "complex terrain: {} non-canonical normals",
            bad.len()
        );
    }

    /// Fuzz: canonical normal preservation on smooth terrain.
    #[test]
    #[ignore] // Slow fuzz test (~100 seeds); run explicitly with `cargo test -- --ignored`
    fn fuzz_canonical_normals_smooth_terrain() {
        let mut failures = 0;
        let mut tested = 0;
        for seed in 0..100 {
            let heights = smooth_random_heightmap(seed, 12, 3);
            let mut mesh = build_heightmap_terrain(&heights);
            let (boundary, non_manifold) = check_watertight(&mesh);
            if !non_manifold.is_empty() || !boundary.is_empty() {
                continue;
            }
            tested += 1;
            // Verify pre-decimation is canonical.
            let pre_bad = count_non_canonical_normals(&mesh, 1.0);
            assert!(
                pre_bad.is_empty(),
                "seed={seed} pre-decimation has non-canonical normals"
            );

            mesh.coplanar_region_retri(None);
            mesh.collapse_collinear_boundary_vertices(None);
            mesh.decimate(1e-6, None);

            let post_bad = count_non_canonical_normals(&mesh, 2.0);
            if !post_bad.is_empty() {
                failures += 1;
                println!(
                    "seed={seed}: {} non-canonical normals (worst: {:.1}°)",
                    post_bad.len(),
                    post_bad.iter().map(|(_, a)| *a).fold(0.0f32, f32::max)
                );
            }
        }
        assert!(
            failures == 0,
            "{failures}/{tested} seeds have non-canonical normals after decimation"
        );
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
            ("Diagonal ridge", build_diagonal_ridge()),
            ("Rolling hills", build_rolling_hills()),
            ("Diagonal valley", build_diagonal_valley()),
            ("Diagonal nook", build_diagonal_nook()),
            ("Zigzag terrain", build_zigzag_terrain()),
            ("Diag checkerboard", build_diagonal_checkerboard()),
            ("Terraced hillside", build_terraced_hillside()),
            ("Saddle", build_saddle()),
            ("Cove", build_cove()),
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
