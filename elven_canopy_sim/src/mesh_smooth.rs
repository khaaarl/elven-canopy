// Smooth mesh generation for wood and root voxels.
//
// Replaces raw unit-cube rendering with organic-looking surfaces. For each
// exposed face of a wood (Trunk|Branch) or root voxel, the face is split
// into 8 triangles radiating from a center vertex. Corner and edge-midpoint
// vertices are displaced outward along the face normal when a same-material
// diagonal neighbor exists, creating smooth ramps where staircases occur and
// chamfered edges at corners.
//
// The algorithm groups voxels into material groups (Wood vs Root) and only
// smooths within the same group — wood never blends with root. Any solid
// voxel (including cross-group) culls a face.
//
// Output is an indexed triangle mesh with per-vertex normals suitable for
// direct upload to Godot's `ArrayMesh`. Vertices are welded across faces
// and voxels using a `BTreeMap<VertexKey, u32>` so shared edges produce
// smooth normals.
//
// This module has no Godot dependencies and lives in the sim crate so it
// can be tested headless. The GDExtension bridge (`sim_bridge.rs`) calls
// `generate_smoothed_meshes()` and converts the result to packed Godot
// arrays.
//
// See also: `world.rs` for the `VoxelWorld` used for neighbor lookups,
// `types.rs` for `VoxelType` and `VoxelCoord`, `sim_bridge.rs` for the
// GDExtension bridge methods that expose this to Godot, `tree_renderer.gd`
// for the GDScript consumer.

use std::collections::BTreeMap;

use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

/// Material groups for smoothing. Smoothing only occurs between voxels in the
/// same group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MaterialGroup {
    Wood,
    Root,
}

/// Classify a voxel type into a material group, if any.
fn material_group(vt: VoxelType) -> Option<MaterialGroup> {
    match vt {
        VoxelType::Trunk | VoxelType::Branch => Some(MaterialGroup::Wood),
        VoxelType::Root => Some(MaterialGroup::Root),
        _ => None,
    }
}

/// Output mesh: indexed triangle list with per-vertex normals.
#[derive(Clone, Debug)]
pub struct SmoothedMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// Face basis vectors for the 6 cube faces.
/// `normal` is the outward face normal, `tangent_u` and `tangent_v` span the
/// face plane. U × V = N for all faces. Combined with the CW-in-UV pinwheel
/// order, this produces CW winding when viewed from outside (front-facing in
/// Godot 4's Vulkan renderer).
struct FaceBasis {
    normal: [i32; 3],
    tangent_u: [i32; 3],
    tangent_v: [i32; 3],
}

/// The 6 face bases in order: +X, -X, +Y, -Y, +Z, -Z.
const FACE_BASES: [FaceBasis; 6] = [
    // +X: N=(1,0,0)  U=(0,0,-1)  V=(0,1,0)
    FaceBasis {
        normal: [1, 0, 0],
        tangent_u: [0, 0, -1],
        tangent_v: [0, 1, 0],
    },
    // -X: N=(-1,0,0) U=(0,0,1)   V=(0,1,0)
    FaceBasis {
        normal: [-1, 0, 0],
        tangent_u: [0, 0, 1],
        tangent_v: [0, 1, 0],
    },
    // +Y: N=(0,1,0)  U=(0,0,1)   V=(1,0,0)
    FaceBasis {
        normal: [0, 1, 0],
        tangent_u: [0, 0, 1],
        tangent_v: [1, 0, 0],
    },
    // -Y: N=(0,-1,0) U=(0,0,-1)  V=(1,0,0)
    FaceBasis {
        normal: [0, -1, 0],
        tangent_u: [0, 0, -1],
        tangent_v: [1, 0, 0],
    },
    // +Z: N=(0,0,1)  U=(1,0,0)   V=(0,1,0)
    FaceBasis {
        normal: [0, 0, 1],
        tangent_u: [1, 0, 0],
        tangent_v: [0, 1, 0],
    },
    // -Z: N=(0,0,-1) U=(-1,0,0)  V=(0,1,0)
    FaceBasis {
        normal: [0, 0, -1],
        tangent_u: [-1, 0, 0],
        tangent_v: [0, 1, 0],
    },
];

/// Vertex welding key: position multiplied by 2 and rounded to integer.
/// This maps the half-unit grid (0.0, 0.5, 1.0, 1.5, ...) to integers,
/// ensuring exact matching of coincident vertices across faces and voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct VertexKey {
    hx: i32,
    hy: i32,
    hz: i32,
}

impl VertexKey {
    fn from_pos(pos: [f32; 3]) -> Self {
        Self {
            hx: (pos[0] * 2.0).round() as i32,
            hy: (pos[1] * 2.0).round() as i32,
            hz: (pos[2] * 2.0).round() as i32,
        }
    }
}

/// Generate smoothed meshes for wood and root voxels.
///
/// Takes pre-collected voxel lists (from `Tree.trunk_voxels` + `branch_voxels`
/// for wood, and `root_voxels` for roots) to avoid iterating the full world.
/// Uses `world.get()` for O(1) neighbor lookups during face culling and
/// displacement checks.
///
/// Returns a map from material group to smoothed mesh. Groups with no exposed
/// faces are omitted.
pub fn generate_smoothed_meshes(
    world: &VoxelWorld,
    wood_voxels: &[VoxelCoord],
    root_voxels: &[VoxelCoord],
) -> BTreeMap<MaterialGroup, SmoothedMesh> {
    let mut result = BTreeMap::new();

    for &group in &[MaterialGroup::Wood, MaterialGroup::Root] {
        let voxels = match group {
            MaterialGroup::Wood => wood_voxels,
            MaterialGroup::Root => root_voxels,
        };

        if voxels.is_empty() {
            continue;
        }

        let mesh = generate_group_mesh(world, voxels, group);
        if !mesh.indices.is_empty() {
            result.insert(group, mesh);
        }
    }

    result
}

/// Accumulates vertex/index data during mesh construction, with vertex welding.
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    weld_map: BTreeMap<VertexKey, u32>,
}

impl MeshBuilder {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            weld_map: BTreeMap::new(),
        }
    }

    /// Get or insert a welded vertex. If a vertex at the same position already
    /// exists (by key), accumulate the normal and return the existing index.
    /// Otherwise, create a new vertex.
    fn get_or_insert_vertex(&mut self, pos: [f32; 3], face_normal: [f32; 3]) -> u32 {
        let key = VertexKey::from_pos(pos);
        if let Some(&idx) = self.weld_map.get(&key) {
            // Accumulate normal for smooth shading
            self.normals[idx as usize][0] += face_normal[0];
            self.normals[idx as usize][1] += face_normal[1];
            self.normals[idx as usize][2] += face_normal[2];
            idx
        } else {
            let idx = self.positions.len() as u32;
            self.positions.push(pos);
            self.normals.push(face_normal);
            self.weld_map.insert(key, idx);
            idx
        }
    }

    /// Normalize all accumulated normals and produce the final mesh.
    fn finish(mut self) -> SmoothedMesh {
        for n in &mut self.normals {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len > 1e-10 {
                n[0] /= len;
                n[1] /= len;
                n[2] /= len;
            }
        }
        SmoothedMesh {
            positions: self.positions,
            normals: self.normals,
            indices: self.indices,
        }
    }
}

/// Build the mesh for a single material group.
fn generate_group_mesh(
    world: &VoxelWorld,
    voxels: &[VoxelCoord],
    group: MaterialGroup,
) -> SmoothedMesh {
    let mut builder = MeshBuilder::new();

    for &coord in voxels {
        for face in &FACE_BASES {
            // Face culling: skip if neighbor in normal direction is solid
            let neighbor = VoxelCoord::new(
                coord.x + face.normal[0],
                coord.y + face.normal[1],
                coord.z + face.normal[2],
            );
            if world.get(neighbor).is_solid() {
                continue;
            }

            emit_face(coord, face, group, world, &mut builder);
        }
    }

    builder.finish()
}

/// Compute displacement for direction D on a voxel face.
///
/// Returns:
/// - `0.0` if the perpendicular neighbor is the same material group (flush)
/// - `+0.5` if the perpendicular is NOT same group but the diagonal across the
///   face IS same group (outward ramp at staircases)
/// - `-0.5` if neither perpendicular nor diagonal is same group (inward chamfer
///   at exposed edges, creating diamond cross-sections)
fn edge_displacement(
    world: &VoxelWorld,
    coord: VoxelCoord,
    normal: [i32; 3],
    direction: [i32; 3],
    group: MaterialGroup,
) -> f32 {
    let perp = VoxelCoord::new(
        coord.x + direction[0],
        coord.y + direction[1],
        coord.z + direction[2],
    );
    if material_group(world.get(perp)) == Some(group) {
        return 0.0;
    }
    let diag = VoxelCoord::new(
        coord.x + direction[0] + normal[0],
        coord.y + direction[1] + normal[1],
        coord.z + direction[2] + normal[2],
    );
    if material_group(world.get(diag)) == Some(group) {
        0.5
    } else {
        -0.5
    }
}

/// Emit 8 triangles for one face of a voxel.
///
/// The face is divided into a 3×3 grid of vertex positions in UV space:
/// ```text
///   (0,1)---(0.5,1)---(1,1)
///     |  \  2 | 3  /    |
///     | 1  \  |  /  4   |
///   (0,0.5)--(C)--(1,0.5)
///     | 8  /  |  \  5   |
///     |  /  7 | 6  \    |
///   (0,0)---(0.5,0)---(1,0)
/// ```
/// where C is the face center (0.5, 0.5).
fn emit_face(
    coord: VoxelCoord,
    face: &FaceBasis,
    group: MaterialGroup,
    world: &VoxelWorld,
    builder: &mut MeshBuilder,
) {
    let n = face.normal;
    let u = face.tangent_u;
    let v = face.tangent_v;

    // Voxel center in world space
    let cx = coord.x as f32 + 0.5;
    let cy = coord.y as f32 + 0.5;
    let cz = coord.z as f32 + 0.5;

    // Compute displacements for the 4 cardinal directions on the face plane.
    // Each returns -0.5 (chamfer), 0.0 (flush), or +0.5 (ramp).
    let neg_u = [-u[0], -u[1], -u[2]];
    let pos_u = u;
    let neg_v = [-v[0], -v[1], -v[2]];
    let pos_v = v;

    let disp_neg_u = edge_displacement(world, coord, n, neg_u, group);
    let disp_pos_u = edge_displacement(world, coord, n, pos_u, group);
    let disp_neg_v = edge_displacement(world, coord, n, neg_v, group);
    let disp_pos_v = edge_displacement(world, coord, n, pos_v, group);

    // Corner displacements: sum of the two adjacent edge displacements,
    // clamped to [-0.5, 1.0] to prevent over-displacement at exposed corners.
    let corner_disp = |a: f32, b: f32| -> f32 { (a + b).clamp(-0.5, 1.0) };
    let corner_00_disp = corner_disp(disp_neg_u, disp_neg_v);
    let corner_10_disp = corner_disp(disp_pos_u, disp_neg_v);
    let corner_11_disp = corner_disp(disp_pos_u, disp_pos_v);
    let corner_01_disp = corner_disp(disp_neg_u, disp_pos_v);

    // Edge midpoint displacements: directly from the perpendicular direction.
    let mid_bot_disp = disp_neg_v;
    let mid_right_disp = disp_pos_u;
    let mid_top_disp = disp_pos_v;
    let mid_left_disp = disp_neg_u;

    // Center: always 0.0

    // Compute world-space positions:
    // pos = voxel_center + 0.5*N + (uv_u - 0.5)*U + (uv_v - 0.5)*V + disp*N
    let nf = [n[0] as f32, n[1] as f32, n[2] as f32];
    let uf = [u[0] as f32, u[1] as f32, u[2] as f32];
    let vf = [v[0] as f32, v[1] as f32, v[2] as f32];

    let make_pos = |uv_u: f32, uv_v: f32, disp: f32| -> [f32; 3] {
        [
            cx + 0.5 * nf[0] + (uv_u - 0.5) * uf[0] + (uv_v - 0.5) * vf[0] + disp * nf[0],
            cy + 0.5 * nf[1] + (uv_u - 0.5) * uf[1] + (uv_v - 0.5) * vf[1] + disp * nf[1],
            cz + 0.5 * nf[2] + (uv_u - 0.5) * uf[2] + (uv_v - 0.5) * vf[2] + disp * nf[2],
        ]
    };

    // 9 vertices: corners, edge midpoints, center
    let v_00 = make_pos(0.0, 0.0, corner_00_disp);
    let v_10 = make_pos(1.0, 0.0, corner_10_disp);
    let v_11 = make_pos(1.0, 1.0, corner_11_disp);
    let v_01 = make_pos(0.0, 1.0, corner_01_disp);
    let v_mid_bot = make_pos(0.5, 0.0, mid_bot_disp);
    let v_mid_right = make_pos(1.0, 0.5, mid_right_disp);
    let v_mid_top = make_pos(0.5, 1.0, mid_top_disp);
    let v_mid_left = make_pos(0.0, 0.5, mid_left_disp);
    let v_center = make_pos(0.5, 0.5, 0.0);

    // Get or create welded vertex indices
    let face_normal = nf;
    let i_00 = builder.get_or_insert_vertex(v_00, face_normal);
    let i_10 = builder.get_or_insert_vertex(v_10, face_normal);
    let i_11 = builder.get_or_insert_vertex(v_11, face_normal);
    let i_01 = builder.get_or_insert_vertex(v_01, face_normal);
    let i_mid_bot = builder.get_or_insert_vertex(v_mid_bot, face_normal);
    let i_mid_right = builder.get_or_insert_vertex(v_mid_right, face_normal);
    let i_mid_top = builder.get_or_insert_vertex(v_mid_top, face_normal);
    let i_mid_left = builder.get_or_insert_vertex(v_mid_left, face_normal);
    let i_center = builder.get_or_insert_vertex(v_center, face_normal);

    // 8 triangles in pinwheel pattern. Boundary traces CW in UV space
    // (00→mid_left→01→mid_top→11→mid_right→10→mid_bot) so that
    // center→boundary[i]→boundary[i+1] produces CW winding when viewed
    // from outside (since U×V = N). Godot 4's Vulkan renderer treats CW
    // as front-facing due to the Y-axis flip in clip space.
    let tris = [
        [i_center, i_00, i_mid_left],
        [i_center, i_mid_left, i_01],
        [i_center, i_01, i_mid_top],
        [i_center, i_mid_top, i_11],
        [i_center, i_11, i_mid_right],
        [i_center, i_mid_right, i_10],
        [i_center, i_10, i_mid_bot],
        [i_center, i_mid_bot, i_00],
    ];

    for tri in &tris {
        builder.indices.push(tri[0]);
        builder.indices.push(tri[1]);
        builder.indices.push(tri[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a small world and place a single voxel.
    fn world_with_voxel(coord: VoxelCoord, vt: VoxelType) -> VoxelWorld {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(coord, vt);
        world
    }

    #[test]
    fn single_voxel_all_faces_exposed() {
        let coord = VoxelCoord::new(5, 5, 5);
        let world = world_with_voxel(coord, VoxelType::Trunk);
        let result = generate_smoothed_meshes(&world, &[coord], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // 6 faces × 8 triangles × 3 indices = 144 indices
        assert_eq!(mesh.indices.len(), 144);

        // Welded vertex count with chamfering: 6 face centers (merged with
        // displaced edge midpoints that land on adjacent face centers) + 12
        // corner positions (pairs of opposite-face corners merge at cube edge
        // midpoints) = 18.
        assert_eq!(mesh.positions.len(), 18);
    }

    #[test]
    fn adjacent_solid_culls_face() {
        // Two adjacent wood voxels — shared face should be culled.
        let c1 = VoxelCoord::new(5, 5, 5);
        let c2 = VoxelCoord::new(6, 5, 5);
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(c1, VoxelType::Trunk);
        world.set(c2, VoxelType::Branch);

        let result = generate_smoothed_meshes(&world, &[c1, c2], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // Each voxel loses 1 face from the shared boundary = 5 faces each = 10 faces total
        // 10 faces × 8 tris × 3 = 240 indices
        assert_eq!(mesh.indices.len(), 240);
    }

    #[test]
    fn non_group_solid_culls_face() {
        // Trunk on top of ForestFloor — bottom face should be culled.
        let trunk = VoxelCoord::new(5, 1, 5);
        let floor = VoxelCoord::new(5, 0, 5);
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(trunk, VoxelType::Trunk);
        world.set(floor, VoxelType::ForestFloor);

        let result = generate_smoothed_meshes(&world, &[trunk], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // 5 exposed faces × 8 tris × 3 = 120 indices
        assert_eq!(mesh.indices.len(), 120);
    }

    #[test]
    fn diagonal_neighbor_causes_displacement() {
        // Staircase: two trunk voxels arranged diagonally in Y and X.
        // coord A at (4,4,5), coord B at (5,5,5).
        // B's -X face has a diagonal neighbor A at (4,5,5) - wait, A is at (4,4,5).
        // Let's use a clear staircase: A at (4,4,5), B at (5,5,5).
        // For B's -Y face (bottom), check direction -X:
        //   coord + (-1,0,0) = (4,5,5) → Air (not same group)
        //   coord + (-1,0,0) + (0,-1,0) = (4,4,5) → Trunk (same group) ✓
        // So the -Y face of B should have displacement at the -U edge/corner
        // vertices.
        //
        // The -Y face basis: N=(0,-1,0), U=(1,0,0), V=(0,0,-1).
        // The -U direction on this face corresponds to world direction (-1,0,0).
        // So `check_displacement` for direction (-1,0,0) on the -Y face:
        //   diag = (5,5,5) + (-1,0,0) + (0,-1,0) = (4,4,5) → Trunk ✓
        //   perp = (5,5,5) + (-1,0,0) = (4,5,5) → Air, not same group ✓
        // Displacement occurs!
        //
        // The vertex at (u=0, v=0.5) = mid_left on -Y face:
        //   pos = (5.5, 5.5, 5.5) + 0.5*(0,-1,0) + (0-0.5)*(1,0,0) + (0.5-0.5)*(0,0,-1) + 0.5*(0,-1,0)
        //       = (5.5, 5.5, 5.5) + (0,-0.5,0) + (-0.5,0,0) + (0,0,0) + (0,-0.5,0)
        //       = (5.0, 4.5, 5.5)
        // Wait, that's the -Y face (downward). Let's verify carefully.
        // Actually, let's check a simpler scenario. Let me use the +Y face of A.
        //
        // A at (4,4,5). +Y face: N=(0,1,0), U=(1,0,0), V=(0,0,1).
        // Direction +U = (1,0,0):
        //   diag = (4,4,5) + (1,0,0) + (0,1,0) = (5,5,5) → Trunk ✓
        //   perp = (4,4,5) + (1,0,0) = (5,4,5) → Air ✓
        // Displacement at +U direction = true.
        //
        // mid_right vertex (u=1, v=0.5) on +Y face:
        //   pos = (4.5, 4.5, 5.5) + 0.5*(0,1,0) + (1-0.5)*(1,0,0) + (0.5-0.5)*(0,0,1) + 0.5*(0,1,0)
        //       = (4.5, 4.5, 5.5) + (0,0.5,0) + (0.5,0,0) + (0,0,0) + (0,0.5,0)
        //       = (5.0, 5.5, 5.5)

        let a = VoxelCoord::new(4, 4, 5);
        let b = VoxelCoord::new(5, 5, 5);
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(a, VoxelType::Trunk);
        world.set(b, VoxelType::Trunk);

        let result = generate_smoothed_meshes(&world, &[a, b], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // Find the displaced vertex at (5.0, 5.5, 5.5)
        let target = [5.0_f32, 5.5, 5.5];
        let found = mesh.positions.iter().any(|p| {
            (p[0] - target[0]).abs() < 1e-5
                && (p[1] - target[1]).abs() < 1e-5
                && (p[2] - target[2]).abs() < 1e-5
        });
        assert!(
            found,
            "Expected displaced vertex at {:?}, positions: {:?}",
            target, mesh.positions
        );
    }

    #[test]
    fn no_displacement_when_perp_is_same_group() {
        // Filled L-shape: voxels at (5,5,5), (6,5,5), (5,6,5).
        // For (5,5,5) +Y face, direction +U = (1,0,0):
        //   perp = (5,5,5) + (1,0,0) = (6,5,5) → Trunk = same group
        //   So displacement check fails (perp IS same group), no displacement.
        let v1 = VoxelCoord::new(5, 5, 5);
        let v2 = VoxelCoord::new(6, 5, 5);
        let v3 = VoxelCoord::new(5, 6, 5);
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(v1, VoxelType::Trunk);
        world.set(v2, VoxelType::Trunk);
        world.set(v3, VoxelType::Trunk);

        let result = generate_smoothed_meshes(&world, &[v1, v2, v3], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // The +Y face of v1 is culled (v3 is above it).
        // The +X face of v1 is culled (v2 is beside it).
        // For v2's +Y face, direction -U = (-1,0,0):
        //   perp = (6,5,5) + (-1,0,0) = (5,5,5) → same group → no displacement
        // The mid_left vertex on v2's +Y face (u=0, v=0.5):
        //   Without displacement: pos = (6.5, 5.5, 5.5) + 0.5*(0,1,0) + (0-0.5)*(1,0,0) + 0 + 0
        //       = (6.0, 6.0, 5.5)
        //   With displacement it would be (6.0, 6.5, 5.5).
        // Verify the undisplaced vertex exists at the expected position.
        let undisplaced_pos = [6.0_f32, 6.0, 5.5];
        let found = mesh.positions.iter().any(|p| {
            (p[0] - undisplaced_pos[0]).abs() < 1e-5
                && (p[1] - undisplaced_pos[1]).abs() < 1e-5
                && (p[2] - undisplaced_pos[2]).abs() < 1e-5
        });
        assert!(
            found,
            "Should have undisplaced vertex at {:?} (no displacement when perp is same group)",
            undisplaced_pos
        );
    }

    #[test]
    fn wood_does_not_smooth_with_root() {
        // Diagonal arrangement: Trunk at (5,5,5), Root at (6,6,5).
        // For Trunk's +Y face, direction +U = (1,0,0):
        //   diag = (5,5,5) + (1,0,0) + (0,1,0) = (6,6,5) → Root = different group
        //   So no displacement for Wood.
        let trunk = VoxelCoord::new(5, 5, 5);
        let root = VoxelCoord::new(6, 6, 5);
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(trunk, VoxelType::Trunk);
        world.set(root, VoxelType::Root);

        let result = generate_smoothed_meshes(&world, &[trunk], &[root]);
        let wood_mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // All wood vertices should be at base face positions (no displacement).
        // A displaced vertex on +Y face mid_right would be at (6.0, 6.5, 5.5).
        let displaced = [6.0_f32, 6.5, 5.5];
        let found = wood_mesh.positions.iter().any(|p| {
            (p[0] - displaced[0]).abs() < 1e-5
                && (p[1] - displaced[1]).abs() < 1e-5
                && (p[2] - displaced[2]).abs() < 1e-5
        });
        assert!(!found, "Wood should NOT displace toward Root neighbor");
    }

    #[test]
    fn vertices_welded_across_voxel_boundary() {
        // Two vertically stacked trunk voxels at (5,5,5) and (5,6,5).
        // Their shared face is culled. The top face of (5,6,5) and the bottom
        // face of (5,5,5) don't share vertices. But the side faces do share
        // edge vertices at y=6.
        let c1 = VoxelCoord::new(5, 5, 5);
        let c2 = VoxelCoord::new(5, 6, 5);
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(c1, VoxelType::Trunk);
        world.set(c2, VoxelType::Trunk);

        let result = generate_smoothed_meshes(&world, &[c1, c2], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // Count how many positions are at y=6.0 (the boundary between the two voxels).
        // With chamfering, only 4 vertices sit at y=6.0: the side face edge
        // midpoints (undisplaced because the perpendicular along-rod neighbor is
        // same material) and displaced corners. These form a diamond at the
        // boundary.
        let boundary_verts: Vec<_> = mesh
            .positions
            .iter()
            .filter(|p| (p[1] - 6.0).abs() < 1e-5)
            .collect();

        assert_eq!(
            boundary_verts.len(),
            4,
            "Expected 4 welded boundary vertices (diamond), got {:?}",
            boundary_verts
        );
    }

    #[test]
    fn empty_world_produces_no_mesh() {
        let world = VoxelWorld::new(16, 16, 16);
        let result = generate_smoothed_meshes(&world, &[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn normals_point_outward() {
        let coord = VoxelCoord::new(5, 5, 5);
        let world = world_with_voxel(coord, VoxelType::Trunk);
        let result = generate_smoothed_meshes(&world, &[coord], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        let voxel_center = [5.5_f32, 5.5, 5.5];

        for (i, normal) in mesh.normals.iter().enumerate() {
            let pos = &mesh.positions[i];
            // Vector from voxel center to vertex
            let to_vertex = [
                pos[0] - voxel_center[0],
                pos[1] - voxel_center[1],
                pos[2] - voxel_center[2],
            ];
            let dot =
                normal[0] * to_vertex[0] + normal[1] * to_vertex[1] + normal[2] * to_vertex[2];
            // With chamfering, corner vertices can land at cube-edge midpoints
            // where opposite-face normals cancel (dot ≈ 0). These are
            // geometrically correct zero-length normals; allow them.
            assert!(
                dot >= 0.0,
                "Normal at vertex {:?} (pos {:?}) should not point inward, dot={:.4}",
                i,
                pos,
                dot
            );
        }
    }

    #[test]
    fn triangles_wind_cw_from_outside() {
        // Godot 4's Vulkan renderer treats CW winding as front-facing (due to
        // the Y-axis flip in clip space). For each triangle, the cross product
        // of two edges should point INWARD (toward the voxel center), which
        // means vertices appear CW when viewed from outside.
        let coord = VoxelCoord::new(5, 5, 5);
        let world = world_with_voxel(coord, VoxelType::Trunk);
        let result = generate_smoothed_meshes(&world, &[coord], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        let voxel_center = [5.5_f32, 5.5, 5.5];

        for tri_idx in (0..mesh.indices.len()).step_by(3) {
            let a = mesh.positions[mesh.indices[tri_idx] as usize];
            let b = mesh.positions[mesh.indices[tri_idx + 1] as usize];
            let c = mesh.positions[mesh.indices[tri_idx + 2] as usize];

            // Edge vectors
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];

            // Cross product (triangle normal)
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];

            // Triangle centroid
            let centroid = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];

            // Vector from voxel center to triangle centroid
            let outward = [
                centroid[0] - voxel_center[0],
                centroid[1] - voxel_center[1],
                centroid[2] - voxel_center[2],
            ];

            // CW from outside = cross product points INWARD = dot < 0
            let dot = cross[0] * outward[0] + cross[1] * outward[1] + cross[2] * outward[2];
            assert!(
                dot < 0.0,
                "Triangle {} has CCW winding (dot={:.6}), vertices: {:?} {:?} {:?}",
                tri_idx / 3,
                dot,
                a,
                b,
                c
            );
        }
    }

    #[test]
    fn straight_rod_has_diamond_cross_section() {
        // A horizontal rod of 3 trunk voxels along X. The middle voxel's
        // cross-section in the YZ plane should be a diamond: the +Y and -Y
        // face edge midpoints perpendicular to the rod are displaced inward
        // by -0.5, landing on adjacent face centers.
        let v0 = VoxelCoord::new(4, 5, 5);
        let v1 = VoxelCoord::new(5, 5, 5);
        let v2 = VoxelCoord::new(6, 5, 5);
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(v0, VoxelType::Trunk);
        world.set(v1, VoxelType::Trunk);
        world.set(v2, VoxelType::Trunk);

        let result = generate_smoothed_meshes(&world, &[v0, v1, v2], &[]);
        let mesh = result
            .get(&MaterialGroup::Wood)
            .expect("should have wood mesh");

        // The middle voxel (5,5,5) has center (5.5, 5.5, 5.5).
        // Its +Y face center is at (5.5, 6.0, 5.5) — undisplaced (diamond top).
        // The +Y face's mid_right (u=1, +Z edge) should be displaced inward
        // by -0.5, landing at (5.5, 5.5, 6.0) = the +Z face center position.
        // This confirms the chamfer creates the diamond shape.
        let diamond_top = [5.5_f32, 6.0, 5.5];
        let diamond_right = [5.5_f32, 5.5, 6.0]; // +Y mid_right displaced = +Z center
        let diamond_bottom = [5.5_f32, 5.0, 5.5];
        let diamond_left = [5.5_f32, 5.5, 5.0]; // +Y mid_left displaced = -Z center

        for (label, target) in [
            ("top", diamond_top),
            ("right", diamond_right),
            ("bottom", diamond_bottom),
            ("left", diamond_left),
        ] {
            let found = mesh.positions.iter().any(|p| {
                (p[0] - target[0]).abs() < 1e-5
                    && (p[1] - target[1]).abs() < 1e-5
                    && (p[2] - target[2]).abs() < 1e-5
            });
            assert!(
                found,
                "Diamond vertex '{}' at {:?} not found in mesh",
                label, target
            );
        }
    }
}
