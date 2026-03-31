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

use crate::chunk_neighborhood::ChunkNeighborhood;
use crate::smooth_mesh::{SmoothMesh, TAG_BARK, TAG_GROUND, TAG_LEAF};

/// Configuration for the mesh generation pipeline. Controls which stages run
/// (smoothing, decimation) and their parameters. Passed through the pipeline
/// instead of using global state, enabling safe parallel test execution and
/// concurrent mesh generation with different settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshPipelineConfig {
    /// Enable the iterative smoothing pass after chamfering. When false, only
    /// chamfering is applied. Debug tool for comparing chamfer-only vs
    /// chamfer+smoothing output.
    pub smoothing_enabled: bool,
    /// Use smooth area-weighted vertex normals instead of flat per-face normals.
    pub smooth_normals_enabled: bool,
    /// Enable QEM mesh decimation to reduce triangle count after chamfering
    /// (and optionally smoothing).
    pub decimation_enabled: bool,
    /// When true, skip coplanar retri and collinear passes — only run QEM
    /// edge-collapse.
    pub qem_only: bool,
    /// Maximum quadric error for mesh decimation. Lower values preserve more
    /// detail; near-zero (1e-6) is lossless for flat-shaded chamfered meshes.
    pub decimation_max_error: f32,
}

impl Default for MeshPipelineConfig {
    fn default() -> Self {
        Self {
            smoothing_enabled: false,
            smooth_normals_enabled: false,
            decimation_enabled: false,
            qem_only: false,
            decimation_max_error: 1e-6,
        }
    }
}
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

/// Side length of a chunk in voxels.
pub const CHUNK_SIZE: i32 = 16;

/// A chunk coordinate in chunk-space (each unit = 16 voxels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// Export a `ChunkMesh` as Wavefront OBJ text. Combines all surfaces
/// (bark, ground, leaf) into one mesh with named groups.
pub fn chunk_mesh_to_obj(mesh: &ChunkMesh) -> String {
    let mut obj = String::from("# Elven Canopy chunk mesh export\n");
    let mut vertex_offset = 0u32;

    for (name, surface) in [
        ("bark", &mesh.bark),
        ("ground", &mesh.ground),
        ("leaf", &mesh.leaf),
    ] {
        if surface.is_empty() {
            continue;
        }
        // OBJ uses the same index for vertex and normal (f a//a). This
        // requires 1:1 correspondence between vertices and normals.
        debug_assert_eq!(
            surface.vertices.len(),
            surface.normals.len(),
            "OBJ export requires vertex/normal count parity"
        );
        obj.push_str(&format!("g {name}\n"));
        let vert_count = surface.vertices.len() / 3;
        for i in 0..vert_count {
            let x = surface.vertices[i * 3];
            let y = surface.vertices[i * 3 + 1];
            let z = surface.vertices[i * 3 + 2];
            obj.push_str(&format!("v {x} {y} {z}\n"));
        }
        let norm_count = surface.normals.len() / 3;
        for i in 0..norm_count {
            let nx = surface.normals[i * 3];
            let ny = surface.normals[i * 3 + 1];
            let nz = surface.normals[i * 3 + 2];
            obj.push_str(&format!("vn {nx} {ny} {nz}\n"));
        }
        let tri_count = surface.indices.len() / 3;
        for i in 0..tri_count {
            let a = surface.indices[i * 3] + 1 + vertex_offset;
            let b = surface.indices[i * 3 + 1] + 1 + vertex_offset;
            let c = surface.indices[i * 3 + 2] + 1 + vertex_offset;
            obj.push_str(&format!("f {a}//{a} {b}//{b} {c}//{c}\n"));
        }
        vertex_offset += vert_count as u32;
    }
    obj
}

/// Generate a chunk mesh with decimation explicitly enabled or disabled,
/// overriding the config's `decimation_enabled` field. Thread-safe: creates
/// a modified config copy instead of mutating global state.
pub fn generate_chunk_mesh_with_decimation(
    world: &VoxelWorld,
    chunk: ChunkCoord,
    y_cutoff: Option<i32>,
    decimate: bool,
    grassless: &std::collections::BTreeSet<VoxelCoord>,
    config: &MeshPipelineConfig,
) -> ChunkMesh {
    let mut cfg = *config;
    cfg.decimation_enabled = decimate;
    generate_chunk_mesh_from_world(world, chunk, y_cutoff, grassless, &cfg)
}

/// Convenience wrapper: extracts a `ChunkNeighborhood` from the world and
/// generates the mesh. Use this when you have a `&VoxelWorld` and don't need
/// to decouple extraction from generation (tests, synchronous callers).
pub fn generate_chunk_mesh_from_world(
    world: &VoxelWorld,
    chunk: ChunkCoord,
    y_cutoff: Option<i32>,
    grassless: &std::collections::BTreeSet<VoxelCoord>,
    config: &MeshPipelineConfig,
) -> ChunkMesh {
    let neighborhood = ChunkNeighborhood::extract(world, chunk, y_cutoff, grassless);
    generate_chunk_mesh(&neighborhood, config)
}

/// Vertex color for grassless (grazed) dirt — earthy brown instead of the
/// default green. Used by the mesh generator when a dirt voxel is in the
/// grassless set.
pub const GRASSLESS_DIRT_COLOR: [f32; 4] = [0.40, 0.30, 0.15, 1.0];

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
/// Also used by `ChunkNeighborhood::extract` to determine the capture region.
pub const SMOOTH_BORDER: i32 = 2;

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

/// Compute the chunk bounds (min/max voxel coordinates) for a chunk.
///
/// Used by both the face-generation and post-processing stages for
/// boundary pinning and output filtering.
pub fn chunk_bounds(chunk: ChunkCoord) -> ([i32; 3], [i32; 3]) {
    let base_x = chunk.cx * CHUNK_SIZE;
    let base_y = chunk.cy * CHUNK_SIZE;
    let base_z = chunk.cz * CHUNK_SIZE;
    let chunk_min = [base_x, base_y, base_z];
    let chunk_max = [
        base_x + CHUNK_SIZE,
        base_y + CHUNK_SIZE,
        base_z + CHUNK_SIZE,
    ];
    (chunk_min, chunk_max)
}

/// Build the raw subdivided `SmoothMesh` from a neighborhood snapshot.
///
/// This is the face-generation stage of the mesh pipeline: it iterates
/// voxels in the chunk + border region, creates 8-triangle subdivided faces
/// for every visible surface, and returns the resulting `SmoothMesh` before
/// any smoothing, chamfering, or decimation. Returns `None` if the chunk
/// is empty or entirely above `y_cutoff`.
///
/// Exposed as a public function so benchmarks can measure face generation
/// independently from the post-processing stages.
pub fn build_smooth_mesh(
    nh: &ChunkNeighborhood,
    config: &MeshPipelineConfig,
) -> Option<SmoothMesh> {
    let chunk = nh.chunk;
    let y_cutoff = nh.y_cutoff;

    let base_x = chunk.cx * CHUNK_SIZE;
    let base_y = chunk.cy * CHUNK_SIZE;
    let base_z = chunk.cz * CHUNK_SIZE;
    let chunk_y_end = base_y + CHUNK_SIZE - 1;

    // Effective Y ceiling: the lower of the chunk top and the y_cutoff.
    let effective_y_end = match y_cutoff {
        Some(cutoff) => chunk_y_end.min(cutoff.saturating_sub(1)),
        None => chunk_y_end,
    };

    // If the entire chunk is above the cutoff, produce nothing.
    if effective_y_end < base_y {
        return None;
    }

    let border = SMOOTH_BORDER;
    let smooth_min_x = (base_x - border).max(0);
    let smooth_max_x = (base_x + CHUNK_SIZE + border).min(nh.world_size_x as i32);
    let smooth_min_y = (base_y - border).max(0);
    let smooth_max_y_unbounded = (base_y + CHUNK_SIZE + border).min(nh.world_size_y as i32);
    let smooth_min_z = (base_z - border).max(0);
    let smooth_max_z = (base_z + CHUNK_SIZE + border).min(nh.world_size_z as i32);

    // Pre-estimate face count to avoid repeated Vec/HashMap reallocations.
    // The iteration volume covers (chunk + border)³ voxels. Surface voxels
    // in a typical chunk contribute ~2 visible faces each on average. A chunk
    // with 20³ = 8000 voxels and ~30% surface exposure gives ~4800 faces —
    // over-estimating slightly is cheaper than under-estimating and rehashing.
    let volume = (smooth_max_x - smooth_min_x) as usize
        * (smooth_max_y_unbounded.min(effective_y_end + 1 + border) - smooth_min_y).max(0) as usize
        * (smooth_max_z - smooth_min_z) as usize;
    // Heuristic: ~1 visible face per 3 voxels in the iteration volume.
    // This over-estimates for solid interiors (fully culled) and under-
    // estimates for thin shells, but is a good middle ground that avoids
    // the worst-case reallocation chains.
    let face_estimate = volume / 3;

    // Iterate the chunk + border region, building a single unified SmoothMesh
    // for all solid surfaces. Using one mesh ensures that vertices shared
    // between different voxel types (e.g., trunk/dirt boundary) are smoothed
    // together, preventing seams. The mesh is split into bark and ground
    // surfaces at the output stage using per-triangle surface tags.
    let mut solid_smooth = SmoothMesh::with_estimated_faces(face_estimate, *config);

    // Effective Y ceiling for smooth mesh includes the border.
    let smooth_effective_y_end = match y_cutoff {
        Some(cutoff) => smooth_max_y_unbounded.min(cutoff.saturating_sub(1)),
        None => smooth_max_y_unbounded,
    };

    for wz in smooth_min_z..smooth_max_z {
        for wx in smooth_min_x..smooth_max_x {
            for (vt, y_start, y_end) in nh.column_spans(wx as u32, wz as u32) {
                if !is_smooth_voxel(vt) {
                    continue;
                }

                let clipped_start = (y_start as i32).max(smooth_min_y);
                let clipped_end = (y_end as i32).min(smooth_effective_y_end);
                if clipped_start > clipped_end {
                    continue;
                }

                let base_color = voxel_color(vt);
                let is_leaf = vt == VoxelType::Leaf;
                let is_dirt = vt == VoxelType::Dirt;
                let tag = if is_leaf {
                    TAG_LEAF
                } else if is_ground_voxel(vt) {
                    TAG_GROUND
                } else {
                    TAG_BARK
                };

                for wy in clipped_start..=clipped_end {
                    // For dirt voxels, check grassless status per-voxel.
                    let color = if is_dirt {
                        let coord = VoxelCoord::new(wx, wy, wz);
                        if nh.grassless.contains(&coord) {
                            GRASSLESS_DIRT_COLOR
                        } else {
                            base_color
                        }
                    } else {
                        base_color
                    };

                    for (face_idx, &(dx, dy, dz)) in FACES.iter().enumerate() {
                        let ny = wy + dy;
                        let neighbor = nh.get(VoxelCoord::new(wx + dx, ny, wz + dz));

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

    if solid_smooth.vertices.is_empty() {
        None
    } else {
        Some(solid_smooth)
    }
}

/// Run the chamfer/smooth pipeline on a `SmoothMesh`.
///
/// This performs: resolve non-manifold, normalize initial normals,
/// apply anchoring, chamfer, and optionally smooth (if `config.smoothing_enabled`).
/// Exposed as a public function so benchmarks can measure this stage
/// independently.
pub fn run_chamfer_smooth(mesh: &mut SmoothMesh) {
    mesh.resolve_non_manifold();
    mesh.normalize_initial_normals();
    mesh.apply_anchoring();
    mesh.chamfer();
    if mesh.config.smoothing_enabled {
        mesh.smooth();
    }
}

/// Run the decimation pipeline on a `SmoothMesh`.
///
/// This performs: coplanar region re-triangulation, collinear boundary
/// vertex collapse, and QEM edge-collapse (unless `config.qem_only` is set,
/// in which case only the QEM pass runs). Exposed as a public function
/// so benchmarks can measure this stage independently.
pub fn run_decimation(mesh: &mut SmoothMesh, chunk: ChunkCoord) {
    let (chunk_min, chunk_max) = chunk_bounds(chunk);
    if !mesh.config.qem_only {
        mesh.coplanar_region_retri(Some((chunk_min, chunk_max)));
        mesh.collapse_collinear_boundary_vertices(Some((chunk_min, chunk_max)));
    }
    mesh.decimate(
        mesh.config.decimation_max_error,
        Some((chunk_min, chunk_max)),
    );
}

/// Flatten a `SmoothMesh` into a `ChunkMesh`, filtering to chunk bounds.
///
/// Splits the unified smooth mesh by surface tag into bark, ground, and
/// leaf surfaces. Only triangles whose source voxel position falls within
/// the chunk bounds are included. Exposed as a public function so
/// benchmarks can measure this stage independently.
pub fn flatten_to_chunk_mesh(mesh: &SmoothMesh, chunk: ChunkCoord) -> ChunkMesh {
    let (chunk_min, chunk_max) = chunk_bounds(chunk);
    let mut result = ChunkMesh::default();
    let mut surfaces = mesh.to_split_surface_meshes_filtered(chunk_min, chunk_max);
    if let Some(bark) = surfaces.remove(&TAG_BARK) {
        result.bark = bark;
    }
    if let Some(ground) = surfaces.remove(&TAG_GROUND) {
        result.ground = ground;
    }
    if let Some(leaf) = surfaces.remove(&TAG_LEAF) {
        result.leaf = leaf;
    }
    result
}

/// Generate a chunk mesh from a pre-extracted neighborhood snapshot.
///
/// This is the core mesh generation entry point for the async pipeline:
/// the neighborhood is extracted on the main thread (fast), then this
/// function runs on a background worker with no world access.
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
pub fn generate_chunk_mesh(nh: &ChunkNeighborhood, config: &MeshPipelineConfig) -> ChunkMesh {
    let Some(mut solid_smooth) = build_smooth_mesh(nh, config) else {
        return ChunkMesh::default();
    };

    run_chamfer_smooth(&mut solid_smooth);
    if config.decimation_enabled {
        run_decimation(&mut solid_smooth, nh.chunk);
    }
    flatten_to_chunk_mesh(&solid_smooth, nh.chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Empty grassless set for tests that don't need grassless dirt.
    fn no_grassless() -> BTreeSet<VoxelCoord> {
        BTreeSet::new()
    }

    /// Config with decimation disabled — mesh_gen tests check exact triangle
    /// counts from the subdivision pipeline and should not be affected by
    /// decimation.
    fn no_decimate() -> MeshPipelineConfig {
        MeshPipelineConfig {
            decimation_enabled: false,
            ..MeshPipelineConfig::default()
        }
    }

    /// Helper: create a small world (one chunk = 16x16x16).
    fn one_chunk_world() -> VoxelWorld {
        VoxelWorld::new(16, 16, 16)
    }

    #[test]
    fn empty_chunk_produces_empty_mesh() {
        let world = one_chunk_world();
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn single_trunk_voxel_produces_smooth_mesh() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

        // Leaf: 5 faces (face toward trunk culled). Smooth mesh.
        assert_eq!(mesh.leaf.indices.len(), 5 * 8 * 3);
        // Trunk: 6 faces (face toward leaf NOT culled — wood visible
        // through semi-transparent leaves).
        assert_eq!(mesh.bark.indices.len(), 6 * 8 * 3);
    }

    #[test]
    fn chunk_boundary_neighbor_check() {
        let cfg = no_decimate();
        // World is 32 voxels wide (2 chunks). Place voxels at chunk boundary.
        let mut world = VoxelWorld::new(32, 16, 16);
        world.set(VoxelCoord::new(15, 8, 8), VoxelType::Trunk); // last voxel in chunk 0
        world.set(VoxelCoord::new(16, 8, 8), VoxelType::Trunk); // first voxel in chunk 1

        let mesh0 = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &cfg,
        );
        let mesh1 = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(1, 0, 0),
            None,
            &no_grassless(),
            &cfg,
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        // Smooth mesh: 6 faces × 8 triangles = 48 triangles.
        assert_eq!(mesh.bark.indices.len(), 48 * 3);
    }

    #[test]
    fn vertex_colors_match_voxel_type() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        assert!(mesh.leaf.uvs.is_empty());
        // Should have smooth mesh geometry: 6 faces × 8 tri × 3 verts.
        assert_eq!(mesh.leaf.indices.len(), 6 * 8 * 3);
    }

    #[test]
    fn fruit_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Fruit);
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn building_interior_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::BuildingInterior);
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        assert!(mesh.is_empty());
    }

    // --- Y cutoff tests ---

    #[test]
    fn y_cutoff_hides_voxels_at_and_above_cutoff() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 5, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(8, 10, 8), VoxelType::Trunk);

        // Cutoff at y=8: voxel at y=5 visible, voxel at y=10 hidden.
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &no_grassless(),
            &no_decimate(),
        );
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
        let mesh_no_cutoff = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        assert_eq!(mesh_no_cutoff.bark.indices.len(), 10 * 8 * 3);

        // With cutoff at y=8: upper voxel hidden, lower voxel's +Y face exposed.
        let mesh_cutoff = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &no_grassless(),
            &no_decimate(),
        );
        // Lower voxel now has all 6 faces visible (upper neighbor treated as air).
        assert_eq!(mesh_cutoff.bark.indices.len(), 6 * 8 * 3);
    }

    #[test]
    fn y_cutoff_none_matches_no_cutoff() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mesh_none = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        let mesh_some = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(100),
            &no_grassless(),
            &no_decimate(),
        );

        assert_eq!(mesh_none.bark.vertex_count(), mesh_some.bark.vertex_count());
    }

    #[test]
    fn y_cutoff_at_voxel_hides_that_voxel() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Cutoff exactly at voxel Y — the voxel is at the cutoff, so hidden.
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &no_grassless(),
            &no_decimate(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn y_cutoff_leaf_voxels_hidden_above() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 5, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(8, 10, 8), VoxelType::Leaf);

        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &no_grassless(),
            &no_decimate(),
        );
        // Only y=5 leaf visible (6 faces, smooth mesh).
        assert_eq!(mesh.leaf.indices.len(), 6 * 8 * 3);
        assert!(mesh.bark.is_empty());
    }

    // --- RLE span clipping tests ---

    #[test]
    fn span_clips_to_chunk_y_range() {
        // World with 2 vertical chunks (32 tall). Place a tall column of trunk
        // spanning y=10..25 (crosses the chunk boundary at y=16).
        let mut world = VoxelWorld::new(16, 32, 16);
        for y in 10..26 {
            world.set(VoxelCoord::new(4, y, 4), VoxelType::Trunk);
        }

        let mesh0 = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        let mesh1 = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 1, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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
        // A tall world where a chunk far above the content is empty.
        let mut world = VoxelWorld::new(16, 128, 16);
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::Trunk);

        // Chunk at y=64..79 should be empty.
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 4, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn y_cutoff_below_chunk_produces_empty_mesh() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Cutoff at y=0 means every voxel in chunk 0 (y=0..15) is at or above
        // the cutoff. The early-return optimization should produce an empty mesh.
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(0),
            &no_grassless(),
            &no_decimate(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn strut_voxel_produces_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Strut);
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        // 6 faces × 8 triangles (smooth mesh).
        assert_eq!(mesh.bark.indices.len(), 48 * 3);
    }

    #[test]
    fn world_smaller_than_chunk_skips_out_of_bounds_columns() {
        // World is only 8×16×8 but chunk footprint is 16×16. The 8 columns
        // outside the world in each dimension should be skipped.
        let mut world = VoxelWorld::new(8, 16, 8);
        world.set(VoxelCoord::new(4, 4, 4), VoxelType::Trunk);
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(5),
            &no_grassless(),
            &no_decimate(),
        );

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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(-1, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
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
        let mesh = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
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
        let mut world = VoxelWorld::new(32, 16, 16);
        for x in 0..32 {
            let height = 3 + (x % 3);
            for y in 0..height {
                for z in 0..16 {
                    world.set(VoxelCoord::new(x, y, z), VoxelType::Dirt);
                }
            }
        }

        let mesh0 = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );
        let mesh1 = generate_chunk_mesh_from_world(
            &world,
            ChunkCoord::new(1, 0, 0),
            None,
            &no_grassless(),
            &no_decimate(),
        );

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

    #[test]
    fn chunk_bounds_correctness() {
        let (min, max) = chunk_bounds(ChunkCoord::new(2, 3, 1));
        assert_eq!(min, [32, 48, 16]);
        assert_eq!(max, [48, 64, 32]);
    }

    #[test]
    fn chunk_bounds_origin() {
        let (min, max) = chunk_bounds(ChunkCoord::new(0, 0, 0));
        assert_eq!(min, [0, 0, 0]);
        assert_eq!(max, [16, 16, 16]);
    }

    #[test]
    fn build_smooth_mesh_empty_returns_none() {
        let world = VoxelWorld::new(16, 16, 16);
        let nh =
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &no_grassless());
        assert!(build_smooth_mesh(&nh, &no_decimate()).is_none());
    }

    #[test]
    fn sub_stages_match_generate_chunk_mesh() {
        let cfg = MeshPipelineConfig {
            decimation_enabled: true,
            smoothing_enabled: false,
            ..MeshPipelineConfig::default()
        };
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
        world.set(VoxelCoord::new(8, 7, 8), VoxelType::Dirt);
        let chunk = ChunkCoord::new(0, 0, 0);
        let nh = ChunkNeighborhood::extract(&world, chunk, None, &no_grassless());

        // Path A: monolithic function.
        let mesh_a = generate_chunk_mesh(&nh, &cfg);

        // Path B: composable sub-stages.
        let mut sm = build_smooth_mesh(&nh, &cfg).unwrap();
        run_chamfer_smooth(&mut sm);
        run_decimation(&mut sm, chunk);
        let mesh_b = flatten_to_chunk_mesh(&sm, chunk);

        assert_eq!(mesh_a.bark.vertices, mesh_b.bark.vertices);
        assert_eq!(mesh_a.bark.normals, mesh_b.bark.normals);
        assert_eq!(mesh_a.bark.indices, mesh_b.bark.indices);
        assert_eq!(mesh_a.bark.colors, mesh_b.bark.colors);
        assert_eq!(mesh_a.ground.vertices, mesh_b.ground.vertices);
        assert_eq!(mesh_a.ground.indices, mesh_b.ground.indices);
    }

    #[test]
    fn serde_roundtrip_chunk_coord() {
        let coord = ChunkCoord::new(3, -1, 7);
        let json = serde_json::to_string(&coord).unwrap();
        let restored: ChunkCoord = serde_json::from_str(&json).unwrap();
        assert_eq!(coord, restored);
    }

    #[test]
    fn mesh_pipeline_config_defaults_match_legacy_globals() {
        let cfg = MeshPipelineConfig::default();
        assert!(!cfg.smoothing_enabled);
        assert!(!cfg.smooth_normals_enabled);
        assert!(!cfg.decimation_enabled);
        assert!(!cfg.qem_only);
        assert!((cfg.decimation_max_error - 1e-6).abs() < f32::EPSILON);
    }

    /// Helper: create a multi-voxel world for config-variation tests.
    /// Returns (world, chunk, neighborhood) with 3 opaque voxels.
    fn config_test_world() -> (VoxelWorld, ChunkCoord, ChunkNeighborhood) {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
        world.set(VoxelCoord::new(8, 7, 8), VoxelType::Dirt);
        let chunk = ChunkCoord::new(0, 0, 0);
        let nh = ChunkNeighborhood::extract(&world, chunk, None, &no_grassless());
        (world, chunk, nh)
    }

    #[test]
    fn config_smoothing_enabled_changes_vertex_positions() {
        let (_, _, nh) = config_test_world();
        let no_smooth = MeshPipelineConfig {
            smoothing_enabled: false,
            decimation_enabled: false,
            ..MeshPipelineConfig::default()
        };
        let with_smooth = MeshPipelineConfig {
            smoothing_enabled: true,
            decimation_enabled: false,
            ..MeshPipelineConfig::default()
        };
        let mesh_a = generate_chunk_mesh(&nh, &no_smooth);
        let mesh_b = generate_chunk_mesh(&nh, &with_smooth);

        // Both should produce geometry, but smoothing moves vertices.
        assert!(!mesh_a.bark.is_empty());
        assert!(!mesh_b.bark.is_empty());
        assert_ne!(
            mesh_a.bark.vertices, mesh_b.bark.vertices,
            "smoothing should move vertex positions"
        );
    }

    #[test]
    fn config_smooth_normals_changes_normal_output() {
        let (_, _, nh) = config_test_world();
        let flat = MeshPipelineConfig {
            smooth_normals_enabled: false,
            decimation_enabled: false,
            ..MeshPipelineConfig::default()
        };
        let smooth = MeshPipelineConfig {
            smooth_normals_enabled: true,
            decimation_enabled: false,
            ..MeshPipelineConfig::default()
        };
        let mesh_flat = generate_chunk_mesh(&nh, &flat);
        let mesh_smooth = generate_chunk_mesh(&nh, &smooth);

        assert!(!mesh_flat.bark.is_empty());
        assert!(!mesh_smooth.bark.is_empty());
        // Same triangle count (normals don't change topology).
        assert_eq!(mesh_flat.bark.indices.len(), mesh_smooth.bark.indices.len());
        // But normals differ (flat = per-face, smooth = area-weighted).
        assert_ne!(
            mesh_flat.bark.normals, mesh_smooth.bark.normals,
            "smooth vs flat normals should produce different normal vectors"
        );
    }

    #[test]
    fn config_qem_only_skips_retri_and_collinear() {
        let (_, _, nh) = config_test_world();
        let full_decimate = MeshPipelineConfig {
            decimation_enabled: true,
            qem_only: false,
            ..MeshPipelineConfig::default()
        };
        let qem_only = MeshPipelineConfig {
            decimation_enabled: true,
            qem_only: true,
            ..MeshPipelineConfig::default()
        };
        let mesh_full = generate_chunk_mesh(&nh, &full_decimate);
        let mesh_qem = generate_chunk_mesh(&nh, &qem_only);

        // Both should produce geometry.
        assert!(!mesh_full.bark.is_empty());
        assert!(!mesh_qem.bark.is_empty());
        // Retri + collinear passes reduce triangle count further than QEM
        // alone, so full decimation should have fewer or equal triangles.
        assert!(
            mesh_full.bark.indices.len() <= mesh_qem.bark.indices.len(),
            "full decimation ({}) should produce <= triangles than QEM-only ({})",
            mesh_full.bark.indices.len(),
            mesh_qem.bark.indices.len()
        );
    }

    #[test]
    fn generate_chunk_mesh_with_decimation_overrides_config() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
        world.set(VoxelCoord::new(8, 7, 8), VoxelType::Dirt);

        // Base config has decimation disabled.
        let cfg = MeshPipelineConfig {
            decimation_enabled: false,
            ..MeshPipelineConfig::default()
        };

        // Override to enable decimation.
        let mesh_decimated = generate_chunk_mesh_with_decimation(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            true,
            &no_grassless(),
            &cfg,
        );
        // Override to disable decimation (matching the base config).
        let mesh_no_decimate = generate_chunk_mesh_with_decimation(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            false,
            &no_grassless(),
            &cfg,
        );

        assert!(!mesh_decimated.bark.is_empty());
        assert!(!mesh_no_decimate.bark.is_empty());
        // Decimation reduces triangle count.
        assert!(
            mesh_decimated.bark.indices.len() <= mesh_no_decimate.bark.indices.len(),
            "decimated ({}) should have <= triangles than undecimated ({})",
            mesh_decimated.bark.indices.len(),
            mesh_no_decimate.bark.indices.len()
        );
    }

    #[test]
    fn config_threaded_isolation() {
        // Two threads generate meshes with different configs concurrently.
        // This is the core motivation for replacing global atomics.
        // Extract two separate neighborhoods (ChunkNeighborhood is not Clone).
        fn make_nh() -> ChunkNeighborhood {
            let mut world = VoxelWorld::new(16, 16, 16);
            world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
            world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
            world.set(VoxelCoord::new(8, 7, 8), VoxelType::Dirt);
            ChunkNeighborhood::extract(&world, ChunkCoord::new(0, 0, 0), None, &BTreeSet::new())
        }

        let handle1 = std::thread::spawn(|| {
            let cfg = MeshPipelineConfig {
                smoothing_enabled: true,
                decimation_enabled: false,
                ..MeshPipelineConfig::default()
            };
            generate_chunk_mesh(&make_nh(), &cfg)
        });
        let handle2 = std::thread::spawn(|| {
            let cfg = MeshPipelineConfig {
                smoothing_enabled: false,
                decimation_enabled: false,
                ..MeshPipelineConfig::default()
            };
            generate_chunk_mesh(&make_nh(), &cfg)
        });

        let mesh1 = handle1.join().unwrap();
        let mesh2 = handle2.join().unwrap();

        // Both produce geometry but with different vertex positions
        // (smoothing vs no smoothing).
        assert!(!mesh1.bark.is_empty());
        assert!(!mesh2.bark.is_empty());
        assert_ne!(
            mesh1.bark.vertices, mesh2.bark.vertices,
            "concurrent meshes with different configs should produce different results"
        );
    }
}
