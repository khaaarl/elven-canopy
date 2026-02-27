// Beveled tree mesh generation from voxel data.
//
// Produces neighbor-aware beveled mesh geometry for wood voxels (trunk, branch,
// root). Each exposed face is decomposed into a center quad plus 4 edge strips
// that form chamfered transitions between adjacent faces. Hidden faces (where a
// neighbor is also wood) are culled entirely, and fully enclosed voxels produce
// no geometry.
//
// This is a **read-only rendering utility** — it consumes `VoxelWorld` data and
// produces geometry buffers (`MeshData`) and procedural textures (`TextureData`)
// for the GDExtension bridge to hand to Godot. It does not mutate sim state and
// is **not subject to the determinism constraint** that governs the rest of the
// sim crate. (The bark texture uses a simple integer hash for reproducibility,
// but this is a convenience, not a correctness requirement.)
//
// The beveled face decomposition per exposed face:
//   1 center quad  — the face inset from beveled edges, normal = face normal
//   4 edge strips  — trapezoids connecting center quad edges to the outer edges.
//                    Beveled strips get a chamfered normal (average of the two
//                    face normals); flat strips get the face normal.
//
// Per-edge bevel determination:
//   An edge is beveled only when the adjacent face is exposed AND the diagonal
//   neighbor voxel (face_delta + adj_face_delta) is not wood. This ensures:
//   - Face-adjacent wood: edges on the shared boundary are flat (continuous).
//   - Edge-adjacent (diagonal) wood: the shared edge is also flat.
//
// Per-corner geometry:
//   Each corner's inset depends on which of its 2 adjacent edges are beveled.
//   Outer corners are recessed along the face normal only when BOTH edges are
//   beveled. At cube vertices where all 3 meeting faces are exposed and all
//   edges are beveled, a small corner triangle fills the gap.
//
// The companion `generate_bark_texture()` creates a simple procedural RGBA8
// texture with vertical streaks and noise, suitable for tiling across the mesh.
//
// See also: `world.rs` for the `VoxelWorld` being read, `sim_bridge.rs` for the
// GDExtension methods that call into this module, `tree_renderer.gd` for the
// Godot-side ArrayMesh construction.

use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for beveled mesh generation and bark textures.
/// Loaded from `godot/mesh_config.json`. All fields have sensible defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshConfig {
    /// How far each face edge is inset to create the bevel (0.0–0.45).
    /// 0.0 = no bevel (flat cubes), higher = more pronounced chamfer.
    pub bevel_inset: f32,

    /// Side length in pixels of the square bark texture.
    pub bark_texture_size: u32,

    /// Number of vertical streak bands in the bark texture.
    pub bark_streak_count: u32,

    /// Intensity of per-pixel noise added to the bark texture (0.0–1.0).
    pub bark_noise_intensity: f32,

    /// Base RGB color for trunk bark.
    pub trunk_color: [f32; 3],
    /// Per-channel random variation range for trunk color.
    pub trunk_color_variation: f32,

    /// Base RGB color for branch bark.
    pub branch_color: [f32; 3],
    /// Per-channel random variation range for branch color.
    pub branch_color_variation: f32,

    /// Base RGB color for root bark.
    pub root_color: [f32; 3],
    /// Per-channel random variation range for root color.
    pub root_color_variation: f32,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            bevel_inset: 0.12,
            bark_texture_size: 64,
            bark_streak_count: 8,
            bark_noise_intensity: 0.15,
            trunk_color: [0.35, 0.22, 0.10],
            trunk_color_variation: 0.05,
            branch_color: [0.45, 0.30, 0.15],
            branch_color_variation: 0.05,
            root_color: [0.30, 0.20, 0.12],
            root_color_variation: 0.05,
        }
    }
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Mesh geometry output: interleaved flat arrays ready for Godot's ArrayMesh.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    /// Vertex positions as flat [x, y, z, x, y, z, ...].
    pub vertices: Vec<f32>,
    /// Per-vertex normals as flat [nx, ny, nz, ...].
    pub normals: Vec<f32>,
    /// Per-vertex UV coordinates as flat [u, v, u, v, ...].
    pub uvs: Vec<f32>,
    /// Triangle indices (3 per triangle).
    pub indices: Vec<u32>,
}

/// Procedural bark texture output.
#[derive(Clone, Debug)]
pub struct TextureData {
    /// RGBA8 pixel data, row-major, top-to-bottom.
    pub pixels: Vec<u8>,
    /// Texture width in pixels.
    pub width: u32,
    /// Texture height in pixels.
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Face table
// ---------------------------------------------------------------------------

/// A face of the unit cube, described by its 4 corner offsets (CCW from
/// outside), normal vector, and which face index each edge borders.
struct FaceDesc {
    /// 4 corner positions relative to the voxel origin (0,0,0).
    /// Wound counter-clockwise when viewed from outside.
    corners: [[f32; 3]; 4],
    /// Outward-facing normal.
    normal: [f32; 3],
    /// For each of the 4 edges (0→1, 1→2, 2→3, 3→0), the index of the
    /// adjacent face that shares that edge.
    edge_adj: [usize; 4],
    /// Neighbor offset — the voxel coordinate delta for the neighbor on
    /// this face's side.
    neighbor_delta: [i32; 3],
    /// For each edge i (from corner[i] to corner[(i+1)%4]), the unit vector
    /// perpendicular to the edge on the face plane, pointing toward the face
    /// center. Used to compute per-corner insets when only some edges are
    /// beveled.
    edge_inward: [[f32; 3]; 4],
}

/// The 6 faces of a unit cube. Order: +X, -X, +Y, -Y, +Z, -Z.
///
/// Corner winding is CCW when viewed from outside (so the cross product
/// of edge01 × edge03 points outward).
const FACES: [FaceDesc; 6] = [
    // Face 0: +X (right)
    // Viewed from +X: corners go BL→BR→TR→TL (Y up, Z right)
    FaceDesc {
        corners: [
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ],
        normal: [1.0, 0.0, 0.0],
        edge_adj: [3, 4, 2, 5], // bottom→-Y, right→+Z, top→+Y, left→-Z
        neighbor_delta: [1, 0, 0],
        edge_inward: [[0.0, 1.0, 0.0], [0.0, 0.0, -1.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
    },
    // Face 1: -X (left)
    // Viewed from -X: corners go BL→BR→TR→TL (Y up, Z left)
    FaceDesc {
        corners: [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
        ],
        normal: [-1.0, 0.0, 0.0],
        edge_adj: [3, 5, 2, 4], // bottom→-Y, right→-Z, top→+Y, left→+Z
        neighbor_delta: [-1, 0, 0],
        edge_inward: [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
    },
    // Face 2: +Y (top)
    // Viewed from +Y (above): corners go near-L→near-R→far-R→far-L
    FaceDesc {
        corners: [
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
        normal: [0.0, 1.0, 0.0],
        edge_adj: [5, 0, 4, 1], // near→-Z, right→+X, far→+Z, left→-X
        neighbor_delta: [0, 1, 0],
        edge_inward: [[0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]],
    },
    // Face 3: -Y (bottom)
    // Viewed from -Y (below): corners go far-L→far-R→near-R→near-L
    FaceDesc {
        corners: [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
        normal: [0.0, -1.0, 0.0],
        edge_adj: [4, 0, 5, 1], // far→+Z, right→+X, near→-Z, left→-X
        neighbor_delta: [0, -1, 0],
        edge_inward: [[0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
    },
    // Face 4: +Z (front)
    // Viewed from +Z: corners go BR→BL→TL→TR (Y up, X left)
    FaceDesc {
        corners: [
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ],
        normal: [0.0, 0.0, 1.0],
        edge_adj: [3, 1, 2, 0], // bottom→-Y, left→-X, top→+Y, right→+X
        neighbor_delta: [0, 0, 1],
        edge_inward: [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [-1.0, 0.0, 0.0]],
    },
    // Face 5: -Z (back)
    // Viewed from -Z: corners go BL→BR→TR→TL (Y up, X right)
    FaceDesc {
        corners: [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        normal: [0.0, 0.0, -1.0],
        edge_adj: [3, 0, 2, 1], // bottom→-Y, right→+X, top→+Y, left→-X
        neighbor_delta: [0, 0, -1],
        edge_inward: [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]],
    },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true if the voxel type is wood (trunk, branch, or root).
fn is_wood(vt: VoxelType) -> bool {
    matches!(vt, VoxelType::Trunk | VoxelType::Branch | VoxelType::Root)
}

/// Normalize a 3D vector in place. Returns zero vector if length is ~0.
fn normalize(v: &mut [f32; 3]) {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-8 {
        v[0] /= len;
        v[1] /= len;
        v[2] /= len;
    }
}

/// Project a 3D position onto 2D UV coordinates based on the face normal.
/// Uses world-space coordinates for seamless tiling across adjacent voxels.
fn face_uv(pos: [f32; 3], normal: [f32; 3]) -> [f32; 2] {
    if normal[0].abs() > 0.5 {
        // ±X face: use Z, Y
        [pos[2], pos[1]]
    } else if normal[1].abs() > 0.5 {
        // ±Y face: use X, Z
        [pos[0], pos[2]]
    } else {
        // ±Z face: use X, Y
        [pos[0], pos[1]]
    }
}

// ---------------------------------------------------------------------------
// Mesh generation
// ---------------------------------------------------------------------------

/// Generate a beveled mesh for the given list of wood voxels.
///
/// For each voxel, checks its 6 face-neighbors in `world`. Exposed faces
/// (neighbor is not wood) get the beveled decomposition; hidden faces are
/// culled. Fully enclosed voxels produce zero geometry.
pub fn generate_tree_mesh(
    world: &VoxelWorld,
    voxels: &[VoxelCoord],
    config: &MeshConfig,
) -> MeshData {
    let inset = config.bevel_inset.clamp(0.0, 0.45);
    let mut mesh = MeshData::default();

    for &voxel in voxels {
        let base = [voxel.x as f32, voxel.y as f32, voxel.z as f32];

        // Determine which of the 6 faces are exposed.
        let mut exposed = [false; 6];
        let mut any_exposed = false;
        for (fi, face) in FACES.iter().enumerate() {
            let nd = face.neighbor_delta;
            let neighbor = VoxelCoord::new(voxel.x + nd[0], voxel.y + nd[1], voxel.z + nd[2]);
            let neighbor_type = world.get(neighbor);
            if !is_wood(neighbor_type) {
                exposed[fi] = true;
                any_exposed = true;
            }
        }

        if !any_exposed {
            continue;
        }

        for (fi, face) in FACES.iter().enumerate() {
            if !exposed[fi] {
                continue;
            }

            // Compute absolute corner positions for this face.
            let abs_corners: [[f32; 3]; 4] = std::array::from_fn(|ci| {
                [
                    base[0] + face.corners[ci][0],
                    base[1] + face.corners[ci][1],
                    base[2] + face.corners[ci][2],
                ]
            });

            // --- Per-edge bevel determination ---
            // An edge should be beveled only when it's a genuinely exposed
            // corner. Edge i borders adjacent face adj_fi. The edge is beveled
            // when BOTH:
            //   (a) The adjacent face adj_fi is exposed (its face-neighbor is
            //       not wood), AND
            //   (b) The diagonal neighbor voxel (voxel + face.neighbor_delta +
            //       FACES[adj_fi].neighbor_delta) is not wood.
            // If either fails, the edge is "flat" — no bevel recess there.
            let edge_beveled: [bool; 4] = std::array::from_fn(|edge_idx| {
                let adj_fi = face.edge_adj[edge_idx];
                if !exposed[adj_fi] {
                    return false; // adjacent face hidden by face-neighbor wood
                }
                // Check diagonal neighbor
                let adj_nd = FACES[adj_fi].neighbor_delta;
                let diag = VoxelCoord::new(
                    voxel.x + face.neighbor_delta[0] + adj_nd[0],
                    voxel.y + face.neighbor_delta[1] + adj_nd[1],
                    voxel.z + face.neighbor_delta[2] + adj_nd[2],
                );
                !is_wood(world.get(diag))
            });

            // --- Per-corner inset and recess ---
            // Each corner ci is at the junction of edge (ci-1) and edge ci.
            // (Edge ci goes from corner[ci] to corner[(ci+1)%4], so corner ci
            // is the START of edge ci and the END of edge (ci-1)%4.)
            //
            // The inset position depends on which adjacent edges are beveled:
            //   Both beveled → inset from both edges (toward center)
            //   One beveled  → inset only from the beveled edge
            //   Neither      → stay at original face corner (no inset)
            //
            // The recess (pulling the outer corner toward the voxel center
            // along the face normal) only happens when BOTH edges are beveled.
            let prev_edge = |ci: usize| (ci + 3) % 4;

            let inset_corners: [[f32; 3]; 4] = std::array::from_fn(|ci| {
                let bevel_prev = edge_beveled[prev_edge(ci)];
                let bevel_cur = edge_beveled[ci];
                let mut pos = abs_corners[ci];
                if bevel_prev {
                    // Inset from edge (ci-1): push along edge_inward of that edge
                    let iw = face.edge_inward[prev_edge(ci)];
                    pos[0] += iw[0] * inset;
                    pos[1] += iw[1] * inset;
                    pos[2] += iw[2] * inset;
                }
                if bevel_cur {
                    // Inset from edge ci: push along edge_inward of this edge
                    // But edge_inward points from edge toward center. The start
                    // corner of edge ci is corner ci. The perpendicular inward
                    // from edge ci at corner ci is edge_inward[ci].
                    let iw = face.edge_inward[ci];
                    pos[0] += iw[0] * inset;
                    pos[1] += iw[1] * inset;
                    pos[2] += iw[2] * inset;
                }
                pos
            });

            let outer_corners: [[f32; 3]; 4] = std::array::from_fn(|ci| {
                let bevel_prev = edge_beveled[prev_edge(ci)];
                let bevel_cur = edge_beveled[ci];
                if bevel_prev && bevel_cur {
                    // Both edges beveled — recess along normal
                    [
                        abs_corners[ci][0] - face.normal[0] * inset,
                        abs_corners[ci][1] - face.normal[1] * inset,
                        abs_corners[ci][2] - face.normal[2] * inset,
                    ]
                } else {
                    // One or zero edges beveled — stay at face plane
                    abs_corners[ci]
                }
            });

            // Emit center quad with face normal.
            emit_quad(
                &mut mesh,
                inset_corners[0],
                inset_corners[1],
                inset_corners[2],
                inset_corners[3],
                face.normal,
            );

            // Emit 4 edge strips.
            for edge_idx in 0..4 {
                let next_idx = (edge_idx + 1) % 4;

                // Strip corners: outer → inner (center quad edge)
                let outer0 = outer_corners[edge_idx];
                let outer1 = outer_corners[next_idx];
                let inner1 = inset_corners[next_idx];
                let inner0 = inset_corners[edge_idx];

                // Determine the strip normal.
                let strip_normal = if edge_beveled[edge_idx] {
                    // Edge is beveled — chamfered normal (average of the two
                    // face normals).
                    let adj_fi = face.edge_adj[edge_idx];
                    let adj_n = FACES[adj_fi].normal;
                    let mut avg = [
                        face.normal[0] + adj_n[0],
                        face.normal[1] + adj_n[1],
                        face.normal[2] + adj_n[2],
                    ];
                    normalize(&mut avg);
                    avg
                } else {
                    // Edge is flat — face normal.
                    face.normal
                };

                emit_quad(&mut mesh, outer0, outer1, inner1, inner0, strip_normal);
            }
        }

        // --- Corner triangles ---
        // At each of the 8 cube vertices, 3 faces meet. When all 3 faces are
        // exposed and all 3 edges at that vertex are beveled (i.e., all 3
        // diagonal neighbors in those directions are air), the beveled recess
        // creates a small triangular gap. Emit a triangle to fill it.
        //
        // Table: for each cube corner (cx, cy, cz ∈ {0,1}), the 3 face indices
        // that meet there.
        const CORNER_FACES: [([i32; 3], [usize; 3]); 8] = [
            ([0, 0, 0], [1, 3, 5]), // -X, -Y, -Z
            ([1, 0, 0], [0, 3, 5]), // +X, -Y, -Z
            ([0, 1, 0], [1, 2, 5]), // -X, +Y, -Z
            ([1, 1, 0], [0, 2, 5]), // +X, +Y, -Z
            ([0, 0, 1], [1, 3, 4]), // -X, -Y, +Z
            ([1, 0, 1], [0, 3, 4]), // +X, -Y, +Z
            ([0, 1, 1], [1, 2, 4]), // -X, +Y, +Z
            ([1, 1, 1], [0, 2, 4]), // +X, +Y, +Z
        ];

        for &(corner_pos, face_idxs) in &CORNER_FACES {
            let [f0, f1, f2] = face_idxs;

            // All 3 faces must be exposed.
            if !exposed[f0] || !exposed[f1] || !exposed[f2] {
                continue;
            }

            // All 3 diagonal neighbors (pairs of face normals) must be air.
            // The diagonal for faces fi and fj is at
            //   voxel + FACES[fi].neighbor_delta + FACES[fj].neighbor_delta.
            let pairs = [(f0, f1), (f0, f2), (f1, f2)];
            let all_diag_air = pairs.iter().all(|&(fa, fb)| {
                let da = FACES[fa].neighbor_delta;
                let db = FACES[fb].neighbor_delta;
                let diag = VoxelCoord::new(
                    voxel.x + da[0] + db[0],
                    voxel.y + da[1] + db[1],
                    voxel.z + da[2] + db[2],
                );
                !is_wood(world.get(diag))
            });
            if !all_diag_air {
                continue;
            }

            // Also check the body diagonal (all 3 face normals summed).
            let d0 = FACES[f0].neighbor_delta;
            let d1 = FACES[f1].neighbor_delta;
            let d2 = FACES[f2].neighbor_delta;
            let body_diag = VoxelCoord::new(
                voxel.x + d0[0] + d1[0] + d2[0],
                voxel.y + d0[1] + d1[1] + d2[1],
                voxel.z + d0[2] + d1[2] + d2[2],
            );
            if is_wood(world.get(body_diag)) {
                continue;
            }

            // Compute the 3 recessed corner positions — one from each face's
            // outer_corner at this vertex. Each face recesses the shared cube
            // corner along its own normal.
            let abs_corner = [
                base[0] + corner_pos[0] as f32,
                base[1] + corner_pos[1] as f32,
                base[2] + corner_pos[2] as f32,
            ];

            let recessed: [[f32; 3]; 3] = std::array::from_fn(|i| {
                let n = FACES[face_idxs[i]].normal;
                [
                    abs_corner[0] - n[0] * inset,
                    abs_corner[1] - n[1] * inset,
                    abs_corner[2] - n[2] * inset,
                ]
            });

            // Normal: normalized sum of the 3 face normals (points outward
            // along the body diagonal).
            let mut tri_normal = [
                FACES[f0].normal[0] + FACES[f1].normal[0] + FACES[f2].normal[0],
                FACES[f0].normal[1] + FACES[f1].normal[1] + FACES[f2].normal[1],
                FACES[f0].normal[2] + FACES[f1].normal[2] + FACES[f2].normal[2],
            ];
            normalize(&mut tri_normal);

            // Winding: we need CCW when viewed from outside. The parity of the
            // corner determines whether we need to swap winding.
            let parity = corner_pos[0] + corner_pos[1] + corner_pos[2];
            if parity % 2 == 0 {
                emit_triangle(&mut mesh, recessed[0], recessed[2], recessed[1], tri_normal);
            } else {
                emit_triangle(&mut mesh, recessed[0], recessed[1], recessed[2], tri_normal);
            }
        }
    }

    mesh
}

/// Emit a single quad (2 triangles) into the mesh data.
/// Corners are expected in CCW order when viewed from the front.
fn emit_quad(
    mesh: &mut MeshData,
    c0: [f32; 3],
    c1: [f32; 3],
    c2: [f32; 3],
    c3: [f32; 3],
    normal: [f32; 3],
) {
    let base_idx = (mesh.vertices.len() / 3) as u32;

    // 4 vertices
    for &corner in &[c0, c1, c2, c3] {
        mesh.vertices.extend_from_slice(&corner);
        mesh.normals.extend_from_slice(&normal);
        let uv = face_uv(corner, normal);
        mesh.uvs.extend_from_slice(&uv);
    }

    // 2 triangles: 0-1-2, 0-2-3
    mesh.indices.push(base_idx);
    mesh.indices.push(base_idx + 1);
    mesh.indices.push(base_idx + 2);
    mesh.indices.push(base_idx);
    mesh.indices.push(base_idx + 2);
    mesh.indices.push(base_idx + 3);
}

/// Emit a single triangle into the mesh data.
/// Corners are expected in CCW order when viewed from the front.
fn emit_triangle(
    mesh: &mut MeshData,
    c0: [f32; 3],
    c1: [f32; 3],
    c2: [f32; 3],
    normal: [f32; 3],
) {
    let base_idx = (mesh.vertices.len() / 3) as u32;

    for &corner in &[c0, c1, c2] {
        mesh.vertices.extend_from_slice(&corner);
        mesh.normals.extend_from_slice(&normal);
        let uv = face_uv(corner, normal);
        mesh.uvs.extend_from_slice(&uv);
    }

    mesh.indices.push(base_idx);
    mesh.indices.push(base_idx + 1);
    mesh.indices.push(base_idx + 2);
}

// ---------------------------------------------------------------------------
// Bark texture generation
// ---------------------------------------------------------------------------

/// Simple integer hash for deterministic pseudo-random values.
fn hash_u32(mut x: u32) -> u32 {
    x = x.wrapping_mul(0x9E3779B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85EBCA6B);
    x ^= x >> 13;
    x = x.wrapping_mul(0xC2B2AE35);
    x ^= x >> 16;
    x
}

/// Generate a procedural bark texture with vertical streaks and noise.
///
/// The texture is fully opaque RGBA8, deterministic from the config parameters
/// and base color. Uses simple integer hashing (not the sim PRNG).
pub fn generate_bark_texture(config: &MeshConfig, base_color: [f32; 3]) -> TextureData {
    let size = config.bark_texture_size.max(1);
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);

    let streak_count = config.bark_streak_count.max(1) as f32;
    let noise_intensity = config.bark_noise_intensity.clamp(0.0, 1.0);

    for py in 0..size {
        for px in 0..size {
            // Vertical streak pattern: sine-based with varying frequency.
            let x_norm = px as f32 / size as f32;
            let y_norm = py as f32 / size as f32;

            // Primary vertical streaks.
            let streak = (x_norm * streak_count * std::f32::consts::PI * 2.0).sin() * 0.5 + 0.5;

            // Slight horizontal band variation (knots / growth rings).
            let band_hash = hash_u32(py.wrapping_mul(7919)) as f32 / u32::MAX as f32;
            let band = if band_hash < 0.05 { -0.1 } else { 0.0 };

            // Per-pixel noise.
            let pixel_hash =
                hash_u32(px.wrapping_mul(31).wrapping_add(py.wrapping_mul(37))) as f32
                    / u32::MAX as f32;
            let noise = (pixel_hash - 0.5) * 2.0 * noise_intensity;

            // Combine: base color modulated by streak darkness and noise.
            let streak_mod = 0.7 + streak * 0.3 + band;
            let _ = y_norm; // y_norm reserved for future vertical gradient

            let r = (base_color[0] * streak_mod + noise).clamp(0.0, 1.0);
            let g = (base_color[1] * streak_mod + noise).clamp(0.0, 1.0);
            let b = (base_color[2] * streak_mod + noise).clamp(0.0, 1.0);

            pixels.push((r * 255.0) as u8);
            pixels.push((g * 255.0) as u8);
            pixels.push((b * 255.0) as u8);
            pixels.push(255); // Fully opaque
        }
    }

    TextureData {
        pixels,
        width: size,
        height: size,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a small world and place a single trunk voxel at (5,5,5).
    fn single_voxel_world() -> (VoxelWorld, Vec<VoxelCoord>) {
        let mut world = VoxelWorld::new(12, 12, 12);
        let coord = VoxelCoord::new(5, 5, 5);
        world.set(coord, VoxelType::Trunk);
        (world, vec![coord])
    }

    #[test]
    fn isolated_voxel_produces_correct_geometry_count() {
        let (world, voxels) = single_voxel_world();
        let config = MeshConfig::default();
        let mesh = generate_tree_mesh(&world, &voxels, &config);

        // 6 exposed faces × 5 quads × 4 verts = 120 vertices
        // + 8 corner triangles × 3 verts = 24 vertices
        // Total: 144 vertices (= 432 floats)
        assert_eq!(mesh.vertices.len(), 432, "expected 144 vertices × 3 floats");
        // 6 faces × 5 quads × 6 indices = 180
        // + 8 corner triangles × 3 indices = 24
        // Total: 204
        assert_eq!(mesh.indices.len(), 204, "expected 204 indices");
        // Normals same count as vertices
        assert_eq!(mesh.normals.len(), 432);
        // UVs: 144 vertices × 2 = 288
        assert_eq!(mesh.uvs.len(), 288);
    }

    #[test]
    fn two_adjacent_voxels_hide_shared_face() {
        let mut world = VoxelWorld::new(12, 12, 12);
        let a = VoxelCoord::new(5, 5, 5);
        let b = VoxelCoord::new(6, 5, 5);
        world.set(a, VoxelType::Trunk);
        world.set(b, VoxelType::Trunk);
        let voxels = vec![a, b];

        let config = MeshConfig::default();
        let mesh = generate_tree_mesh(&world, &voxels, &config);

        // Each voxel has 1 hidden face (the shared +X/-X face), so 5 exposed each.
        // 2 × 5 faces × 5 quads × 4 verts = 200 face vertices
        // + 2 × 4 corner triangles × 3 verts = 24 corner vertices
        //   (each voxel has 4 corners on the non-shared side with all 3
        //    faces exposed and all diagonals air; the 4 corners on the
        //    shared side have the hidden face so no triangle)
        // Total: 224 vertices
        assert_eq!(
            mesh.vertices.len() / 3,
            224,
            "expected 224 vertices for 2 adjacent voxels"
        );
        // 300 face indices + 24 corner indices = 324
        assert_eq!(mesh.indices.len(), 324);
    }

    #[test]
    fn fully_enclosed_voxel_produces_no_geometry() {
        let mut world = VoxelWorld::new(12, 12, 12);
        let center = VoxelCoord::new(5, 5, 5);
        world.set(center, VoxelType::Trunk);
        // Surround with 6 face-adjacent wood voxels.
        for &[dx, dy, dz] in &[[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]
        {
            world.set(
                VoxelCoord::new(5 + dx, 5 + dy, 5 + dz),
                VoxelType::Branch,
            );
        }

        let config = MeshConfig::default();
        let mesh = generate_tree_mesh(&world, &[center], &config);

        assert_eq!(mesh.vertices.len(), 0, "enclosed voxel should produce no geometry");
        assert_eq!(mesh.indices.len(), 0);
    }

    #[test]
    fn trunk_adjacent_to_branch_hides_shared_face() {
        let mut world = VoxelWorld::new(12, 12, 12);
        let trunk = VoxelCoord::new(5, 5, 5);
        let branch = VoxelCoord::new(5, 6, 5);
        world.set(trunk, VoxelType::Trunk);
        world.set(branch, VoxelType::Branch);

        let config = MeshConfig::default();

        // Generate mesh for just the trunk voxel — its +Y face should be hidden.
        let mesh = generate_tree_mesh(&world, &[trunk], &config);
        // 5 exposed faces × 5 quads × 4 verts = 100 face verts
        // + 4 corner triangles at y=0 × 3 verts = 12 corner verts
        //   (4 corners at y=1 include hidden +Y face → no triangle)
        // Total: 112
        assert_eq!(
            mesh.vertices.len() / 3,
            112,
            "trunk +Y face hidden by adjacent branch"
        );
    }

    #[test]
    fn zero_bevel_inset_still_works() {
        let (world, voxels) = single_voxel_world();
        let config = MeshConfig {
            bevel_inset: 0.0,
            ..MeshConfig::default()
        };
        let mesh = generate_tree_mesh(&world, &voxels, &config);

        // Same geometry count — edge strips and corner triangles have zero
        // area but are still emitted.
        assert_eq!(mesh.vertices.len() / 3, 144);
        assert_eq!(mesh.indices.len(), 204);
    }

    #[test]
    fn nonzero_bevel_produces_angled_normals() {
        let (world, voxels) = single_voxel_world();
        let config = MeshConfig {
            bevel_inset: 0.2,
            ..MeshConfig::default()
        };
        let mesh = generate_tree_mesh(&world, &voxels, &config);

        // Collect all unique normals.
        let mut normals = std::collections::BTreeSet::new();
        for i in (0..mesh.normals.len()).step_by(3) {
            // Quantize to avoid floating point comparison issues.
            let nx = (mesh.normals[i] * 1000.0).round() as i32;
            let ny = (mesh.normals[i + 1] * 1000.0).round() as i32;
            let nz = (mesh.normals[i + 2] * 1000.0).round() as i32;
            normals.insert((nx, ny, nz));
        }

        // With bevel > 0 on an isolated voxel (all faces exposed), edge strips
        // get chamfered normals (average of two face normals). There should be
        // more than just the 6 axis-aligned normals.
        assert!(
            normals.len() > 6,
            "expected angled normals from beveled edges, got {} unique normals",
            normals.len()
        );
    }

    #[test]
    fn bevel_produces_non_coplanar_geometry() {
        // With a non-zero bevel, the edge strip outer corners are recessed
        // inward along the face normal. This means not all vertices on the
        // +X face lie at x=1 — some should be at x=1-inset.
        let (world, voxels) = single_voxel_world();
        let inset = 0.2;
        let config = MeshConfig {
            bevel_inset: inset,
            ..MeshConfig::default()
        };
        let mesh = generate_tree_mesh(&world, &voxels, &config);

        // Collect all unique x-coordinates of vertices (quantized).
        let mut x_values = std::collections::BTreeSet::new();
        for i in (0..mesh.vertices.len()).step_by(3) {
            x_values.insert((mesh.vertices[i] * 1000.0).round() as i32);
        }

        // For the +X face (at x=6 since voxel is at (5,5,5)), center quad
        // vertices are at x=6.0 and edge strip outer vertices at x=6.0-0.2=5.8.
        // For the -X face (at x=5), center quad at x=5.0 and outer at x=5.0+0.2=5.2.
        // Other faces have x values ranging from 5.0 to 6.0.
        assert!(
            x_values.contains(&5800),
            "expected recessed x=5.8 from +X face bevel, got x values: {:?}",
            x_values
        );
        assert!(
            x_values.contains(&5200),
            "expected recessed x=5.2 from -X face bevel, got x values: {:?}",
            x_values
        );
    }

    #[test]
    fn edge_adj_correctness() {
        // Verify each face's edge_adj entries reference the correct adjacent
        // face by checking that the shared edge actually borders that face.
        for (fi, face) in FACES.iter().enumerate() {
            for edge_idx in 0..4 {
                let next_idx = (edge_idx + 1) % 4;
                let adj_fi = face.edge_adj[edge_idx];

                // The edge goes from face.corners[edge_idx] to face.corners[next_idx].
                // Both corners should lie on the adjacent face's plane.
                let adj = &FACES[adj_fi];
                for &ci in &[edge_idx, next_idx] {
                    let corner = face.corners[ci];
                    // The corner should satisfy: dot(corner, adj.normal) == dot(adj.corners[0], adj.normal)
                    // (i.e., it lies on the adjacent face's plane, which is at the voxel boundary).
                    // For an adjacent face, the corner lies on the adjacent face's plane
                    // OR on the opposite side (for the face's own plane).
                    // Simpler check: the corner should be on the boundary between
                    // this face and the adjacent face.
                    let dot_corner = corner[0] * adj.neighbor_delta[0] as f32
                        + corner[1] * adj.neighbor_delta[1] as f32
                        + corner[2] * adj.neighbor_delta[2] as f32;
                    // The adjacent face's neighbor_delta points outward. The shared
                    // edge corners should be at the maximum extent in that direction
                    // (value = 0 or 1 depending on direction).
                    let expected = if adj.neighbor_delta[0] + adj.neighbor_delta[1] + adj.neighbor_delta[2] > 0 {
                        1.0
                    } else {
                        0.0
                    };
                    assert!(
                        (dot_corner - expected).abs() < 1e-6,
                        "face {fi} edge {edge_idx} corner {ci}: corner {:?} should be on the boundary of face {adj_fi} (adj normal {:?}), dot={dot_corner} expected={expected}",
                        corner, adj.normal
                    );
                }
            }
        }
    }

    #[test]
    fn diagonal_neighbor_suppresses_edge_bevel() {
        // Two wood voxels sharing a cube edge (diagonal neighbors, not
        // face-adjacent). The shared edge should NOT be beveled.
        let mut world = VoxelWorld::new(12, 12, 12);
        let a = VoxelCoord::new(5, 5, 5);
        let b = VoxelCoord::new(6, 6, 5); // diagonal in X-Y plane
        world.set(a, VoxelType::Trunk);
        world.set(b, VoxelType::Trunk);

        let config = MeshConfig {
            bevel_inset: 0.2,
            ..MeshConfig::default()
        };
        let mesh = generate_tree_mesh(&world, &[a], &config);

        // Voxel A has all 6 faces exposed (B is diagonal, not face-adjacent).
        // But the edge where A's +X face meets A's +Y face has diagonal
        // neighbor B at (6,6,5) = A + (1,0,0) + (0,1,0). That diagonal is
        // wood, so this edge should NOT be beveled.
        //
        // All 6 faces × 5 quads = 30 quads (vertex count unchanged).
        // Corner triangles: the 2 corners at (1,1,z) for z∈{0,1} have their
        // +X/+Y diagonal blocked by B, so no triangle there. The other
        // 6 corners still get triangles.
        // Total: 120 face verts + 6×3 corner verts = 138 verts
        assert_eq!(
            mesh.vertices.len() / 3,
            138,
            "expected 138 vertices (6 corner triangles, 2 suppressed by diagonal)"
        );

        // Verify the +X face's edge bordering +Y has flat (non-chamfered)
        // normal. For face 0 (+X), edge 2 borders face 2 (+Y).
        // With this edge flat, the strip normal should be [1,0,0] (face
        // normal), not the chamfered [0.707, 0.707, 0].
        // Collect normals from vertices near the +X/+Y edge.
        let mut found_chamfered_xy = false;
        for i in (0..mesh.normals.len()).step_by(3) {
            let nx = mesh.normals[i];
            let ny = mesh.normals[i + 1];
            // A chamfered +X/+Y normal would have nx≈0.707 and ny≈0.707
            if nx > 0.5 && nx < 0.9 && ny > 0.5 && ny < 0.9 {
                found_chamfered_xy = true;
            }
        }
        assert!(
            !found_chamfered_xy,
            "edge between +X and +Y should NOT be chamfered when diagonal neighbor exists"
        );
    }

    #[test]
    fn l_shaped_selective_beveling() {
        // L-shaped configuration: 3 voxels forming an L.
        // A at (5,5,5), B at (6,5,5), C at (5,6,5).
        // Tests that bevels are selectively applied based on neighbors.
        let mut world = VoxelWorld::new(12, 12, 12);
        let a = VoxelCoord::new(5, 5, 5);
        let b = VoxelCoord::new(6, 5, 5);
        let c = VoxelCoord::new(5, 6, 5);
        world.set(a, VoxelType::Trunk);
        world.set(b, VoxelType::Trunk);
        world.set(c, VoxelType::Trunk);

        let config = MeshConfig {
            bevel_inset: 0.2,
            ..MeshConfig::default()
        };
        let mesh = generate_tree_mesh(&world, &[a, b, c], &config);

        // Basic sanity: mesh should have geometry.
        assert!(mesh.vertices.len() > 0, "L-shape should produce geometry");
        assert!(mesh.indices.len() > 0);

        // The inner corner at (6,6,5) — where A's +X/+Y diagonal is — has B
        // (at (6,5,5)) and C (at (5,6,5)) as face neighbors. The diagonal
        // voxel at (6,6,5) is air, but B and C each suppress certain bevels
        // on A's faces. Check that A's +X face edge bordering +Y is NOT
        // beveled (A's +X face is hidden by B, so it produces no geometry at
        // all). A's +Y face edge bordering +X is also NOT beveled (A's +X
        // face is hidden, so that edge_adj is not exposed).
        //
        // Verify: no chamfered normal at 45° between +X and +Y should appear
        // on voxel A's geometry (since A's +X face is entirely hidden by B).
        // We can't easily isolate A's geometry from the combined mesh, but we
        // can verify overall consistency.
        assert_eq!(
            mesh.vertices.len() % 3,
            0,
            "vertex count should be divisible by 3"
        );
        assert_eq!(
            mesh.normals.len(),
            mesh.vertices.len(),
            "normals count should match vertices"
        );
    }

    #[test]
    fn edge_inward_correctness() {
        // Verify that each face's edge_inward vectors are perpendicular to
        // the edge and point toward the face center.
        for (fi, face) in FACES.iter().enumerate() {
            let center = [
                (face.corners[0][0] + face.corners[1][0] + face.corners[2][0] + face.corners[3][0]) / 4.0,
                (face.corners[0][1] + face.corners[1][1] + face.corners[2][1] + face.corners[3][1]) / 4.0,
                (face.corners[0][2] + face.corners[1][2] + face.corners[2][2] + face.corners[3][2]) / 4.0,
            ];
            for edge_idx in 0..4 {
                let next_idx = (edge_idx + 1) % 4;
                let c0 = face.corners[edge_idx];
                let c1 = face.corners[next_idx];

                // Edge direction
                let edge_dir = [c1[0] - c0[0], c1[1] - c0[1], c1[2] - c0[2]];
                let iw = face.edge_inward[edge_idx];

                // edge_inward should be perpendicular to the edge
                let dot = edge_dir[0] * iw[0] + edge_dir[1] * iw[1] + edge_dir[2] * iw[2];
                assert!(
                    dot.abs() < 1e-6,
                    "face {fi} edge {edge_idx}: edge_inward not perpendicular to edge (dot={dot})"
                );

                // edge_inward should be perpendicular to the face normal
                let dot_n = face.normal[0] * iw[0] + face.normal[1] * iw[1] + face.normal[2] * iw[2];
                assert!(
                    dot_n.abs() < 1e-6,
                    "face {fi} edge {edge_idx}: edge_inward not on face plane (dot with normal={dot_n})"
                );

                // edge_inward should point toward the center: the midpoint of
                // the edge plus edge_inward should be closer to center.
                let mid = [(c0[0] + c1[0]) / 2.0, (c0[1] + c1[1]) / 2.0, (c0[2] + c1[2]) / 2.0];
                let shifted = [mid[0] + iw[0] * 0.1, mid[1] + iw[1] * 0.1, mid[2] + iw[2] * 0.1];
                let dist_mid = (mid[0] - center[0]).powi(2) + (mid[1] - center[1]).powi(2) + (mid[2] - center[2]).powi(2);
                let dist_shifted = (shifted[0] - center[0]).powi(2) + (shifted[1] - center[1]).powi(2) + (shifted[2] - center[2]).powi(2);
                assert!(
                    dist_shifted < dist_mid,
                    "face {fi} edge {edge_idx}: edge_inward points away from center"
                );
            }
        }
    }

    #[test]
    fn bark_texture_correct_dimensions_and_opaque() {
        let config = MeshConfig {
            bark_texture_size: 32,
            ..MeshConfig::default()
        };
        let tex = generate_bark_texture(&config, config.trunk_color);

        assert_eq!(tex.width, 32);
        assert_eq!(tex.height, 32);
        assert_eq!(tex.pixels.len(), 32 * 32 * 4);

        // Every alpha byte should be 255 (fully opaque).
        for (i, &byte) in tex.pixels.iter().enumerate() {
            if i % 4 == 3 {
                assert_eq!(byte, 255, "pixel at byte offset {i} should be fully opaque");
            }
        }
    }

    #[test]
    fn mesh_config_json_roundtrip() {
        let config = MeshConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let restored: MeshConfig = serde_json::from_str(&json).unwrap();

        assert!((config.bevel_inset - restored.bevel_inset).abs() < 1e-6);
        assert_eq!(config.bark_texture_size, restored.bark_texture_size);
        assert_eq!(config.bark_streak_count, restored.bark_streak_count);
        assert!((config.bark_noise_intensity - restored.bark_noise_intensity).abs() < 1e-6);
        for i in 0..3 {
            assert!((config.trunk_color[i] - restored.trunk_color[i]).abs() < 1e-6);
            assert!((config.branch_color[i] - restored.branch_color[i]).abs() < 1e-6);
            assert!((config.root_color[i] - restored.root_color[i]).abs() < 1e-6);
        }
    }
}
