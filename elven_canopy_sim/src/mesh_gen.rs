// Chunk-based voxel mesh generation with per-face culling.
//
// Pure Rust, no Godot dependencies. Converts a region of the voxel world into
// triangle mesh data suitable for rendering. The world is divided into 16x16x16
// chunks; each chunk produces a `ChunkMesh` with three surfaces (bark, ground,
// and leaf) that the gdext bridge converts into Godot `ArrayMesh` objects.
//
// ## RLE-aware iteration
//
// Instead of iterating all 4096 voxels per chunk (16³), mesh generation walks
// each column's RLE spans via `VoxelWorld::column_spans()`, clips them to the
// chunk's Y range, and skips Air spans entirely. This means chunks in mostly-
// empty regions (the vast majority in a tall world) are nearly free. The center
// voxel type is known from the span without a `get()` call; only neighbor
// lookups for face culling still use `world.get()`.
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
// ForestFloor is opaque (blocks adjacent faces) but produces no geometry itself —
// the ground plane in the Godot scene handles floor visuals.
//
// ## Geometry
//
// Each visible face produces 4 vertices and 6 indices (2 triangles). Vertices
// include position, normal, vertex color (for material tinting), and UV
// coordinates. Opaque voxels are split into two surfaces: bark (trunk, branch,
// root, construction) and ground (dirt). Both use per-face atlas textures
// generated from 3D Perlin noise (see `texture_gen.rs`): each face gets a
// small tile in a per-chunk texture atlas, with UVs pointing into the tile.
// The 3D noise sampling ensures seamless edges between adjacent faces,
// even at corners where different orientations meet. Leaf UVs span the full
// [0,1] range for the alpha-scissor texture.
//
// See also: `world.rs` for the voxel grid and `column_spans()` API,
// `types.rs` for `VoxelType::is_opaque()`, `mesh_cache.rs` (in the gdext
// crate) for the caching layer that sits on top of this module,
// `sim_bridge.rs` for the Godot-facing API that builds `ArrayMesh` objects
// from `ChunkMesh` data, `tree_renderer.gd` for the texture generation and
// material setup on the Godot side.
//
// **Determinism note:** This module is pure and deterministic (same world state
// produces identical mesh data), but mesh generation is a rendering concern and
// does not participate in the sim's lockstep determinism contract.

use crate::texture_gen::{
    self, FACE_LOCAL_UVS, FACE_TEX_SIZE, FaceAtlas, FaceTexInfo, FaceTileCache, MaterialKind,
};
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

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
    /// UV coordinates (2 floats per vertex: u, v). For bark/ground surfaces,
    /// these point into the per-chunk texture atlas. For leaf surfaces, each
    /// face uses the full [0,1] range.
    pub uvs: Vec<f32>,
    /// Per-face texture atlas pixels (RGBA, 4 bytes per pixel). Each face
    /// gets a FACE_TEX_SIZE × FACE_TEX_SIZE tile arranged in a grid.
    /// Empty for leaf surfaces (which use a shared texture).
    pub atlas_pixels: Vec<u8>,
    /// Atlas width in pixels (0 if no atlas).
    pub atlas_width: u32,
    /// Atlas height in pixels (0 if no atlas).
    pub atlas_height: u32,
}

impl SurfaceMesh {
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// Number of vertices in this surface.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len() / 3
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
}

/// Return the vertex color for a voxel type. Returns `[r, g, b, a]`.
/// Colors match the existing GDScript renderers for visual consistency.
pub fn voxel_color(vt: VoxelType) -> [f32; 4] {
    match vt {
        VoxelType::Trunk => [0.35, 0.22, 0.10, 1.0],
        VoxelType::Branch => [0.45, 0.30, 0.15, 1.0],
        VoxelType::Root => [0.30, 0.20, 0.12, 1.0],
        VoxelType::Dirt => [0.25, 0.45, 0.20, 1.0],
        VoxelType::Leaf => [0.18, 0.55, 0.15, 1.0],
        VoxelType::GrownPlatform
        | VoxelType::GrownWall
        | VoxelType::GrownStairs
        | VoxelType::Bridge => [0.50, 0.35, 0.18, 1.0],
        // Strut: slightly darker/redder wood tint to distinguish from platforms.
        VoxelType::Strut => [0.55, 0.30, 0.15, 1.0],
        // Types that don't produce geometry — return a visible debug color.
        _ => [1.0, 0.0, 1.0, 1.0],
    }
}

/// Returns true if this voxel type should produce geometry in the chunk mesh.
/// ForestFloor is opaque (culls neighbors) but produces no geometry itself.
fn produces_geometry(vt: VoxelType) -> bool {
    matches!(
        vt,
        VoxelType::Trunk
            | VoxelType::Branch
            | VoxelType::Root
            | VoxelType::Dirt
            | VoxelType::Leaf
            | VoxelType::GrownPlatform
            | VoxelType::GrownWall
            | VoxelType::GrownStairs
            | VoxelType::Bridge
            | VoxelType::Strut
    )
}

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
const FACE_VERTICES: [[[f32; 3]; 4]; 6] = [
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

/// UV coordinates for leaf faces (same for all faces — each face shows the
/// full leaf texture). Bark/ground faces use per-face atlas UVs instead.
const LEAF_FACE_UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

/// Returns true if this opaque voxel type belongs to the ground surface.
fn is_ground_voxel(vt: VoxelType) -> bool {
    vt == VoxelType::Dirt
}

/// Record of a pending face that needs atlas UV fixup after atlas generation.
struct PendingFace {
    /// Voxel world coordinates.
    wx: i32,
    wy: i32,
    wz: i32,
    /// Face direction index (0..5).
    face_idx: usize,
    /// Starting index in the surface's `uvs` Vec for this face's 4 vertices
    /// (8 floats: u0,v0, u1,v1, u2,v2, u3,v3).
    uv_start: usize,
}

/// Generate the mesh data for a single chunk of the world.
///
/// Iterates column spans via `VoxelWorld::column_spans()` instead of per-voxel
/// `get()` calls. For each (x, z) column in the chunk's 16×16 XZ footprint,
/// the column's RLE spans are clipped to the chunk's Y range. Air spans are
/// skipped entirely, so chunks in mostly-empty regions are nearly free.
///
/// Neighbor lookups for face culling still use `world.get()` since neighbors
/// can be in different columns or chunks. The main win is eliminating the
/// center-voxel `get()` call and skipping Air ranges.
///
/// After emitting all face geometry, generates per-face Perlin noise texture
/// atlases for bark and ground surfaces, and fixes up their UVs to point into
/// the atlas tiles.
///
/// If `y_cutoff` is `Some(y)`, voxels with world Y ≥ y are treated as air:
/// they produce no geometry, and neighbors facing them get their faces exposed.
/// This lets the renderer hide everything above the camera's focus height while
/// correctly showing the "cap" faces at the cut boundary.
pub fn generate_chunk_mesh(
    world: &VoxelWorld,
    chunk: ChunkCoord,
    y_cutoff: Option<i32>,
    tile_cache: &mut FaceTileCache,
) -> ChunkMesh {
    let mut mesh = ChunkMesh::default();
    let mut bark_pending: Vec<PendingFace> = Vec::new();
    let mut ground_pending: Vec<PendingFace> = Vec::new();

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

    for lz in 0..CHUNK_SIZE {
        for lx in 0..CHUNK_SIZE {
            let wx = base_x + lx;
            let wz = base_z + lz;

            // Skip columns outside the world bounds.
            if wx < 0 || wz < 0 || (wx as u32) >= world.size_x || (wz as u32) >= world.size_z {
                continue;
            }

            for (vt, y_start, y_end) in world.column_spans(wx as u32, wz as u32) {
                if !produces_geometry(vt) {
                    continue;
                }

                // Clip span to this chunk's Y range and the effective ceiling.
                let clipped_start = (y_start as i32).max(base_y);
                let clipped_end = (y_end as i32).min(effective_y_end);
                if clipped_start > clipped_end {
                    continue;
                }

                let color = voxel_color(vt);
                let is_leaf = vt == VoxelType::Leaf;
                let is_ground = is_ground_voxel(vt);
                let surface = if is_leaf {
                    &mut mesh.leaf
                } else if is_ground {
                    &mut mesh.ground
                } else {
                    &mut mesh.bark
                };

                for wy in clipped_start..=clipped_end {
                    for (face_idx, &(dx, dy, dz)) in FACES.iter().enumerate() {
                        let ny = wy + dy;
                        let neighbor = world.get(VoxelCoord::new(wx + dx, ny, wz + dz));

                        // Treat above-cutoff neighbors as air so boundary faces
                        // are exposed when the height cutoff is active.
                        let neighbor_visible = match y_cutoff {
                            Some(cutoff) if ny >= cutoff => false,
                            _ => neighbor.is_opaque(),
                        };

                        // Cull face if neighbor is opaque and visible.
                        if neighbor_visible {
                            continue;
                        }

                        let base_vertex = surface.vertex_count() as u32;
                        let normal = [dx as f32, dy as f32, dz as f32];

                        // Record UV start for atlas fixup (bark/ground only).
                        let uv_start = surface.uvs.len();

                        // Emit 4 vertices for this face.
                        for (vi, vert) in FACE_VERTICES[face_idx].iter().enumerate() {
                            surface.vertices.push(vert[0] + wx as f32);
                            surface.vertices.push(vert[1] + wy as f32);
                            surface.vertices.push(vert[2] + wz as f32);

                            surface.normals.push(normal[0]);
                            surface.normals.push(normal[1]);
                            surface.normals.push(normal[2]);

                            surface.colors.push(color[0]);
                            surface.colors.push(color[1]);
                            surface.colors.push(color[2]);
                            surface.colors.push(color[3]);

                            if is_leaf {
                                surface.uvs.push(LEAF_FACE_UVS[vi][0]);
                                surface.uvs.push(LEAF_FACE_UVS[vi][1]);
                            } else {
                                // Placeholder UVs — will be overwritten by atlas fixup.
                                surface.uvs.push(0.0);
                                surface.uvs.push(0.0);
                            }
                        }

                        // 2 triangles: 0-1-2, 0-2-3
                        surface.indices.push(base_vertex);
                        surface.indices.push(base_vertex + 1);
                        surface.indices.push(base_vertex + 2);
                        surface.indices.push(base_vertex);
                        surface.indices.push(base_vertex + 2);
                        surface.indices.push(base_vertex + 3);

                        // Record pending face for atlas generation.
                        if !is_leaf {
                            let pending = PendingFace {
                                wx,
                                wy,
                                wz,
                                face_idx,
                                uv_start,
                            };
                            if is_ground {
                                ground_pending.push(pending);
                            } else {
                                bark_pending.push(pending);
                            }
                        }
                    }
                }
            }
        }
    }

    // Generate texture atlases and fix up UVs for bark and ground surfaces.
    if !bark_pending.is_empty() {
        let atlas = build_atlas_and_fixup(
            &mut mesh.bark,
            &bark_pending,
            MaterialKind::Bark,
            tile_cache,
        );
        mesh.bark.atlas_pixels = atlas.pixels;
        mesh.bark.atlas_width = atlas.width;
        mesh.bark.atlas_height = atlas.height;
    }
    if !ground_pending.is_empty() {
        let atlas = build_atlas_and_fixup(
            &mut mesh.ground,
            &ground_pending,
            MaterialKind::Ground,
            tile_cache,
        );
        mesh.ground.atlas_pixels = atlas.pixels;
        mesh.ground.atlas_width = atlas.width;
        mesh.ground.atlas_height = atlas.height;
    }

    mesh
}

/// Generate a texture atlas for a set of pending faces and fix up the UV
/// coordinates in the surface mesh to point into the atlas tiles.
fn build_atlas_and_fixup(
    surface: &mut SurfaceMesh,
    pending: &[PendingFace],
    material: MaterialKind,
    tile_cache: &mut FaceTileCache,
) -> FaceAtlas {
    let face_infos: Vec<FaceTexInfo> = pending
        .iter()
        .map(|p| FaceTexInfo {
            wx: p.wx,
            wy: p.wy,
            wz: p.wz,
            face_idx: p.face_idx,
        })
        .collect();
    let atlas = texture_gen::generate_atlas_cached(&face_infos, material, tile_cache);

    // Fix up UVs: map each face's 4 vertices to their tile in the atlas.
    let tex_size = FACE_TEX_SIZE as f32;
    let atlas_w = atlas.width as f32;
    let atlas_h = atlas.height as f32;

    for (tile_idx, face) in pending.iter().enumerate() {
        let tile_col = (tile_idx as u32 % atlas.tiles_per_row) as f32;
        let tile_row = (tile_idx as u32 / atlas.tiles_per_row) as f32;
        let local_uvs = &FACE_LOCAL_UVS[face.face_idx];

        for (vi, &[lu, lv]) in local_uvs.iter().enumerate() {
            // Map face-local UV [0,1] to atlas texel centers.
            let atlas_u = (tile_col * tex_size + 0.5 + lu * (tex_size - 1.0)) / atlas_w;
            let atlas_v = (tile_row * tex_size + 0.5 + lv * (tex_size - 1.0)) / atlas_h;
            let uv_idx = face.uv_start + vi * 2;
            surface.uvs[uv_idx] = atlas_u;
            surface.uvs[uv_idx + 1] = atlas_v;
        }
    }

    atlas
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a small world (one chunk = 16x16x16) and return it.
    fn one_chunk_world() -> VoxelWorld {
        VoxelWorld::new(16, 16, 16)
    }

    #[test]
    fn empty_chunk_produces_empty_mesh() {
        let world = one_chunk_world();
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn single_trunk_voxel_produces_6_faces() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // 6 faces * 4 vertices = 24 vertices
        assert_eq!(mesh.bark.vertex_count(), 24);
        // 6 faces * 6 indices = 36 indices
        assert_eq!(mesh.bark.indices.len(), 36);
        // No leaf geometry.
        assert!(mesh.leaf.is_empty());
    }

    #[test]
    fn two_adjacent_opaque_voxels_cull_shared_faces() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Each voxel alone = 6 faces. Together they share 1 face, so each loses
        // 1 face: 2 * 5 = 10 faces. 10 * 4 = 40 vertices.
        assert_eq!(mesh.bark.vertex_count(), 40);
        assert_eq!(mesh.bark.indices.len(), 60); // 10 * 6
    }

    #[test]
    fn forest_floor_produces_no_geometry_but_culls_adjacent() {
        let mut world = one_chunk_world();
        // Place ForestFloor and a trunk voxel above it.
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::ForestFloor);
        world.set(VoxelCoord::new(8, 1, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // ForestFloor produces no geometry, but the trunk's -Y face should be
        // culled because ForestFloor is opaque. Trunk = 5 visible faces.
        assert_eq!(mesh.bark.vertex_count(), 20); // 5 * 4
        assert_eq!(mesh.bark.indices.len(), 30); // 5 * 6
    }

    #[test]
    fn leaf_to_leaf_faces_preserved() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Leaf);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Leaf is not opaque, so leaf-to-leaf faces are NOT culled.
        // Each leaf has 6 faces, both get all 6 = 12 faces total.
        assert_eq!(mesh.leaf.vertex_count(), 48); // 12 * 4
        assert_eq!(mesh.leaf.indices.len(), 72); // 12 * 6
        assert!(mesh.bark.is_empty());
    }

    #[test]
    fn leaf_to_opaque_face_culled() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Leaf: 5 faces (the +X face toward trunk is culled — trunk is opaque).
        assert_eq!(mesh.leaf.vertex_count(), 20); // 5 * 4
        // Trunk: 6 faces (the -X face toward leaf is NOT culled — leaf isn't opaque).
        assert_eq!(mesh.bark.vertex_count(), 24); // 6 * 4
    }

    #[test]
    fn chunk_boundary_neighbor_check() {
        // World is 32 voxels wide (2 chunks). Place voxels at chunk boundary.
        let mut world = VoxelWorld::new(32, 16, 16);
        world.set(VoxelCoord::new(15, 8, 8), VoxelType::Trunk); // last voxel in chunk 0
        world.set(VoxelCoord::new(16, 8, 8), VoxelType::Trunk); // first voxel in chunk 1

        let mesh0 = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        let mesh1 = generate_chunk_mesh(
            &world,
            ChunkCoord::new(1, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Each should have 5 faces (shared face culled across chunk boundary).
        assert_eq!(mesh0.bark.vertex_count(), 20);
        assert_eq!(mesh1.bark.vertex_count(), 20);
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
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert_eq!(mesh.bark.vertex_count(), 24); // 6 faces * 4 verts
    }

    #[test]
    fn vertex_colors_match_voxel_type() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
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
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Dirt should be on the ground surface, not bark.
        assert!(mesh.bark.is_empty());
        assert_eq!(mesh.ground.vertex_count(), 24); // 6 faces * 4 verts
        assert!(mesh.leaf.is_empty());
    }

    #[test]
    fn atlas_uvs_differ_between_faces() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 8, 2), VoxelType::Trunk);
        world.set(VoxelCoord::new(8, 8, 6), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Each trunk has 6 faces * 4 verts = 24 verts, total 48.
        assert_eq!(mesh.bark.vertex_count(), 48);

        // The two voxels' first faces should point to different atlas tiles,
        // so at least some UV values should differ.
        let uv0_u = mesh.bark.uvs[0];
        let uv0_v = mesh.bark.uvs[1];
        // Second voxel starts at vertex 24.
        let uv1_u = mesh.bark.uvs[48];
        let uv1_v = mesh.bark.uvs[49];

        assert!(
            uv0_u != uv1_u || uv0_v != uv1_v,
            "Atlas UVs should differ between faces: ({uv0_u},{uv0_v}) vs ({uv1_u},{uv1_v})"
        );
    }

    #[test]
    fn bark_surface_has_atlas_data() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Single voxel = 6 faces → atlas should be 3×2 tiles = 48×32 pixels.
        assert!(mesh.bark.atlas_width > 0);
        assert!(mesh.bark.atlas_height > 0);
        assert!(!mesh.bark.atlas_pixels.is_empty());
        assert_eq!(
            mesh.bark.atlas_pixels.len(),
            (mesh.bark.atlas_width * mesh.bark.atlas_height * 4) as usize
        );
    }

    #[test]
    fn leaf_surface_has_no_atlas() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );

        assert!(mesh.leaf.atlas_pixels.is_empty());
        assert_eq!(mesh.leaf.atlas_width, 0);
    }

    #[test]
    fn fruit_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Fruit);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn building_interior_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::BuildingInterior);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
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
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &mut FaceTileCache::new(),
        );
        // Only the y=5 voxel should produce geometry (6 faces).
        assert_eq!(mesh.bark.vertex_count(), 24); // 6 * 4
    }

    #[test]
    fn y_cutoff_exposes_boundary_faces() {
        let mut world = one_chunk_world();
        // Stack two trunk voxels vertically.
        world.set(VoxelCoord::new(8, 7, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Without cutoff: shared +Y/-Y face is culled → 10 faces total.
        let mesh_no_cutoff = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert_eq!(mesh_no_cutoff.bark.vertex_count(), 40); // 10 * 4

        // With cutoff at y=8: upper voxel hidden, lower voxel's +Y face exposed.
        let mesh_cutoff = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &mut FaceTileCache::new(),
        );
        // Lower voxel now has all 6 faces visible (upper neighbor treated as air).
        assert_eq!(mesh_cutoff.bark.vertex_count(), 24); // 6 * 4
    }

    #[test]
    fn y_cutoff_none_matches_no_cutoff() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        let mesh_none = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        let mesh_some = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(100), // well above voxel
            &mut FaceTileCache::new(),
        );

        assert_eq!(mesh_none.bark.vertex_count(), mesh_some.bark.vertex_count());
    }

    #[test]
    fn y_cutoff_at_voxel_hides_that_voxel() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Cutoff exactly at voxel Y — the voxel is at the cutoff, so hidden.
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &mut FaceTileCache::new(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn y_cutoff_leaf_voxels_hidden_above() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 5, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(8, 10, 8), VoxelType::Leaf);

        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(8),
            &mut FaceTileCache::new(),
        );
        // Only y=5 leaf visible (6 faces).
        assert_eq!(mesh.leaf.vertex_count(), 24); // 6 * 4
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

        let mesh0 = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        let mesh1 = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 1, 0),
            None,
            &mut FaceTileCache::new(),
        );

        // Chunk 0 (y=0..15): 6 voxels visible (y=10..15). Internal ±Y faces
        // between adjacent same-type voxels are culled. The +Y face at y=15
        // is also culled because the cross-chunk neighbor at y=16 is trunk.
        // - 4 side faces × 6 = 24 side faces
        // - 1 bottom face (y=10, -Y toward Air)
        // Total: 24 + 1 = 25 faces
        assert_eq!(mesh0.bark.vertex_count(), 25 * 4);

        // Chunk 1 (y=16..31): 10 voxels visible (y=16..25).
        // - 4 side faces * 10 = 40 side faces
        // - bottom (y=16 -Y toward y=15 trunk → culled)
        // - top (y=25 +Y toward Air → 1 face)
        // Total: 40 + 1 = 41 faces
        assert_eq!(mesh1.bark.vertex_count(), 41 * 4);
    }

    #[test]
    fn empty_chunk_in_tall_world() {
        // A tall world where a chunk far above the content is empty.
        let mut world = VoxelWorld::new(16, 128, 16);
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::Trunk);

        // Chunk at y=64..79 should be empty.
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 4, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn y_cutoff_below_chunk_produces_empty_mesh() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);

        // Cutoff at y=0 means every voxel in chunk 0 (y=0..15) is at or above
        // the cutoff. The early-return optimization should produce an empty mesh.
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(0),
            &mut FaceTileCache::new(),
        );
        assert!(mesh.is_empty());
    }

    #[test]
    fn strut_voxel_produces_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Strut);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert_eq!(mesh.bark.vertex_count(), 24); // 6 faces * 4 verts
    }

    #[test]
    fn world_smaller_than_chunk_skips_out_of_bounds_columns() {
        // World is only 8×16×8 but chunk footprint is 16×16. The 8 columns
        // outside the world in each dimension should be skipped.
        let mut world = VoxelWorld::new(8, 16, 8);
        world.set(VoxelCoord::new(4, 4, 4), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert_eq!(mesh.bark.vertex_count(), 24); // 6 faces * 4 verts
    }

    #[test]
    fn y_cutoff_within_contiguous_span() {
        // A trunk column from y=0..9 with cutoff=5 should mesh only y=0..4
        // and expose the +Y cap face at y=4.
        let mut world = one_chunk_world();
        for y in 0..10 {
            world.set(VoxelCoord::new(8, y, 8), VoxelType::Trunk);
        }
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(0, 0, 0),
            Some(5),
            &mut FaceTileCache::new(),
        );

        // 5 visible voxels (y=0..4). Internal ±Y faces are culled.
        // - 4 side faces × 5 = 20 side faces
        // - 1 bottom face (y=0, -Y toward OOB = Air)
        // - 1 top face (y=4, +Y toward y=5 which is above cutoff = treated as Air)
        // Total: 20 + 2 = 22 faces
        assert_eq!(mesh.bark.vertex_count(), 22 * 4);
    }

    #[test]
    fn negative_chunk_coords_produce_empty_mesh() {
        let world = one_chunk_world();
        let mesh = generate_chunk_mesh(
            &world,
            ChunkCoord::new(-1, 0, 0),
            None,
            &mut FaceTileCache::new(),
        );
        assert!(mesh.is_empty());
    }
}
