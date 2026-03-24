// Chunk-based voxel mesh generation with smooth surface rendering.
//
// Pure Rust, no Godot dependencies. Converts a region of the voxel world into
// triangle mesh data suitable for rendering. The world is divided into 16x16x16
// chunks; each chunk produces a `ChunkMesh` with three surfaces (bark, ground,
// and leaf) that the gdext bridge converts into Godot `ArrayMesh` objects.
//
// ## Solid voxels: smooth mesh pipeline
//
// Solid opaque voxels (Trunk, Branch, Root, Dirt, GrownPlatform, GrownWall,
// Strut) go through the smooth mesh pipeline in `smooth_mesh.rs`: each visible
// face is subdivided into 8 triangles, chamfered at edges and corners, and
// iteratively smoothed via a Laplacian curvature-minimizing algorithm. The
// generator reads a 2-voxel border around each chunk for cross-boundary
// smoothing consistency, then filters the output to only include triangles
// within the chunk's own 16³ region. Solid surfaces use vertex colors only
// (no textures). Per-vertex normals enable smooth (Gouraud) shading.
//
// ## Leaf voxels
//
// Leaf voxels also go through the smooth mesh pipeline with shell-only
// culling (leaf↔leaf faces culled). Solid→leaf faces are NOT culled (wood
// visible through semi-transparent leaves). Leaf uses a procedural noise
// shader for alpha scissor (no UVs needed).
//
// ## Decimation
//
// After the smooth mesh pipeline, an optional decimation pass in
// `mesh_decimation.rs` reduces triangle count via QEM edge-collapse,
// coplanar region re-triangulation, and collinear vertex collapse.
//
// ## RLE-aware iteration
//
// The smooth mesh pass iterates all voxels in the chunk + border region. The
// leaf pass walks each column's RLE spans via `VoxelWorld::column_spans()`,
// clipping to the chunk's Y range and skipping non-leaf spans.
//
// ## Face culling rule
//
// A face of voxel A toward neighbor B is **culled iff B is opaque**. This means:
// - Opaque↔Opaque: both faces culled (never visible)
// - Opaque→Air/Leaf/Fruit: face rendered
// - Leaf→Opaque: face culled
// - Leaf→Leaf: both faces rendered (transparency needs them)
// - Leaf→Air: face rendered
//
// All solid opaque voxels participate in the smooth mesh pipeline. Non-opaque
// types (Air, BuildingInterior, WoodLadder, RopeLadder) produce no geometry.
//
// See also: `smooth_mesh.rs` for the smoothing pipeline and design rationale,
// `world.rs` for the voxel grid and `column_spans()` API,
// `types.rs` for `VoxelType::is_opaque()`, `mesh_cache.rs` (in the gdext
// crate) for the caching layer that sits on top of this module,
// `sim_bridge.rs` for the Godot-facing API that builds `ArrayMesh` objects
// from `ChunkMesh` data, `tree_renderer.gd` for material creation,
// `texture_gen.rs` for the old tiling texture system (not currently used for
// solid surfaces but kept for reference).
//
// **Determinism note:** This module is pure and deterministic (same world state
// produces identical mesh data), but mesh generation is a rendering concern and
// does not participate in the sim's lockstep determinism contract.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::smooth_mesh::{SmoothMesh, TAG_BARK, TAG_GROUND, TAG_LEAF};
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

/// Global toggle for the smoothing pass (chamfer always runs).
/// When false, only chamfering is applied. Debug tool for comparing
/// chamfer-only vs chamfer+smoothing output.
static SMOOTHING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Global toggle for smooth vs flat normals. When false, each triangle
/// gets a flat per-face normal instead of smooth area-weighted normals.
static SMOOTH_NORMALS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Global toggle for QEM mesh decimation. When enabled, coplanar triangles
/// are collapsed after chamfer (and optionally smoothing) to reduce triangle
/// count with minimal visual impact.
static DECIMATION_ENABLED: AtomicBool = AtomicBool::new(true);

/// Maximum quadric error for mesh decimation. Lower values preserve more
/// detail; near-zero is lossless for flat-shaded chamfered meshes.
/// Default: 1e-6 (near-zero, lossless for chamfered meshes).
static DECIMATION_MAX_ERROR: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x3586_37BD); // f32::to_bits(1e-6)

/// Enable or disable QEM mesh decimation.
pub fn set_decimation_enabled(enabled: bool) {
    DECIMATION_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Returns whether mesh decimation is currently enabled.
pub fn decimation_enabled() -> bool {
    DECIMATION_ENABLED.load(Ordering::Relaxed)
}

/// Set the maximum error threshold for decimation.
pub fn set_decimation_max_error(max_error: f32) {
    DECIMATION_MAX_ERROR.store(max_error.to_bits(), Ordering::Relaxed);
}

/// Returns the current decimation max error threshold.
pub fn decimation_max_error() -> f32 {
    f32::from_bits(DECIMATION_MAX_ERROR.load(Ordering::Relaxed))
}

/// Enable or disable the smoothing pass (chamfer always runs).
pub fn set_smoothing_enabled(enabled: bool) {
    SMOOTHING_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Returns whether smoothing is currently enabled.
pub fn smoothing_enabled() -> bool {
    SMOOTHING_ENABLED.load(Ordering::Relaxed)
}

/// Enable or disable smooth normals (vs flat per-face normals).
pub fn set_smooth_normals_enabled(enabled: bool) {
    SMOOTH_NORMALS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Returns whether smooth normals are enabled.
pub fn smooth_normals_enabled() -> bool {
    SMOOTH_NORMALS_ENABLED.load(Ordering::Relaxed)
}

/// Side length of a chunk in voxels.
pub const CHUNK_SIZE: i32 = 16;

/// A chunk coordinate in chunk-space (each unit = 16 voxels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChunkCoord {
    pub cx: i32,
    pub cy: i32,
    pub cz: i32,
}

impl ChunkCoord {
    pub const fn new(cx: i32, cy: i32, cz: i32) -> Self {
        Self { cx, cy, cz }
    }
}

/// Convert a voxel coordinate to the chunk coordinate that contains it.
pub fn voxel_to_chunk(coord: VoxelCoord) -> ChunkCoord {
    ChunkCoord {
        cx: coord.x.div_euclid(CHUNK_SIZE),
        cy: coord.y.div_euclid(CHUNK_SIZE),
        cz: coord.z.div_euclid(CHUNK_SIZE),
    }
}

/// Raw mesh data for one surface (material group) of a chunk.
#[derive(Clone, Debug, Default)]
pub struct SurfaceMesh {
    /// Vertex positions (3 floats per vertex: x, y, z).
    pub vertices: Vec<f32>,
    /// Vertex normals (3 floats per vertex: nx, ny, nz).
    pub normals: Vec<f32>,
    /// Triangle indices (3 per triangle, referencing vertices).
    pub indices: Vec<u32>,
    /// Vertex colors (4 floats per vertex: r, g, b, a).
    pub colors: Vec<f32>,
    /// UV coordinates (2 floats per vertex: u, v). Currently unused — all
    /// surfaces use procedural noise shaders that derive texture coordinates
    /// from world position. Retained for potential future use.
    pub uvs: Vec<f32>,
}

impl SurfaceMesh {
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// Number of vertices in this surface.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len() / 3
    }

    /// Estimated heap byte size of this surface's data.
    pub fn estimate_byte_size(&self) -> usize {
        self.vertices.len() * 4
            + self.normals.len() * 4
            + self.indices.len() * 4
            + self.colors.len() * 4
            + self.uvs.len() * 4
    }
}

/// Mesh data for one chunk, split into bark, ground, and leaf surfaces.
#[derive(Clone, Debug, Default)]
pub struct ChunkMesh {
    /// Surface 0: bark voxels (Trunk, Branch, Root, construction types).
    pub bark: SurfaceMesh,
    /// Surface 1: ground voxels (Dirt).
    pub ground: SurfaceMesh,
    /// Surface 2: leaf voxels (alpha-scissor transparency).
    pub leaf: SurfaceMesh,
}

impl ChunkMesh {
    pub fn is_empty(&self) -> bool {
        self.bark.is_empty() && self.ground.is_empty() && self.leaf.is_empty()
    }

    /// Estimated heap byte size of all surface data in this chunk mesh.
    pub fn estimate_byte_size(&self) -> usize {
        self.bark.estimate_byte_size()
            + self.ground.estimate_byte_size()
            + self.leaf.estimate_byte_size()
    }
}

/// Return the vertex color for a voxel type. Returns `[r, g, b, a]`.
/// Colors match the existing GDScript renderers for visual consistency.
pub fn voxel_color(vt: VoxelType) -> [f32; 4] {
    match vt {
        VoxelType::Trunk => [0.35, 0.22, 0.10, 1.0],
        VoxelType::Branch => [0.45, 0.30, 0.15, 1.0],
        VoxelType::Root => [0.30, 0.20, 0.12, 1.0],
        VoxelType::Dirt => [0.25, 0.45, 0.20, 1.0],
        VoxelType::Leaf => [0.12, 0.42, 0.08, 1.0],
        VoxelType::GrownPlatform | VoxelType::GrownWall => [0.50, 0.35, 0.18, 1.0],
        // Strut: slightly darker/redder wood tint to distinguish from platforms.
        VoxelType::Strut => [0.55, 0.30, 0.15, 1.0],
        // Types that don't produce geometry — return a visible debug color.
        _ => [1.0, 0.0, 1.0, 1.0],
    }
}

/// Returns true if this voxel type should produce geometry in the chunk mesh.
pub fn produces_geometry(vt: VoxelType) -> bool {
    matches!(
        vt,
        VoxelType::Trunk
            | VoxelType::Branch
            | VoxelType::Root
            | VoxelType::Dirt
            | VoxelType::Leaf
            | VoxelType::GrownPlatform
            | VoxelType::GrownWall
            | VoxelType::Strut
    )
}

/// Border radius (in voxels) around each chunk for the smooth mesh pipeline.
/// Provides context for face culling, anchoring, and smoothing passes so that
/// boundary vertices get the same smoothing result in adjacent chunks.
const SMOOTH_BORDER: i32 = 2;

/// The 6 face directions as (dx, dy, dz) offsets and their outward normals.
const FACES: [(i32, i32, i32); 6] = [
    (1, 0, 0),  // +X
    (-1, 0, 0), // -X
    (0, 1, 0),  // +Y
    (0, -1, 0), // -Y
    (0, 0, 1),  // +Z
    (0, 0, -1), // -Z
];

/// Face vertex offsets relative to the voxel origin (0,0,0)→(1,1,1).
/// Each face has 4 vertices in CCW winding order when viewed from outside.
/// Format: [face_index][vertex_index] = (x, y, z)
pub const FACE_VERTICES: [[[f32; 3]; 4]; 6] = [
    // +X face (normal +1,0,0): x=1 plane
    [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    ],
    // -X face (normal -1,0,0): x=0 plane
    [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0],
    ],
    // +Y face (normal 0,+1,0): y=1 plane
    [
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ],
    // -Y face (normal 0,-1,0): y=0 plane
    [
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    ],
    // +Z face (normal 0,0,+1): z=1 plane
    [
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
    ],
    // -Z face (normal 0,0,-1): z=0 plane
    [
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    ],
];

/// The 6 face normals as floats, matching FACES order.
pub const FACE_NORMALS: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],  // +X
    [-1.0, 0.0, 0.0], // -X
    [0.0, 1.0, 0.0],  // +Y
    [0.0, -1.0, 0.0], // -Y
    [0.0, 0.0, 1.0],  // +Z
    [0.0, 0.0, -1.0], // -Z
];

/// Returns true if this opaque voxel type belongs to the ground surface.
fn is_ground_voxel(vt: VoxelType) -> bool {
    vt == VoxelType::Dirt
}

/// Returns true if this voxel type should use the smooth mesh pipeline
/// (subdivided, chamfered, smoothed). Includes both solid opaque voxels
/// and leaf voxels. Fruit is excluded (rendered as billboard sprites).
fn is_smooth_voxel(vt: VoxelType) -> bool {
    vt.is_opaque() || vt == VoxelType::Leaf
}

/// Returns true if this voxel type should be treated as "solid" for
/// face culling within the smooth mesh. Leaf voxels are included so
/// leaf↔leaf faces are culled (shell-only rendering).
fn is_smooth_opaque(vt: VoxelType) -> bool {
    vt.is_opaque() || vt == VoxelType::Leaf
}

/// Generate the mesh data for a single chunk of the world.
///
/// All solid opaque voxels and leaf voxels go through the smooth mesh
/// pipeline: each visible face is subdivided into 8 triangles, chamfered,
/// and optionally smoothed. Solid and leaf share vertices at boundaries.
/// Chamfer/smoothing processes solid first, then leaf-only vertices.
/// Leaf↔leaf interior faces are culled (shell-only). The pipeline reads
/// voxels in a border around the chunk for cross-boundary consistency.
///
/// If `y_cutoff` is `Some(y)`, voxels with world Y ≥ y are treated as air:
/// they produce no geometry, and neighbors facing them get their faces exposed.
///
/// See `smooth_mesh.rs` for the smoothing pipeline and
/// `docs/drafts/visual_smooth.md` for the full design.
pub fn generate_chunk_mesh(
    world: &VoxelWorld,
    chunk: ChunkCoord,
    y_cutoff: Option<i32>,
) -> ChunkMesh {
    let mut mesh = ChunkMesh::default();

    let base_x = chunk.cx * CHUNK_SIZE;
    let base_y = chunk.cy * CHUNK_SIZE;
    let base_z = chunk.cz * CHUNK_SIZE;
    let chunk_y_end = base_y + CHUNK_SIZE - 1;

    // Effective Y ceiling: the lower of the chunk top and the y_cutoff.
    let effective_y_end = match y_cutoff {
        Some(cutoff) => chunk_y_end.min(cutoff.saturating_sub(1)),
        None => chunk_y_end,
    };

    // If the entire chunk is above the cutoff, produce an empty mesh.
    if effective_y_end < base_y {
        return mesh;
    }

    // --- Smooth mesh pass for solid opaque voxels ---
    // Iterate the chunk + border region, building a single unified SmoothMesh
    // for all solid surfaces. Using one mesh ensures that vertices shared
    // between different voxel types (e.g., trunk/dirt boundary) are smoothed
    // together, preventing seams. The mesh is split into bark and ground
    // surfaces at the output stage using per-triangle surface tags.
    let mut solid_smooth = SmoothMesh::new();

    let border = SMOOTH_BORDER;
    let smooth_min_x = (base_x - border).max(0);
    let smooth_max_x = (base_x + CHUNK_SIZE + border).min(world.size_x as i32);
    let smooth_min_y = (base_y - border).max(0);
    let smooth_max_y_unbounded = (base_y + CHUNK_SIZE + border).min(world.size_y as i32);
    let smooth_min_z = (base_z - border).max(0);
    let smooth_max_z = (base_z + CHUNK_SIZE + border).min(world.size_z as i32);

    // Effective Y ceiling for smooth mesh includes the border.
    let smooth_effective_y_end = match y_cutoff {
        Some(cutoff) => smooth_max_y_unbounded.min(cutoff.saturating_sub(1)),
        None => smooth_max_y_unbounded,
    };

    for wz in smooth_min_z..smooth_max_z {
        for wx in smooth_min_x..smooth_max_x {
            for (vt, y_start, y_end) in world.column_spans(wx as u32, wz as u32) {
                if !is_smooth_voxel(vt) {
                    continue;
                }

                let clipped_start = (y_start as i32).max(smooth_min_y);
                let clipped_end = (y_end as i32).min(smooth_effective_y_end);
                if clipped_start > clipped_end {
                    continue;
                }

                let color = voxel_color(vt);
                let is_leaf = vt == VoxelType::Leaf;
                let tag = if is_leaf {
                    TAG_LEAF
                } else if is_ground_voxel(vt) {
                    TAG_GROUND
                } else {
                    TAG_BARK
                };

                for wy in clipped_start..=clipped_end {
                    for (face_idx, &(dx, dy, dz)) in FACES.iter().enumerate() {
                        let ny = wy + dy;
                        let neighbor = world.get(VoxelCoord::new(wx + dx, ny, wz + dz));

                        // Face culling rules:
                        // - solid→solid: cull (both opaque)
                        // - solid→leaf: DON'T cull (wood visible through leaves)
                        // - leaf→solid: cull (hidden behind solid)
                        // - leaf→leaf: cull (shell-only)
                        // - anything→air: don't cull (visible)
                        let neighbor_culls = match y_cutoff {
                            Some(cutoff) if ny >= cutoff => false,
                            _ => {
                                if is_leaf {
                                    // Leaf face: cull toward solid OR other leaf.
                                    is_smooth_opaque(neighbor)
                                } else {
                                    // Solid face: cull toward solid only.
                                    neighbor.is_opaque()
                                }
                            }
                        };

                        if neighbor_culls {
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
                        let vert_indices = solid_smooth.add_subdivided_face(
                            corners,
                            normal,
                            color,
                            tag,
                            [wx, wy, wz],
                        );

                        // Anchoring rule 2: faces adjacent to non-solid
                        // constructed voxels keep sharp edges.
                        if matches!(
                            neighbor,
                            VoxelType::BuildingInterior
                                | VoxelType::WoodLadder
                                | VoxelType::RopeLadder
                        ) {
                            solid_smooth.anchor_vertices(&vert_indices);
                        }
                    }
                }
            }
        }
    }

    // Run the smoothing pipeline on the unified solid mesh.
    if !solid_smooth.vertices.is_empty() {
        solid_smooth.normalize_initial_normals();
        solid_smooth.apply_anchoring();
        solid_smooth.chamfer();
        if smoothing_enabled() {
            solid_smooth.smooth();
        }
        // Chunk bounds used for both decimation boundary pinning and output filtering.
        let chunk_min = [base_x, base_y, base_z];
        let chunk_max = [
            base_x + CHUNK_SIZE,
            base_y + CHUNK_SIZE,
            base_z + CHUNK_SIZE,
        ];
        if decimation_enabled() {
            solid_smooth.coplanar_region_retri(Some((chunk_min, chunk_max)));
            solid_smooth.collapse_collinear_boundary_vertices(Some((chunk_min, chunk_max)));
            solid_smooth.decimate(decimation_max_error(), Some((chunk_min, chunk_max)));
        }
        let mut surfaces = solid_smooth.to_split_surface_meshes_filtered(chunk_min, chunk_max);
        if let Some(bark) = surfaces.remove(&TAG_BARK) {
            mesh.bark = bark;
        }
        if let Some(ground) = surfaces.remove(&TAG_GROUND) {
            mesh.ground = ground;
        }
        if let Some(leaf) = surfaces.remove(&TAG_LEAF) {
            mesh.leaf = leaf;
        }
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a small world (one chunk = 16x16x16) and return it.
    /// Also disables decimation — mesh_gen tests check exact triangle counts
    /// from the subdivision pipeline and should not be affected by decimation.
    fn one_chunk_world() -> VoxelWorld {
        set_decimation_enabled(false);
        VoxelWorld::new(16, 16, 16)
    }

    #[test]
    fn empty_chunk_produces_empty_mesh() {
        let world = one_chunk_world();
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        assert!(mesh.is_empty());
    }

    #[test]
    fn single_trunk_voxel_produces_smooth_mesh() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);

        // Smooth mesh: 6 faces × 8 triangles = 48 triangles.
        // Per-triangle colors mean 3 verts per triangle = 144 output vertices.
        assert_eq!(mesh.bark.vertex_count(), 48 * 3);
        assert_eq!(mesh.bark.indices.len(), 48 * 3);
        // No leaf geometry.
        assert!(mesh.leaf.is_empty());
        // Smooth bark has no UVs (vertex colors only).
        assert!(mesh.bark.uvs.is_empty());
    }

    #[test]
    fn two_adjacent_opaque_voxels_cull_shared_faces() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);

        // Each voxel has 5 visible faces (shared face culled).
        // 10 faces × 8 triangles = 80 triangles × 3 verts = 240 output verts.
        assert_eq!(mesh.bark.indices.len(), 80 * 3);
        assert_eq!(mesh.bark.vertex_count(), 80 * 3);
    }

    #[test]
    fn dirt_below_trunk_culls_shared_face() {
        let mut world = one_chunk_world();
        // Place Dirt and a trunk voxel above it.
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::Dirt);
        world.set(VoxelCoord::new(8, 1, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);

        // Both are opaque: the shared face is culled.
        // Trunk = 5 visible faces (bark), Dirt = 5 visible faces (ground).
        // 5 faces × 8 triangles × 3 verts each.
        assert_eq!(mesh.bark.indices.len(), 5 * 8 * 3);
        assert_eq!(mesh.ground.indices.len(), 5 * 8 * 3);
    }

    #[test]
    fn leaf_to_leaf_faces_preserved() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Leaf);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);

        // Leaves are now shell-only: leaf↔leaf shared face is culled.
        // Each leaf has 5 visible faces → 10 faces × 8 tri × 3 verts.
        assert_eq!(mesh.leaf.indices.len(), 10 * 8 * 3);
        assert!(mesh.bark.is_empty());
    }

    #[test]
    fn leaf_to_opaque_face_culled() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);

        // Leaf: 5 faces (face toward trunk culled). Smooth mesh.
        assert_eq!(mesh.leaf.indices.len(), 5 * 8 * 3);
        // Trunk: 6 faces (face toward leaf NOT culled — wood visible
        // through semi-transparent leaves).
        assert_eq!(mesh.bark.indices.len(), 6 * 8 * 3);
    }

    #[test]
    fn chunk_boundary_neighbor_check() {
        set_decimation_enabled(false);
        // World is 32 voxels wide (2 chunks). Place voxels at chunk boundary.
        let mut world = VoxelWorld::new(32, 16, 16);
        world.set(VoxelCoord::new(15, 8, 8), VoxelType::Trunk); // last voxel in chunk 0
        world.set(VoxelCoord::new(16, 8, 8), VoxelType::Trunk); // first voxel in chunk 1

        let mesh0 = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        let mesh1 = generate_chunk_mesh(&world, ChunkCoord::new(1, 0, 0), None);

        // Each should have 5 faces (shared face culled across chunk boundary).
        // With smooth mesh and border, both chunks see the other voxel and
        // generate its geometry too, but the shared face is still culled.
        // Both chunks should produce non-empty geometry.
        assert!(!mesh0.bark.is_empty());
        assert!(!mesh1.bark.is_empty());
        // Both should have the same vertex count (symmetric configuration).
        assert_eq!(mesh0.bark.vertex_count(), mesh1.bark.vertex_count());
    }

    #[test]
    fn voxel_to_chunk_positive() {
        assert_eq!(
            voxel_to_chunk(VoxelCoord::new(0, 0, 0)),
            ChunkCoord::new(0, 0, 0)
        );
        assert_eq!(
            voxel_to_chunk(VoxelCoord::new(15, 15, 15)),
            ChunkCoord::new(0, 0, 0)
        );
        assert_eq!(
            voxel_to_chunk(VoxelCoord::new(16, 0, 0)),
            ChunkCoord::new(1, 0, 0)
        );
        assert_eq!(
            voxel_to_chunk(VoxelCoord::new(31, 31, 31)),
            ChunkCoord::new(1, 1, 1)
        );
    }

    #[test]
    fn voxel_to_chunk_negative() {
        // Negative coordinates should map to negative chunk coords.
        assert_eq!(
            voxel_to_chunk(VoxelCoord::new(-1, 0, 0)),
            ChunkCoord::new(-1, 0, 0)
        );
        assert_eq!(
            voxel_to_chunk(VoxelCoord::new(-16, 0, 0)),
            ChunkCoord::new(-1, 0, 0)
        );
        assert_eq!(
            voxel_to_chunk(VoxelCoord::new(-17, 0, 0)),
            ChunkCoord::new(-2, 0, 0)
        );
    }

    #[test]
    fn construction_voxels_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::GrownPlatform);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        // Smooth mesh: 6 faces × 8 triangles = 48 triangles.
        assert_eq!(mesh.bark.indices.len(), 48 * 3);
    }

    #[test]
    fn vertex_colors_match_voxel_type() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);

        // Check first vertex color is trunk color.
        let expected = voxel_color(VoxelType::Trunk);
        assert_eq!(mesh.bark.colors[0], expected[0]);
        assert_eq!(mesh.bark.colors[1], expected[1]);
        assert_eq!(mesh.bark.colors[2], expected[2]);
        assert_eq!(mesh.bark.colors[3], expected[3]);
    }

    #[test]
    fn dirt_goes_to_ground_surface() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Dirt);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);

        // Dirt should be on the ground surface, not bark.
        assert!(mesh.bark.is_empty());
        // Smooth mesh: 6 faces × 8 triangles.
        assert_eq!(mesh.ground.indices.len(), 48 * 3);
        assert!(mesh.leaf.is_empty());
        // Smooth ground has no UVs.
        assert!(mesh.ground.uvs.is_empty());
    }

    #[test]
    fn leaf_surface_no_uvs() {
        // Leaf now uses procedural noise shader — no UVs needed.
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        assert!(mesh.leaf.uvs.is_empty());
        // Should have smooth mesh geometry: 6 faces × 8 tri × 3 verts.
        assert_eq!(mesh.leaf.indices.len(), 6 * 8 * 3);
    }

    #[test]
    fn fruit_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Fruit);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        assert!(mesh.is_empty());
    }

    #[test]
    fn building_interior_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::BuildingInterior);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        assert!(mesh.is_empty());
    }

    // --- Y cutoff tests ---

    #[test]
    fn y_cutoff_hides_voxels_at_and_above_cutoff() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 5, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(8, 10, 8), VoxelType::Trunk);

        // Cutoff at y=8: voxel at y=5 visible, voxel at y=10 hidden.
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), Some(8));
        // Only the y=5 voxel should produce geometry (6 faces, smooth).
        assert_eq!(mesh.bark.indices.len(), 48 * 3);
    }

    #[test]
    fn y_cutoff_exposes_boundary_faces() {
        let mut world = one_chunk_world();
        // Stack two trunk voxels vertically.
        world.set(VoxelCoord::new(8, 7, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Without cutoff: shared +Y/-Y face is culled → 10 faces total.
        let mesh_no_cutoff = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        assert_eq!(mesh_no_cutoff.bark.indices.len(), 10 * 8 * 3);

        // With cutoff at y=8: upper voxel hidden, lower voxel's +Y face exposed.
        let mesh_cutoff = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), Some(8));
        // Lower voxel now has all 6 faces visible (upper neighbor treated as air).
        assert_eq!(mesh_cutoff.bark.indices.len(), 6 * 8 * 3);
    }

    #[test]
    fn y_cutoff_none_matches_no_cutoff() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mesh_none = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        let mesh_some = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), Some(100));

        assert_eq!(mesh_none.bark.vertex_count(), mesh_some.bark.vertex_count());
    }

    #[test]
    fn y_cutoff_at_voxel_hides_that_voxel() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Cutoff exactly at voxel Y — the voxel is at the cutoff, so hidden.
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), Some(8));
        assert!(mesh.is_empty());
    }

    #[test]
    fn y_cutoff_leaf_voxels_hidden_above() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 5, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(8, 10, 8), VoxelType::Leaf);

        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), Some(8));
        // Only y=5 leaf visible (6 faces, smooth mesh).
        assert_eq!(mesh.leaf.indices.len(), 6 * 8 * 3);
        assert!(mesh.bark.is_empty());
    }

    // --- RLE span clipping tests ---

    #[test]
    fn span_clips_to_chunk_y_range() {
        set_decimation_enabled(false);
        // World with 2 vertical chunks (32 tall). Place a tall column of trunk
        // spanning y=10..25 (crosses the chunk boundary at y=16).
        let mut world = VoxelWorld::new(16, 32, 16);
        for y in 10..26 {
            world.set(VoxelCoord::new(4, y, 4), VoxelType::Trunk);
        }

        let mesh0 = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        let mesh1 = generate_chunk_mesh(&world, ChunkCoord::new(0, 1, 0), None);

        // Both chunks should produce non-empty bark geometry. With smooth mesh
        // and the border, exact counts depend on deduplication across the
        // extended range. Just verify the topology is correct: chunk 0 sees
        // the column y=10..15 plus border context, chunk 1 sees y=16..25
        // plus border context.
        assert!(!mesh0.bark.is_empty());
        assert!(!mesh1.bark.is_empty());
        // Chunk 1 has more voxels (10 vs 6), so should have more geometry.
        assert!(mesh1.bark.indices.len() > mesh0.bark.indices.len());
    }

    #[test]
    fn empty_chunk_in_tall_world() {
        set_decimation_enabled(false);
        // A tall world where a chunk far above the content is empty.
        let mut world = VoxelWorld::new(16, 128, 16);
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::Trunk);

        // Chunk at y=64..79 should be empty.
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 4, 0), None);
        assert!(mesh.is_empty());
    }

    #[test]
    fn y_cutoff_below_chunk_produces_empty_mesh() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Cutoff at y=0 means every voxel in chunk 0 (y=0..15) is at or above
        // the cutoff. The early-return optimization should produce an empty mesh.
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), Some(0));
        assert!(mesh.is_empty());
    }

    #[test]
    fn strut_voxel_produces_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Strut);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        // 6 faces × 8 triangles (smooth mesh).
        assert_eq!(mesh.bark.indices.len(), 48 * 3);
    }

    #[test]
    fn world_smaller_than_chunk_skips_out_of_bounds_columns() {
        // World is only 8×16×8 but chunk footprint is 16×16. The 8 columns
        // outside the world in each dimension should be skipped.
        set_decimation_enabled(false);
        let mut world = VoxelWorld::new(8, 16, 8);
        world.set(VoxelCoord::new(4, 4, 4), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        assert_eq!(mesh.bark.indices.len(), 48 * 3); // 6 faces smooth
    }

    #[test]
    fn y_cutoff_within_contiguous_span() {
        // A trunk column from y=0..9 with cutoff=5 should mesh only y=0..4
        // and expose the +Y cap face at y=4.
        let mut world = one_chunk_world();
        for y in 0..10 {
            world.set(VoxelCoord::new(8, y, 8), VoxelType::Trunk);
        }
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), Some(5));

        // 5 visible voxels (y=0..4). Internal ±Y faces are culled.
        // - 4 side faces × 5 = 20 side faces
        // - 1 bottom face (y=0, -Y toward OOB = Air)
        // - 1 top face (y=4, +Y toward y=5 which is above cutoff = treated as Air)
        // Total: 22 faces × 8 triangles = 176 triangles
        assert_eq!(mesh.bark.indices.len(), 22 * 8 * 3);
    }

    #[test]
    fn negative_chunk_coords_produce_empty_mesh() {
        let world = one_chunk_world();
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(-1, 0, 0), None);
        assert!(mesh.is_empty());
    }

    #[test]
    fn estimate_byte_size_empty() {
        let mesh = ChunkMesh::default();
        assert_eq!(mesh.estimate_byte_size(), 0);
    }

    #[test]
    fn estimate_byte_size_single_voxel() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        let size = mesh.estimate_byte_size();
        // Smooth mesh: 26 verts, 48 triangles.
        // vertices: 26*3*4=312, normals: 312, indices: 144*4=576,
        // colors: 26*4*4=416, uvs: 0.
        assert!(size > 0);
        assert!(size >= 312 + 312 + 576 + 416);
    }

    /// Collect (position, normal) pairs from a SurfaceMesh. If tolerance is
    /// large, collects all vertices; otherwise filters to those near boundary_x.
    fn collect_boundary_verts(
        surface: &SurfaceMesh,
        boundary_x: f32,
        tolerance: f32,
    ) -> Vec<([f32; 3], [f32; 3])> {
        (0..surface.vertex_count())
            .filter_map(|i| {
                let x = surface.vertices[i * 3];
                if (x - boundary_x).abs() < tolerance {
                    Some((
                        [x, surface.vertices[i * 3 + 1], surface.vertices[i * 3 + 2]],
                        [
                            surface.normals[i * 3],
                            surface.normals[i * 3 + 1],
                            surface.normals[i * 3 + 2],
                        ],
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn chunk_boundary_smooth_mesh_alignment() {
        // End-to-end test using the actual generate_chunk_mesh production
        // code. For two adjacent chunks sharing a boundary at x=16, every
        // vertex that appears in BOTH chunks' output must have IDENTICAL
        // position AND normal (within float epsilon).
        set_decimation_enabled(false);
        let mut world = VoxelWorld::new(32, 16, 16);
        for x in 0..32 {
            let height = 3 + (x % 3);
            for y in 0..height {
                for z in 0..16 {
                    world.set(VoxelCoord::new(x, y, z), VoxelType::Dirt);
                }
            }
        }

        let mesh0 = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0), None);
        let mesh1 = generate_chunk_mesh(&world, ChunkCoord::new(1, 0, 0), None);

        assert!(!mesh0.ground.is_empty(), "chunk 0 should have ground");
        assert!(!mesh1.ground.is_empty(), "chunk 1 should have ground");

        let pos_eps = 1e-6;
        let normal_eps = 1e-5;

        // Collect vertices near the chunk boundary at x=16 (not world edges).
        let verts_0 = collect_boundary_verts(&mesh0.ground, 16.0, 1.0);
        let verts_1 = collect_boundary_verts(&mesh1.ground, 16.0, 1.0);

        // For each vertex in chunk 0 near the chunk boundary (but away
        // from world edges at y=0 and z=0/z=15), check if chunk 1 has a
        // vertex at the same position with matching normal.
        let mut shared = 0;
        let mut normal_mismatch = 0;
        let mut details: Vec<String> = Vec::new();

        for (p0, n0) in &verts_0 {
            // Skip vertices at world edges where border asymmetry causes
            // expected normal differences.
            if p0[1] < 1.0 || p0[2] < 1.0 || p0[2] > 15.0 {
                continue;
            }
            for (p1, n1) in &verts_1 {
                let d =
                    ((p0[0] - p1[0]).powi(2) + (p0[1] - p1[1]).powi(2) + (p0[2] - p1[2]).powi(2))
                        .sqrt();
                if d < pos_eps {
                    shared += 1;
                    let nd = ((n0[0] - n1[0]).powi(2)
                        + (n0[1] - n1[1]).powi(2)
                        + (n0[2] - n1[2]).powi(2))
                    .sqrt();
                    if nd >= normal_eps {
                        normal_mismatch += 1;
                        if details.len() < 3 {
                            details.push(format!(
                                "NORMAL MISMATCH pos={p0:?} n0={n0:?} n1={n1:?} nd={nd}"
                            ));
                        }
                    }
                    break;
                }
            }
        }

        assert!(shared > 0, "chunks should share some boundary vertices");
        assert_eq!(
            normal_mismatch,
            0,
            "all shared vertices must have matching normals \
             ({shared} shared, {normal_mismatch} mismatches)\n{}",
            details.join("\n")
        );
    }

    #[test]
    fn produces_geometry_classification() {
        assert!(produces_geometry(VoxelType::Trunk));
        assert!(produces_geometry(VoxelType::Leaf));
        assert!(produces_geometry(VoxelType::Dirt));
        assert!(produces_geometry(VoxelType::Strut));
        assert!(!produces_geometry(VoxelType::Air));
        assert!(!produces_geometry(VoxelType::Fruit));
    }
}
