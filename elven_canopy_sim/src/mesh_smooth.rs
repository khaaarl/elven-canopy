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
/// face plane. Winding: U × V = N (CCW when viewed from outside).
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
    // +Y: N=(0,1,0)  U=(1,0,0)   V=(0,0,1)
    FaceBasis {
        normal: [0, 1, 0],
        tangent_u: [1, 0, 0],
        tangent_v: [0, 0, 1],
    },
    // -Y: N=(0,-1,0) U=(1,0,0)   V=(0,0,-1)
    FaceBasis {
        normal: [0, -1, 0],
        tangent_u: [1, 0, 0],
        tangent_v: [0, 0, -1],
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

/// Check if direction D produces displacement for a voxel face.
///
/// Displacement occurs when:
/// - The voxel at `coord + D + N` is the same material group (diagonal neighbor
///   across the face exists)
/// - AND the voxel at `coord + D` is NOT the same material group (no same-group
///   voxel blocking the diagonal — the diagonal is "exposed")
fn check_displacement(
    world: &VoxelWorld,
    coord: VoxelCoord,
    normal: [i32; 3],
    direction: [i32; 3],
    group: MaterialGroup,
) -> bool {
    let diag = VoxelCoord::new(
        coord.x + direction[0] + normal[0],
        coord.y + direction[1] + normal[1],
        coord.z + direction[2] + normal[2],
    );
    let perp = VoxelCoord::new(
        coord.x + direction[0],
        coord.y + direction[1],
        coord.z + direction[2],
    );
    material_group(world.get(diag)) == Some(group) && material_group(world.get(perp)) != Some(group)
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

    // Compute displacements for each of the 4 directions on the face plane
    // -U, +U, -V, +V
    let neg_u = [-u[0], -u[1], -u[2]];
    let pos_u = u;
    let neg_v = [-v[0], -v[1], -v[2]];
    let pos_v = v;

    let disp_neg_u = check_displacement(world, coord, n, neg_u, group);
    let disp_pos_u = check_displacement(world, coord, n, pos_u, group);
    let disp_neg_v = check_displacement(world, coord, n, neg_v, group);
    let disp_pos_v = check_displacement(world, coord, n, pos_v, group);

    // Also check the 4 diagonal directions for corner displacement
    let diag_nu_nv = [
        neg_u[0] + neg_v[0],
        neg_u[1] + neg_v[1],
        neg_u[2] + neg_v[2],
    ];
    let diag_pu_nv = [
        pos_u[0] + neg_v[0],
        pos_u[1] + neg_v[1],
        pos_u[2] + neg_v[2],
    ];
    let diag_nu_pv = [
        neg_u[0] + pos_v[0],
        neg_u[1] + pos_v[1],
        neg_u[2] + pos_v[2],
    ];
    let diag_pu_pv = [
        pos_u[0] + pos_v[0],
        pos_u[1] + pos_v[1],
        pos_u[2] + pos_v[2],
    ];

    let disp_diag_nu_nv = check_displacement(world, coord, n, diag_nu_nv, group);
    let disp_diag_pu_nv = check_displacement(world, coord, n, diag_pu_nv, group);
    let disp_diag_nu_pv = check_displacement(world, coord, n, diag_nu_pv, group);
    let disp_diag_pu_pv = check_displacement(world, coord, n, diag_pu_pv, group);

    // 9 vertex positions on the face:
    // Corners: (u,v) = (0,0), (1,0), (1,1), (0,1)
    // Edge midpoints: (0.5,0), (1,0.5), (0.5,1), (0,0.5)
    // Center: (0.5, 0.5)

    // Corner displacements: each corner checks its 2 adjacent edge directions
    // AND the diagonal direction. It accumulates from all three.
    // corner(0,0) checks -U, -V, diag(-U,-V)
    // corner(1,0) checks +U, -V, diag(+U,-V)
    // corner(1,1) checks +U, +V, diag(+U,+V)
    // corner(0,1) checks -U, +V, diag(-U,+V)

    let corner_00_disp = displacement_sum(disp_neg_u, disp_neg_v, disp_diag_nu_nv);
    let corner_10_disp = displacement_sum(disp_pos_u, disp_neg_v, disp_diag_pu_nv);
    let corner_11_disp = displacement_sum(disp_pos_u, disp_pos_v, disp_diag_pu_pv);
    let corner_01_disp = displacement_sum(disp_neg_u, disp_pos_v, disp_diag_nu_pv);

    // Edge midpoint displacements: each checks 1 perpendicular direction
    // mid(0.5,0) checks -V
    // mid(1,0.5) checks +U
    // mid(0.5,1) checks +V
    // mid(0,0.5) checks -U
    let mid_bot_disp: f32 = if disp_neg_v { 0.5 } else { 0.0 };
    let mid_right_disp: f32 = if disp_pos_u { 0.5 } else { 0.0 };
    let mid_top_disp: f32 = if disp_pos_v { 0.5 } else { 0.0 };
    let mid_left_disp: f32 = if disp_neg_u { 0.5 } else { 0.0 };

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

    // 8 triangles in pinwheel pattern (CCW when viewed from outside):
    // Tri 1: center, 00, mid_left
    // Tri 2: center, mid_left, 01
    // Tri 3: center, 01, mid_top
    // Tri 4: center, mid_top, 11
    // Tri 5: center, 11, mid_right
    // Tri 6: center, mid_right, 10
    // Tri 7: center, 10, mid_bot
    // Tri 8: center, mid_bot, 00
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

/// Compute corner displacement: sum of 0.5 for each true direction, capped
/// conceptually but in practice the sum of three bools * 0.5 gives 0.0..1.5.
/// Per the plan, corners accumulate 0.0, 0.5, or 1.0 from the two edge
/// directions, plus an additional 0.5 from the diagonal. We cap at a
/// reasonable maximum but the plan says corners check 2 edge directions
/// (each contributing 0.5) for a max of 1.0. The diagonal is a third
/// independent check. Let's keep it simple: each true adds 0.5.
fn displacement_sum(edge_a: bool, edge_b: bool, diag: bool) -> f32 {
    let mut d = 0.0_f32;
    if edge_a {
        d += 0.5;
    }
    if edge_b {
        d += 0.5;
    }
    if diag {
        d += 0.5;
    }
    d
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

        // Welded vertex count: 8 corners + 12 edge midpoints + 6 face centers = 26
        assert_eq!(mesh.positions.len(), 26);
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
        // Without welding, each voxel would create its own copy.
        // With welding, edge midpoints and corners at y=6.0 should appear once.
        let boundary_verts: Vec<_> = mesh
            .positions
            .iter()
            .filter(|p| (p[1] - 6.0).abs() < 1e-5)
            .collect();

        // At y=6.0, there should be exactly 8 unique positions:
        // 4 corners of the shared plane + 4 edge midpoints of that plane.
        // (Face centers are at y=5.5 and y=6.5, not at y=6.0.)
        assert_eq!(
            boundary_verts.len(),
            8,
            "Expected 8 welded boundary vertices, got {:?}",
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
            assert!(
                dot > 0.0,
                "Normal at vertex {:?} (pos {:?}) should point outward, dot={:.4}",
                i,
                pos,
                dot
            );
        }
    }
}
