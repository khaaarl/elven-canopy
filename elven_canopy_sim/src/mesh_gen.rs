// Chunk-based voxel mesh generation with per-face culling.
//
// Pure Rust, no Godot dependencies. Converts a region of the voxel world into
// triangle mesh data suitable for rendering. The world is divided into 16x16x16
// chunks; each chunk produces a `ChunkMesh` with two surfaces (opaque and leaf)
// that the gdext bridge converts into Godot `ArrayMesh` objects.
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
// coordinates. Opaque voxels use atlas-mapped UVs: the V axis selects a row
// in a texture atlas (row 0 = bark for trunk/branch/root/construction,
// row 1 = dirt/grass). Leaf UVs span the full [0,1] range for the
// alpha-scissor texture.
//
// See also: `world.rs` for the voxel grid, `types.rs` for `VoxelType::is_opaque()`,
// `mesh_cache.rs` (in the gdext crate) for the caching layer that sits on top
// of this module, `sim_bridge.rs` for the Godot-facing API that builds
// `ArrayMesh` objects from `ChunkMesh` data.
//
// **Determinism note:** This module is pure and deterministic (same world state
// produces identical mesh data), but mesh generation is a rendering concern and
// does not participate in the sim's lockstep determinism contract.

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
    /// UV coordinates (2 floats per vertex: u, v). Opaque voxels use atlas-mapped
    /// UVs (V selects bark vs dirt/grass row); leaves use full [0,1] range.
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
}

/// Mesh data for one chunk, split into opaque and leaf surfaces.
#[derive(Clone, Debug, Default)]
pub struct ChunkMesh {
    /// Surface 0: opaque voxels (Trunk, Branch, Root, Dirt, construction types).
    pub opaque: SurfaceMesh,
    /// Surface 1: leaf voxels (alpha-scissor transparency).
    pub leaf: SurfaceMesh,
}

impl ChunkMesh {
    pub fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.leaf.is_empty()
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

/// UV coordinates for each face vertex (same for all faces).
/// V-axis is remapped per voxel type to select the correct atlas region.
const FACE_UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

/// Number of atlas rows in the opaque texture atlas. Each voxel type maps to
/// a row, and the V-axis is scaled to [row/ROWS, (row+1)/ROWS].
const ATLAS_ROWS: f32 = 2.0;

/// Return the atlas row index for an opaque voxel type. Row 0 = bark (top),
/// row 1 = dirt/grass (bottom). Leaf voxels use their own material and don't
/// go through this path.
fn atlas_row(vt: VoxelType) -> f32 {
    match vt {
        VoxelType::Dirt => 1.0,
        _ => 0.0, // Trunk, Branch, Root, construction types → bark
    }
}

/// Generate the mesh data for a single chunk of the world.
///
/// Iterates over all voxels in the chunk, checks each of the 6 faces against
/// the neighbor voxel, and emits face geometry when the face should be visible.
/// Neighbor lookups cross chunk boundaries correctly by reading from the world.
pub fn generate_chunk_mesh(world: &VoxelWorld, chunk: ChunkCoord) -> ChunkMesh {
    let mut mesh = ChunkMesh::default();

    let base_x = chunk.cx * CHUNK_SIZE;
    let base_y = chunk.cy * CHUNK_SIZE;
    let base_z = chunk.cz * CHUNK_SIZE;

    for ly in 0..CHUNK_SIZE {
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let wx = base_x + lx;
                let wy = base_y + ly;
                let wz = base_z + lz;
                let coord = VoxelCoord::new(wx, wy, wz);
                let vt = world.get(coord);

                if !produces_geometry(vt) {
                    continue;
                }

                let color = voxel_color(vt);
                let is_leaf = vt == VoxelType::Leaf;
                let surface = if is_leaf {
                    &mut mesh.leaf
                } else {
                    &mut mesh.opaque
                };

                // Atlas UV mapping: remap the V coordinate to the voxel
                // type's row within the texture atlas.
                let row = if is_leaf { 0.0 } else { atlas_row(vt) };
                let v_base = row / ATLAS_ROWS;
                let v_scale = 1.0 / ATLAS_ROWS;

                for (face_idx, &(dx, dy, dz)) in FACES.iter().enumerate() {
                    let neighbor = world.get(VoxelCoord::new(wx + dx, wy + dy, wz + dz));

                    // Cull face if neighbor is opaque.
                    if neighbor.is_opaque() {
                        continue;
                    }

                    let base_vertex = surface.vertex_count() as u32;
                    let normal = [dx as f32, dy as f32, dz as f32];

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

                        surface.uvs.push(FACE_UVS[vi][0]);
                        surface.uvs.push(v_base + FACE_UVS[vi][1] * v_scale);
                    }

                    // 2 triangles: 0-1-2, 0-2-3
                    surface.indices.push(base_vertex);
                    surface.indices.push(base_vertex + 1);
                    surface.indices.push(base_vertex + 2);
                    surface.indices.push(base_vertex);
                    surface.indices.push(base_vertex + 2);
                    surface.indices.push(base_vertex + 3);
                }
            }
        }
    }

    mesh
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
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));
        assert!(mesh.is_empty());
    }

    #[test]
    fn single_trunk_voxel_produces_6_faces() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));

        // 6 faces * 4 vertices = 24 vertices
        assert_eq!(mesh.opaque.vertex_count(), 24);
        // 6 faces * 6 indices = 36 indices
        assert_eq!(mesh.opaque.indices.len(), 36);
        // No leaf geometry.
        assert!(mesh.leaf.is_empty());
    }

    #[test]
    fn two_adjacent_opaque_voxels_cull_shared_faces() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Branch);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));

        // Each voxel alone = 6 faces. Together they share 1 face, so each loses
        // 1 face: 2 * 5 = 10 faces. 10 * 4 = 40 vertices.
        assert_eq!(mesh.opaque.vertex_count(), 40);
        assert_eq!(mesh.opaque.indices.len(), 60); // 10 * 6
    }

    #[test]
    fn forest_floor_produces_no_geometry_but_culls_adjacent() {
        let mut world = one_chunk_world();
        // Place ForestFloor and a trunk voxel above it.
        world.set(VoxelCoord::new(8, 0, 8), VoxelType::ForestFloor);
        world.set(VoxelCoord::new(8, 1, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));

        // ForestFloor produces no geometry, but the trunk's -Y face should be
        // culled because ForestFloor is opaque. Trunk = 5 visible faces.
        assert_eq!(mesh.opaque.vertex_count(), 20); // 5 * 4
        assert_eq!(mesh.opaque.indices.len(), 30); // 5 * 6
    }

    #[test]
    fn leaf_to_leaf_faces_preserved() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Leaf);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));

        // Leaf is not opaque, so leaf-to-leaf faces are NOT culled.
        // Each leaf has 6 faces, both get all 6 = 12 faces total.
        assert_eq!(mesh.leaf.vertex_count(), 48); // 12 * 4
        assert_eq!(mesh.leaf.indices.len(), 72); // 12 * 6
        assert!(mesh.opaque.is_empty());
    }

    #[test]
    fn leaf_to_opaque_face_culled() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Leaf);
        world.set(VoxelCoord::new(9, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));

        // Leaf: 5 faces (the +X face toward trunk is culled — trunk is opaque).
        assert_eq!(mesh.leaf.vertex_count(), 20); // 5 * 4
        // Trunk: 6 faces (the -X face toward leaf is NOT culled — leaf isn't opaque).
        assert_eq!(mesh.opaque.vertex_count(), 24); // 6 * 4
    }

    #[test]
    fn chunk_boundary_neighbor_check() {
        // World is 32 voxels wide (2 chunks). Place voxels at chunk boundary.
        let mut world = VoxelWorld::new(32, 16, 16);
        world.set(VoxelCoord::new(15, 8, 8), VoxelType::Trunk); // last voxel in chunk 0
        world.set(VoxelCoord::new(16, 8, 8), VoxelType::Trunk); // first voxel in chunk 1

        let mesh0 = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));
        let mesh1 = generate_chunk_mesh(&world, ChunkCoord::new(1, 0, 0));

        // Each should have 5 faces (shared face culled across chunk boundary).
        assert_eq!(mesh0.opaque.vertex_count(), 20);
        assert_eq!(mesh1.opaque.vertex_count(), 20);
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
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));
        assert_eq!(mesh.opaque.vertex_count(), 24); // 6 faces * 4 verts
    }

    #[test]
    fn vertex_colors_match_voxel_type() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Trunk);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));

        // Check first vertex color is trunk color.
        let expected = voxel_color(VoxelType::Trunk);
        assert_eq!(mesh.opaque.colors[0], expected[0]);
        assert_eq!(mesh.opaque.colors[1], expected[1]);
        assert_eq!(mesh.opaque.colors[2], expected[2]);
        assert_eq!(mesh.opaque.colors[3], expected[3]);
    }

    #[test]
    fn fruit_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::Fruit);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));
        assert!(mesh.is_empty());
    }

    #[test]
    fn building_interior_does_not_produce_geometry() {
        let mut world = one_chunk_world();
        world.set(VoxelCoord::new(8, 8, 8), VoxelType::BuildingInterior);
        let mesh = generate_chunk_mesh(&world, ChunkCoord::new(0, 0, 0));
        assert!(mesh.is_empty());
    }
}
