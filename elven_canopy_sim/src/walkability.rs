// Walkability predicates for voxel-direct ground pathfinding.
//
// `footprint_walkable` is the single authority for "can a creature with this
// footprint stand at this position." All walkability checks — pathfinding,
// movement, combat, grazing, gravity — go through this function.
//
// `ground_neighbors` centralizes neighbor expansion for ground creatures,
// including surface-snap logic for large (2×2×2) footprints. Callers like
// `astar_ground`, `ground_random_wander`, and `ground_flee_step` use this
// instead of rolling their own neighbor loops.
//
// See also: `pathfinding.rs` (A* using these predicates), `nav.rs` (EdgeType
// enum and DIST_SCALE constants), `world.rs` (VoxelWorld), `types.rs`
// (FaceData, FaceDirection, FaceType).

use crate::nav::EdgeType;
use crate::pathfinding::NEIGHBOR_OFFSETS;
use crate::types::{FaceData, FaceDirection, FaceType, VoxelCoord, VoxelType};
use crate::world::VoxelWorld;
use std::collections::BTreeMap;

/// 6 face-neighbor offsets (±x, ±y, ±z).
pub const FACE_OFFSETS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// 4 horizontal face-neighbor offsets (±x, ±z). Used in the climber support
/// section of `footprint_walkable` instead of `FACE_OFFSETS`. Excludes both
/// vertical offsets:
/// - (0, -1, 0): applied at y-1, this reaches y-2, finding the floor solid
///   even when the creature is far above the floor — the root cause of the
///   large-creature levitation bug.
/// - (0, 1, 0): applied at y-1, this reaches y+0 (the creature's own anchor
///   level), which is always air (the footprint all-air check already passed),
///   so it never contributes useful information.
///
/// Downward support is already handled by the direct-support (y-1) and
/// deep-support (y-2) checks that precede the climber section.
const CLIMBER_FACE_OFFSETS: [(i32, i32, i32); 4] = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];

/// Check whether a ground-creature footprint anchored at `anchor` is walkable.
/// The anchor is the min-corner of the bounding box. This is the **single
/// authority** for walkability — all code paths use this function.
///
/// ## [1,1,1] creatures
///
/// The voxel at anchor must be non-solid and y >= 1. `BuildingInterior` and
/// ladder voxels are always walkable. Otherwise, the voxel must have a solid
/// neighbor in at least one of the 6 face directions. Face data from
/// `BuildingInterior` neighbors with blocking faces also provides support.
///
/// This covers both climbers and non-climbers — the pathfinder's edge-type
/// filter prevents non-climbers from reaching wall/climb positions.
///
/// ## [2,2,2] creatures (the only other size that currently exists)
///
/// All 8 voxels in the footprint must be non-solid and y >= 1. Then we
/// check support across the 4 ground-plane columns (footprint X × Z):
///
/// **Non-climber support (solid directly below):**
/// - 3+ columns have solid at y-1 → walkable, OR
/// - 1+ column has solid at y-1 AND 2+ columns have solid at y-2 → walkable.
///
/// The 1+2 fallback handles terrain inclines where `large_node_surface_y`
/// places the anchor at `max_surface + 1` and only the highest column(s)
/// have solid directly at y-1.
///
/// **Climber support (any face-adjacent solid):**
/// Large climbers exist (trolls have `climb_ticks_per_voxel`), so the climber
/// support rules are actively used, not just forward-compatible. The same
/// 3-of-4 / 1+2 thresholds apply, but each column counts as supported if
/// ANY of its 6 face neighbors at the check depth is solid. The pathfinder's
/// edge-type filter prevents non-climbers from reaching climbing positions.
pub fn footprint_walkable(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    anchor: VoxelCoord,
    footprint: [u8; 3],
) -> bool {
    // Fast path for 1x1x1 creatures (the common case).
    if footprint == [1, 1, 1] {
        return footprint_walkable_1x1(world, face_data, anchor);
    }

    // --- Large footprint (2×2×2) ---
    // All voxels in the footprint must be non-solid and y >= 1.
    for dx in 0..footprint[0] as i32 {
        for dy in 0..footprint[1] as i32 {
            for dz in 0..footprint[2] as i32 {
                let v = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                if v.y < 1 || world.get(v).is_solid() {
                    return false;
                }
            }
        }
    }

    // Support check across the 4 ground-plane columns.
    //
    // Non-climber rules (solid directly below):
    //   3+ columns with solid at y-1 → supported, OR
    //   1+ column with solid at y-1 AND 2+ columns with solid at y-2 → supported.
    //
    // Climber rules (horizontal face-adjacent solid — used by trolls):
    //   Body-level adjacency: any footprint voxel horizontally adjacent to solid.
    //   Below-level face support: same 3/1+2 thresholds with 4 horizontal face
    //   neighbors (vertical excluded to prevent levitation).

    // --- Non-climber: count columns with solid directly at y-1 ---
    let mut direct_support = 0u32;
    for dx in 0..footprint[0] as i32 {
        for dz in 0..footprint[2] as i32 {
            if world
                .get(VoxelCoord::new(anchor.x + dx, anchor.y - 1, anchor.z + dz))
                .is_solid()
            {
                direct_support += 1;
            }
        }
    }
    if direct_support >= 3 {
        return true;
    }
    if direct_support >= 1 {
        let mut deep_support = 0u32;
        for dx in 0..footprint[0] as i32 {
            for dz in 0..footprint[2] as i32 {
                if world
                    .get(VoxelCoord::new(anchor.x + dx, anchor.y - 2, anchor.z + dz))
                    .is_solid()
                {
                    deep_support += 1;
                }
            }
        }
        if deep_support >= 2 {
            return true;
        }
    }

    // --- Climber support: body-level adjacency OR below-level face support ---
    // Large climbers exist (trolls have climb_ticks_per_voxel), so this path
    // is actively used.
    //
    // Two independent checks, either of which grants walkability:
    //
    // 1. Body-level adjacency: any footprint voxel with a horizontal face-
    //    adjacent solid. This is the core "clinging to a surface" check for
    //    climbing creatures — the creature's body touches a wall, trunk, or
    //    branch. Uses CLIMBER_FACE_OFFSETS (horizontal only) because:
    //    - (0,-1,0) from body voxels can reach below the footprint, finding
    //      the floor even when the creature is far above it (levitation bug).
    //    - (0,1,0) from the top row reaches above the footprint, not useful
    //      for support.
    //
    // 2. Below-level face support (y-1, y-2) with direct_support >= 1 gate:
    //    handles positions on partial surfaces like platform edges where not
    //    all 4 columns have ground directly below, but horizontally adjacent
    //    columns do. The direct_support >= 1 gate ensures there's at least one
    //    actual ground column — without it, face support from structures that
    //    are entirely below the creature (e.g., branch tips at y-1/y-2 with no
    //    solid at foot level) would allow floating.

    // Check 1: body-level horizontal adjacency.
    for dy in 0..footprint[1] as i32 {
        for dx in 0..footprint[0] as i32 {
            for dz in 0..footprint[2] as i32 {
                let body = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                if CLIMBER_FACE_OFFSETS.iter().any(|&(fx, fy, fz)| {
                    world
                        .get(VoxelCoord::new(body.x + fx, body.y + fy, body.z + fz))
                        .is_solid()
                }) {
                    return true;
                }
            }
        }
    }

    // Check 2: below-level face support, gated on at least 1 column having
    // solid directly at y-1. Uses CLIMBER_FACE_OFFSETS (horizontal only) to
    // avoid the (0,-1,0) offset from y-1 reaching y-2 (the levitation bug).
    if direct_support >= 1 {
        let mut face_support = 0u32;
        for dx in 0..footprint[0] as i32 {
            for dz in 0..footprint[2] as i32 {
                let below = VoxelCoord::new(anchor.x + dx, anchor.y - 1, anchor.z + dz);
                if CLIMBER_FACE_OFFSETS.iter().any(|&(fx, fy, fz)| {
                    world
                        .get(VoxelCoord::new(below.x + fx, below.y + fy, below.z + fz))
                        .is_solid()
                }) {
                    face_support += 1;
                }
            }
        }
        if face_support >= 3 {
            return true;
        }
        if face_support >= 1 {
            let mut deep_face = 0u32;
            for dx in 0..footprint[0] as i32 {
                for dz in 0..footprint[2] as i32 {
                    let deep = VoxelCoord::new(anchor.x + dx, anchor.y - 2, anchor.z + dz);
                    if CLIMBER_FACE_OFFSETS.iter().any(|&(fx, fy, fz)| {
                        world
                            .get(VoxelCoord::new(deep.x + fx, deep.y + fy, deep.z + fz))
                            .is_solid()
                    }) {
                        deep_face += 1;
                    }
                }
            }
            if deep_face >= 2 {
                return true;
            }
        }
    }
    false
}

/// Inner walkability check for 1×1×1 creatures. Not public — all external
/// callers go through `footprint_walkable`.
fn footprint_walkable_1x1(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
) -> bool {
    if pos.y < 1 {
        return false;
    }
    let voxel = world.get(pos);
    if voxel.is_solid() {
        return false;
    }
    if voxel == VoxelType::BuildingInterior || voxel.is_ladder() {
        return true;
    }
    // Air voxel: check face neighbors for solid or blocking building faces.
    FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
        let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
        let ntype = world.get(neighbor);
        if ntype.is_solid() {
            return true;
        }
        // Check if neighbor is BuildingInterior with a blocking face toward us.
        if ntype == VoxelType::BuildingInterior
            && let Some(fd) = face_data.get(&neighbor)
        {
            let dir = FaceDirection::from_offset(-dx, -dy, -dz);
            if let Some(d) = dir {
                return fd.get(d).blocks_movement();
            }
        }
        false
    })
}

/// Compute the surface Y (air voxel above ground) for a 2×2 footprint
/// anchored at `(ax, az)`. Returns `None` if any column lacks solid ground
/// or if height variation across the 4 columns exceeds 1 voxel.
///
/// Used by `ground_neighbors` for large-creature surface snapping and by
/// `creature.rs` for finding landing positions during gravity.
pub fn large_node_surface_y(world: &VoxelWorld, ax: i32, az: i32) -> Option<i32> {
    let mut min_surface = i32::MAX;
    let mut max_surface = i32::MIN;
    for dz in 0..2 {
        for dx in 0..2 {
            let top_solid = top_solid_y_from_spans(world, (ax + dx) as u32, (az + dz) as u32);
            match top_solid {
                Some(y) => {
                    min_surface = min_surface.min(y as i32);
                    max_surface = max_surface.max(y as i32);
                }
                None => return None,
            }
        }
    }
    if max_surface - min_surface > 1 {
        return None;
    }
    Some(max_surface + 1)
}

/// Find the topmost solid Y in a column using RLE spans.
/// Returns `None` if the column has no solid voxels.
fn top_solid_y_from_spans(world: &VoxelWorld, x: u32, z: u32) -> Option<u8> {
    let mut top: Option<u8> = None;
    for (vt, _y_start, y_end) in world.column_spans(x, z) {
        if vt.is_solid() {
            top = Some(y_end);
        }
    }
    top
}

/// 8 horizontal neighbor offsets for large-creature surface-snap expansion.
const HORIZONTAL_OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Return all valid neighboring positions for a ground creature with the
/// given footprint, along with scaled distances. For \[1,1,1\] creatures,
/// expands 26 neighbors via NEIGHBOR_OFFSETS and checks `footprint_walkable`
/// and `is_edge_blocked_by_faces`. For larger creatures, additionally tries
/// surface-snapped Y positions for horizontal neighbors, bridging terrain
/// inclines where the valid standing Y changes by more than 1 between
/// adjacent anchors.
///
/// The returned `Vec<(VoxelCoord, u64)>` pairs each reachable position with
/// a scaled Euclidean distance suitable for cost computation. Callers derive
/// edge type and cost from these positions.
pub fn ground_neighbors(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
    footprint: [u8; 3],
) -> Vec<(VoxelCoord, u64)> {
    let mut result = Vec::new();

    // Standard 26-neighbor expansion.
    for &(dx, dy, dz, dist_scaled) in &NEIGHBOR_OFFSETS {
        let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
        if !footprint_walkable(world, face_data, neighbor, footprint) {
            continue;
        }
        if is_edge_blocked_by_faces(face_data, pos, neighbor) {
            continue;
        }
        result.push((neighbor, dist_scaled));
    }

    // Large creature surface-snap neighbors: for each horizontal (dx,dz)
    // offset, compute the valid large-creature standing y via
    // large_node_surface_y and try that position if it differs from the
    // y values already tried by NEIGHBOR_OFFSETS (±1).
    let is_large = footprint[0] > 1 || footprint[2] > 1;
    if is_large {
        let wx = world.size_x as i32;
        let wz = world.size_z as i32;
        for &(dx, dz) in &HORIZONTAL_OFFSETS {
            let nx = pos.x + dx;
            let nz = pos.z + dz;
            // Bounds check: large_node_surface_y accesses (nx..nx+2, nz..nz+2).
            if nx < 0 || nx + 1 >= wx || nz < 0 || nz + 1 >= wz {
                continue;
            }
            if let Some(sy) = large_node_surface_y(world, nx, nz) {
                // Skip if already covered by NEIGHBOR_OFFSETS (y within ±1).
                let dy = sy - pos.y;
                if (-1..=1).contains(&dy) {
                    continue;
                }
                let neighbor = VoxelCoord::new(nx, sy, nz);
                if !footprint_walkable(world, face_data, neighbor, footprint) {
                    continue;
                }
                if is_edge_blocked_by_faces(face_data, pos, neighbor) {
                    continue;
                }
                // Approximate 3D octile distance: 1024*max + 424*mid + 318*min.
                let adx = dx.unsigned_abs() as u64;
                let adz = dz.unsigned_abs() as u64;
                let ady = dy.unsigned_abs() as u64;
                let max_comp = adx.max(adz).max(ady);
                let min_comp = adx.min(adz).min(ady);
                let mid_comp = adx + adz + ady - max_comp - min_comp;
                let dist_scaled = 1024 * max_comp + 424 * mid_comp + 318 * min_comp;
                result.push((neighbor, dist_scaled));
            }
        }
    }

    result
}

/// Determine what surface a creature at `pos` is touching.
///
/// Priority: the voxel directly below takes precedence (creature standing on
/// it). Otherwise check horizontal neighbors and above in a fixed order and
/// return the first solid type found (creature clinging to it).
///
/// For `BuildingInterior` voxels, face data determines the surface type:
/// - Floor face → `GrownPlatform` (walkable)
/// - Wall/Window side → `GrownWall` (climbable)
/// - Ceiling face → `GrownPlatform` (walkable on top)
///
/// For Air voxels next to `BuildingInterior` with blocking faces, the face
/// type determines the surface similarly.
pub fn derive_surface_type(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
) -> VoxelType {
    let voxel = world.get(pos);

    // Ladder voxels: surface type is the ladder type itself.
    if voxel.is_ladder() {
        return voxel;
    }

    // BuildingInterior voxels derive surface from their own face data.
    if voxel == VoxelType::BuildingInterior
        && let Some(fd) = face_data.get(&pos)
    {
        // Check Floor first (standing on it).
        if fd.get(FaceDirection::NegY).blocks_movement() {
            return VoxelType::GrownPlatform;
        }
        // Check side faces for walls.
        for &dir in &[
            FaceDirection::PosX,
            FaceDirection::NegX,
            FaceDirection::PosZ,
            FaceDirection::NegZ,
        ] {
            if fd.get(dir).blocks_movement() {
                return VoxelType::GrownWall;
            }
        }
        // Check ceiling.
        if fd.get(FaceDirection::PosY).blocks_movement() {
            return VoxelType::GrownPlatform;
        }
        // Fallback: check solid neighbors like normal Air.
    }

    // Check below first (creature standing on this surface).
    let below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
    let below_type = world.get(below);
    if below_type.is_solid() {
        return below_type;
    }
    // Check if below is BuildingInterior with a Ceiling face pointing up.
    if below_type == VoxelType::BuildingInterior
        && let Some(fd) = face_data.get(&below)
        && fd.get(FaceDirection::PosY).blocks_movement()
    {
        return VoxelType::GrownPlatform;
    }

    // Check horizontal neighbors and above in fixed order.
    let side_offsets: [(i32, i32, i32); 5] =
        [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)];
    for (dx, dy, dz) in side_offsets {
        let neighbor = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
        let ntype = world.get(neighbor);
        if ntype.is_solid() {
            return ntype;
        }
        // Check if neighbor is BuildingInterior with blocking face toward pos.
        if ntype == VoxelType::BuildingInterior
            && let Some(fd) = face_data.get(&neighbor)
        {
            let dir = FaceDirection::from_offset(-dx, -dy, -dz);
            if let Some(d) = dir {
                let ft = fd.get(d);
                if ft.blocks_movement() {
                    return match ft {
                        FaceType::Floor | FaceType::Ceiling => VoxelType::GrownPlatform,
                        _ => VoxelType::GrownWall,
                    };
                }
            }
        }
    }

    // Shouldn't happen — only called for voxels with solid face neighbors.
    VoxelType::Dirt
}

/// Check whether face data blocks movement from `from` to `to`.
///
/// For each nonzero component of the offset (dx, dy, dz):
/// - Check the source voxel's face in that component direction
/// - Check the dest voxel's face in the opposite direction
/// - If any checked face blocks movement → edge is blocked
///
/// For diagonals: if ANY component direction is blocked, the whole diagonal
/// is blocked (prevents corner-cutting through walls).
pub fn is_edge_blocked_by_faces(
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    from: VoxelCoord,
    to: VoxelCoord,
) -> bool {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;

    // Check each nonzero component direction.
    let components: [(i32, i32, i32); 3] = [
        (dx.signum(), 0, 0),
        (0, dy.signum(), 0),
        (0, 0, dz.signum()),
    ];

    for (cx, cy, cz) in components {
        if cx == 0 && cy == 0 && cz == 0 {
            continue;
        }
        // Check source voxel's face in this direction.
        if let Some(fd) = face_data.get(&from)
            && let Some(dir) = FaceDirection::from_offset(cx, cy, cz)
            && fd.get(dir).blocks_movement()
        {
            return true;
        }
        // Check dest voxel's face in the opposite direction.
        if let Some(fd) = face_data.get(&to)
            && let Some(dir) = FaceDirection::from_offset(-cx, -cy, -cz)
            && fd.get(dir).blocks_movement()
        {
            return true;
        }
    }
    false
}

/// Determine the edge type for a connection between two walkable positions
/// based on their surface types and positions.
pub fn derive_edge_type(
    from_surface: VoxelType,
    to_surface: VoxelType,
    from_pos: VoxelCoord,
    to_pos: VoxelCoord,
) -> EdgeType {
    use VoxelType::*;

    // Same surface type on both sides.
    if from_surface == to_surface {
        return match from_surface {
            Dirt => EdgeType::Ground,
            Trunk => {
                if from_pos.y != to_pos.y {
                    EdgeType::TrunkClimb
                } else {
                    EdgeType::TrunkCircumference
                }
            }
            Branch | Leaf | Fruit | GrownPlatform | Root | BuildingInterior | Strut => {
                EdgeType::BranchWalk
            }
            GrownWall => EdgeType::TrunkClimb,
            WoodLadder => EdgeType::WoodLadderClimb,
            RopeLadder => EdgeType::RopeLadderClimb,
            Air => EdgeType::BranchWalk, // shouldn't happen
        };
    }

    // Mixed surface types — one side is ladder, other is not → BranchWalk
    // (stepping on/off the ladder).
    if matches!(from_surface, WoodLadder | RopeLadder)
        || matches!(to_surface, WoodLadder | RopeLadder)
    {
        return EdgeType::BranchWalk;
    }

    // Mixed surface types.
    match (from_surface, to_surface) {
        (Dirt, Trunk) | (Trunk, Dirt) => EdgeType::GroundToTrunk,
        (Dirt, Root) | (Root, Dirt) => EdgeType::Ground,
        (Trunk, Root) | (Root, Trunk) => EdgeType::TrunkClimb,
        (Trunk, Branch) | (Branch, Trunk) | (Trunk, Leaf) | (Leaf, Trunk) => EdgeType::TrunkClimb,
        _ => {
            // GrownWall → climb-like; everything else → walk-like.
            if matches!(from_surface, GrownWall) || matches!(to_surface, GrownWall) {
                EdgeType::TrunkClimb
            } else {
                EdgeType::BranchWalk
            }
        }
    }
}

/// Find the nearest walkable voxel to `pos` within `max_distance` (Manhattan)
/// for a creature with the given footprint.
///
/// Expanding-box search: checks all voxels in expanding Manhattan-radius rings
/// around `pos`. Returns the closest walkable position, or `None` if none found.
///
/// For [1,1,1] creatures, this is equivalent to the old `find_nearest_walkable`.
/// For larger footprints, tests `footprint_walkable` at each candidate.
pub fn find_nearest_walkable(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
    max_distance: u32,
    footprint: [u8; 3],
) -> Option<VoxelCoord> {
    find_nearest_walkable_filtered(world, face_data, pos, max_distance, footprint, |_| true)
}

/// Find the nearest walkable ground-level voxel (surface type `Dirt`) to `pos`.
pub fn find_nearest_ground_walkable(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
    max_distance: u32,
    footprint: [u8; 3],
) -> Option<VoxelCoord> {
    find_nearest_walkable_filtered(world, face_data, pos, max_distance, footprint, |p| {
        derive_surface_type(world, face_data, p) == VoxelType::Dirt
    })
}

/// Find the nearest position where a creature's full footprint is walkable,
/// with an additional filter.
///
/// Expanding-box search around `pos`, testing `footprint_walkable` at each
/// candidate. This consolidates the old `find_nearest_walkable`,
/// `find_nearest_footprint_walkable`, and `find_nearest_ground_walkable` — they
/// all delegate here with different filters.
fn find_nearest_walkable_filtered(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    pos: VoxelCoord,
    max_distance: u32,
    footprint: [u8; 3],
    filter: impl Fn(VoxelCoord) -> bool,
) -> Option<VoxelCoord> {
    // Check the position itself first.
    if footprint_walkable(world, face_data, pos, footprint) && filter(pos) {
        return Some(pos);
    }

    let mut best: Option<(u32, VoxelCoord)> = None;

    for radius in 1..=max_distance as i32 {
        // Early termination: if we already found something closer than this
        // shell's minimum distance, we're done.
        if let Some((best_dist, _)) = best
            && radius as u32 > best_dist
        {
            break;
        }

        // Enumerate all positions at Manhattan distance == radius.
        // For 3D Manhattan distance r: |dx| + |dy| + |dz| = r.
        for dx in -radius..=radius {
            let remaining = radius - dx.abs();
            for dy in -remaining..=remaining {
                let dz_abs = remaining - dy.abs();
                // dz can be +dz_abs or -dz_abs (and 0 if dz_abs == 0).
                let dz_values: &[i32] = if dz_abs == 0 {
                    &[0]
                } else {
                    &[-dz_abs, dz_abs]
                };
                for &dz in dz_values {
                    let candidate = VoxelCoord::new(pos.x + dx, pos.y + dy, pos.z + dz);
                    if !world.in_bounds(candidate) {
                        continue;
                    }
                    if footprint_walkable(world, face_data, candidate, footprint)
                        && filter(candidate)
                    {
                        let dist = pos.manhattan_distance(candidate);
                        if best.is_none() || dist < best.unwrap().0 {
                            best = Some((dist, candidate));
                        }
                    }
                }
            }
        }

        // If we found something at this radius, it's the closest.
        if best.is_some() {
            break;
        }
    }

    best.map(|(_, coord)| coord)
}

/// Find up to `count` distinct walkable positions near `center`, expanding
/// outward via BFS on the walkable voxel grid. The center position is always
/// the first result. Used by group move commands to spread creatures across
/// nearby positions instead of stacking them all on the same voxel.
pub fn spread_destinations(
    world: &VoxelWorld,
    face_data: &BTreeMap<VoxelCoord, FaceData>,
    center: VoxelCoord,
    count: usize,
    footprint: [u8; 3],
) -> Vec<VoxelCoord> {
    if count == 0 || !footprint_walkable(world, face_data, center, footprint) {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(count);
    result.push(center);
    if count == 1 {
        return result;
    }

    // BFS outward from center using ground_neighbors for expansion.
    let mut visited = std::collections::BTreeSet::new();
    visited.insert(center);
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(center);

    while let Some(pos) = queue.pop_front() {
        for (neighbor, _dist) in ground_neighbors(world, face_data, pos, footprint) {
            if visited.contains(&neighbor) {
                continue;
            }
            if !world.in_bounds(neighbor) {
                continue;
            }
            visited.insert(neighbor);
            result.push(neighbor);
            if result.len() >= count {
                return result;
            }
            queue.push_back(neighbor);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::VoxelWorld;

    /// Create a flat dirt floor at y=floor_y, air above.
    fn ground_world(sx: u32, sy: u32, sz: u32, floor_y: i32) -> VoxelWorld {
        let mut world = VoxelWorld::new(sx, sy, sz);
        for x in 0..sx as i32 {
            for z in 0..sz as i32 {
                world.set(VoxelCoord::new(x, floor_y, z), VoxelType::Dirt);
            }
        }
        world
    }

    fn no_faces() -> BTreeMap<VoxelCoord, FaceData> {
        BTreeMap::new()
    }

    const FP1: [u8; 3] = [1, 1, 1];

    #[test]
    fn walkable_air_above_dirt() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // Air at y=6 above dirt at y=5 — walkable.
        assert!(footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 6, 5),
            FP1
        ));
        // Dirt at y=5 — solid, not walkable.
        assert!(!footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 5, 5),
            FP1
        ));
        // Air at y=10 with no solid neighbor — not walkable.
        assert!(!footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 10, 5),
            FP1
        ));
    }

    #[test]
    fn walkable_y_below_1() {
        let world = ground_world(16, 16, 16, 0);
        let fd = no_faces();
        // Even though y=0 has dirt below, y < 1 returns false.
        assert!(!footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 0, 5),
            FP1
        ));
        // y=1 above dirt at y=0 — walkable.
        assert!(footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 1, 5),
            FP1
        ));
    }

    #[test]
    fn find_nearest_walkable_at_self() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let pos = VoxelCoord::new(5, 6, 5);
        assert_eq!(find_nearest_walkable(&world, &fd, pos, 5, FP1), Some(pos));
    }

    #[test]
    fn find_nearest_walkable_from_solid() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // Searching from inside dirt at y=5 — should find walkable at distance 1.
        // Both y=4 (air below dirt) and y=6 (air above dirt) are at distance 1.
        let pos = VoxelCoord::new(5, 5, 5);
        let result = find_nearest_walkable(&world, &fd, pos, 5, FP1);
        assert!(result.is_some());
        let found = result.unwrap();
        assert_eq!(pos.manhattan_distance(found), 1);
        assert!(footprint_walkable(&world, &fd, found, FP1));
    }

    #[test]
    fn find_nearest_walkable_none_in_range() {
        let world = VoxelWorld::new(16, 16, 16); // all air
        let fd = no_faces();
        let pos = VoxelCoord::new(5, 5, 5);
        assert_eq!(find_nearest_walkable(&world, &fd, pos, 5, FP1), None);
    }

    #[test]
    fn find_nearest_ground_walkable_filters_by_dirt() {
        let mut world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // Place trunk near pos — the nearest walkable has Trunk surface, not Dirt.
        world.set(VoxelCoord::new(5, 5, 5), VoxelType::Trunk);
        world.set(VoxelCoord::new(5, 6, 5), VoxelType::Trunk);
        let pos = VoxelCoord::new(6, 6, 5);
        // find_nearest_walkable returns pos itself (adjacent to trunk, walkable).
        assert_eq!(find_nearest_walkable(&world, &fd, pos, 5, FP1), Some(pos));
        // find_nearest_ground_walkable should find a Dirt-surface position.
        let result = find_nearest_ground_walkable(&world, &fd, pos, 5, FP1);
        assert!(result.is_some());
        let found = result.unwrap();
        assert_eq!(derive_surface_type(&world, &fd, found), VoxelType::Dirt);
    }

    #[test]
    fn spread_destinations_returns_center_first() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let center = VoxelCoord::new(5, 6, 5);
        let result = spread_destinations(&world, &fd, center, 5, FP1);
        assert!(!result.is_empty());
        assert_eq!(result[0], center);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn spread_destinations_count_1() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let center = VoxelCoord::new(5, 6, 5);
        let result = spread_destinations(&world, &fd, center, 1, FP1);
        assert_eq!(result, vec![center]);
    }

    #[test]
    fn spread_destinations_unwalkable_center() {
        let world = VoxelWorld::new(16, 16, 16); // all air
        let fd = no_faces();
        let center = VoxelCoord::new(5, 5, 5);
        let result = spread_destinations(&world, &fd, center, 5, FP1);
        assert!(result.is_empty());
    }

    #[test]
    fn spread_destinations_no_duplicates() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let center = VoxelCoord::new(5, 6, 5);
        let result = spread_destinations(&world, &fd, center, 20, FP1);
        let unique: std::collections::BTreeSet<_> = result.iter().collect();
        assert_eq!(
            unique.len(),
            result.len(),
            "spread_destinations returned duplicates"
        );
    }

    #[test]
    fn footprint_walkable_1x1x1() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        assert!(footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 6, 5),
            [1, 1, 1]
        ));
        assert!(!footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 5, 5),
            [1, 1, 1]
        ));
    }

    #[test]
    fn footprint_walkable_2x2x2() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // 2x2x2 at y=6: positions (5,6,5), (6,6,5), (5,6,6), (6,6,6) must be walkable
        // and (5,7,5), (6,7,5), (5,7,6), (6,7,6) — but y=7 is air with no adjacent solid
        // except the dirt at y=5 is 2 away... so only face-adjacent counts.
        // Actually y=7 is NOT adjacent to dirt at y=5. Let's check: y=7 has no solid
        // face neighbor (dirt is at y=5, 2 away). So [2,2,2] won't be walkable at y=6
        // because the y+1=7 layer isn't walkable.
        // Use [2,1,2] instead for a ground-only footprint.
        assert!(footprint_walkable(
            &world,
            &fd,
            VoxelCoord::new(5, 6, 5),
            [2, 1, 2]
        ));
    }

    // -----------------------------------------------------------------------
    // 2x2x2 footprint walkability tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_footprint_walkable_2x2x2_actual_footprint() {
        // Flat dirt floor at y=5. A [2,2,2] creature at y=6 occupies y=6..7.
        // All 8 voxels are air, all 4 ground columns have solid at y=5.
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        assert!(
            footprint_walkable(&world, &fd, VoxelCoord::new(5, 6, 5), [2, 2, 2]),
            "2x2x2 should be walkable at y=6 on flat dirt floor at y=5"
        );

        // Block one upper-layer voxel (y=7) — should become unwalkable.
        let mut world2 = ground_world(16, 16, 16, 5);
        world2.set(VoxelCoord::new(5, 7, 5), VoxelType::Dirt);
        assert!(
            !footprint_walkable(&world2, &fd, VoxelCoord::new(5, 6, 5), [2, 2, 2]),
            "2x2x2 should NOT be walkable when one upper voxel is solid"
        );
    }

    #[test]
    fn test_footprint_walkable_2x2x2_three_columns_supported() {
        // 3 of 4 ground columns have solid at y-1 (y=5), 1 column does not.
        // 3 >= 3 threshold → walkable.
        let mut world = VoxelWorld::new(16, 16, 16);
        let fd = no_faces();
        // Place dirt at y=5 for 3 columns but not (6,5,6).
        world.set(VoxelCoord::new(5, 5, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(6, 5, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(5, 5, 6), VoxelType::Dirt);
        // (6,5,6) is air — only 3 columns supported.
        assert!(
            footprint_walkable(&world, &fd, VoxelCoord::new(5, 6, 5), [2, 2, 2]),
            "3 of 4 columns supported at y-1 should be walkable"
        );
    }

    #[test]
    fn test_footprint_walkable_2x2x2_two_columns_unsupported() {
        // In a 2x2 footprint, all 4 ground columns are face-adjacent to each
        // other. So 2 columns with solid at y-1 gives all 4 columns face
        // support via the climber fallback. To test the "not enough support"
        // case, we need NO solid anywhere near the footprint (y-1 or
        // face-adjacent), with solid ONLY at y-3 (too far for any rule).
        let mut world = VoxelWorld::new(16, 16, 16);
        let fd = no_faces();
        // Place solid only at y=3 (y-3 from anchor y=6), too deep for any rule.
        world.set(VoxelCoord::new(5, 3, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(6, 3, 5), VoxelType::Dirt);
        // No solid at y=5 (y-1) or y=4 (y-2) or face-adjacent to y-1.
        assert!(
            !footprint_walkable(&world, &fd, VoxelCoord::new(5, 6, 5), [2, 2, 2]),
            "Solid only at y-3 should NOT provide support for 2x2x2 creature"
        );
    }

    #[test]
    fn test_footprint_walkable_2x2x2_one_direct_two_deep() {
        // 1 column has solid at y-1, 2 columns have solid at y-2 (but not y-1).
        // 1+ at y-1 AND 2+ at y-2 → walkable.
        let mut world = VoxelWorld::new(16, 16, 16);
        let fd = no_faces();
        // 1 column with direct support at y=5.
        world.set(VoxelCoord::new(5, 5, 5), VoxelType::Dirt);
        // 2 columns with deep support at y=4 (no solid at y=5).
        world.set(VoxelCoord::new(6, 4, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(5, 4, 6), VoxelType::Dirt);
        // (6,5,6) has no support at all.
        assert!(
            footprint_walkable(&world, &fd, VoxelCoord::new(5, 6, 5), [2, 2, 2]),
            "1 direct + 2 deep support should be walkable"
        );
    }

    #[test]
    fn test_footprint_walkable_2x2x2_zero_direct_support() {
        // 0 columns have solid at y-1 directly, but solid at y-2 makes the
        // y-2 voxels face-adjacent to y-1 (via the -Y face offset). The
        // climber fallback at y-1 will see face_support >= 1, and then the
        // deep climber check at y-2 will also see face_support >= 2.
        // So solid at y-2 DOES provide support through the climber path.
        //
        // To actually get false, there must be no solid within face-adjacency
        // of y-1 or y-2 for any column — i.e., no solid at y-1, y-2, y-3,
        // or horizontally adjacent at those levels.
        let world = VoxelWorld::new(16, 16, 16);
        let fd = no_faces();
        // Completely empty world — no solid anywhere.
        assert!(
            !footprint_walkable(&world, &fd, VoxelCoord::new(5, 6, 5), [2, 2, 2]),
            "Empty world should NOT be walkable for 2x2x2 creature"
        );
    }

    // -----------------------------------------------------------------------
    // large_node_surface_y tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_large_node_surface_y_flat() {
        let world = ground_world(16, 16, 16, 5);
        // Flat dirt floor at y=5. Surface y for a 2x2 at (5,5) should be 6.
        assert_eq!(large_node_surface_y(&world, 5, 5), Some(6));
    }

    #[test]
    fn test_large_node_surface_y_height_variation_exceeds_1() {
        // One column at height 2, another at height 5 — variation > 1 → None.
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 2, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(6, 2, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(5, 5, 6), VoxelType::Dirt);
        world.set(VoxelCoord::new(6, 5, 6), VoxelType::Dirt);
        assert_eq!(
            large_node_surface_y(&world, 5, 5),
            None,
            "Height variation of 3 should return None"
        );
    }

    #[test]
    fn test_large_node_surface_y_empty_column() {
        // One column in the 2x2 has no solid → None.
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 5, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(6, 5, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(5, 5, 6), VoxelType::Dirt);
        // (6, *, 6) has no solid at all.
        assert_eq!(
            large_node_surface_y(&world, 5, 5),
            None,
            "Empty column should return None"
        );
    }

    // -----------------------------------------------------------------------
    // ground_neighbors tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ground_neighbors_large_surface_snap() {
        // Test ground_neighbors for a [2,2,2] creature on flat terrain.
        // Verifies that neighbors include positions at the ground surface
        // level (y=6), all returned neighbors are walkable, and there are
        // enough neighbors for movement in multiple directions.
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let pos = VoxelCoord::new(5, 6, 5);
        assert!(
            footprint_walkable(&world, &fd, pos, [2, 2, 2]),
            "Starting position should be walkable"
        );
        let neighbors = ground_neighbors(&world, &fd, pos, [2, 2, 2]);
        assert!(
            !neighbors.is_empty(),
            "2x2x2 creature should have ground neighbors on flat terrain"
        );
        // All returned neighbors must be walkable.
        for (n, _dist) in &neighbors {
            assert!(
                footprint_walkable(&world, &fd, *n, [2, 2, 2]),
                "Neighbor {:?} should be walkable for [2,2,2]",
                n
            );
        }
        // Should include y=6 neighbors (ground surface level).
        let ground_level: Vec<_> = neighbors.iter().filter(|(n, _)| n.y == 6).collect();
        assert!(
            !ground_level.is_empty(),
            "Should have neighbors at ground surface level y=6"
        );
        // Should have multiple neighbors (at least 4 cardinal directions).
        assert!(
            neighbors.len() >= 4,
            "Should have at least 4 neighbors, got {}",
            neighbors.len()
        );
    }

    #[test]
    fn test_ground_neighbors_1x1_basic() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        let pos = VoxelCoord::new(5, 6, 5);
        let neighbors = ground_neighbors(&world, &fd, pos, [1, 1, 1]);
        // All neighbors should be walkable and at y=6 on flat ground.
        assert!(!neighbors.is_empty(), "Should have at least some neighbors");
        for (n, _dist) in &neighbors {
            assert!(
                footprint_walkable(&world, &fd, *n, [1, 1, 1]),
                "Neighbor {:?} should be walkable",
                n
            );
            assert_eq!(n.y, 6, "All neighbors should be at y=6 on flat ground");
        }
    }

    // -----------------------------------------------------------------------
    // derive_edge_type tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_derive_edge_type_trunk_vertical_vs_horizontal() {
        // Trunk/Trunk with different Y → TrunkClimb.
        let a = VoxelCoord::new(5, 6, 5);
        let b = VoxelCoord::new(5, 7, 5);
        assert_eq!(
            derive_edge_type(VoxelType::Trunk, VoxelType::Trunk, a, b),
            EdgeType::TrunkClimb,
            "Trunk-to-Trunk with different Y should be TrunkClimb"
        );
        // Trunk/Trunk with same Y → TrunkCircumference.
        let c = VoxelCoord::new(6, 6, 5);
        assert_eq!(
            derive_edge_type(VoxelType::Trunk, VoxelType::Trunk, a, c),
            EdgeType::TrunkCircumference,
            "Trunk-to-Trunk with same Y should be TrunkCircumference"
        );
    }

    #[test]
    fn test_derive_edge_type_dirt_trunk_boundary() {
        let a = VoxelCoord::new(5, 6, 5);
        let b = VoxelCoord::new(6, 6, 5);
        assert_eq!(
            derive_edge_type(VoxelType::Dirt, VoxelType::Trunk, a, b),
            EdgeType::GroundToTrunk,
            "Dirt-to-Trunk should be GroundToTrunk"
        );
        assert_eq!(
            derive_edge_type(VoxelType::Trunk, VoxelType::Dirt, b, a),
            EdgeType::GroundToTrunk,
            "Trunk-to-Dirt should also be GroundToTrunk"
        );
    }

    #[test]
    fn test_derive_edge_type_ladder_transitions() {
        let a = VoxelCoord::new(5, 6, 5);
        let b = VoxelCoord::new(6, 6, 5);
        assert_eq!(
            derive_edge_type(VoxelType::WoodLadder, VoxelType::Dirt, a, b),
            EdgeType::BranchWalk,
            "WoodLadder-to-Dirt should be BranchWalk (stepping on/off ladder)"
        );
        assert_eq!(
            derive_edge_type(VoxelType::Dirt, VoxelType::RopeLadder, a, b),
            EdgeType::BranchWalk,
            "Dirt-to-RopeLadder should be BranchWalk (stepping on/off ladder)"
        );
    }

    // -----------------------------------------------------------------------
    // is_edge_blocked_by_faces tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_edge_blocked_by_faces_no_faces() {
        let fd = no_faces();
        let a = VoxelCoord::new(5, 6, 5);
        let b = VoxelCoord::new(6, 6, 5);
        assert!(
            !is_edge_blocked_by_faces(&fd, a, b),
            "Empty face_data should never block an edge"
        );
        // Also test diagonal.
        let c = VoxelCoord::new(6, 7, 6);
        assert!(
            !is_edge_blocked_by_faces(&fd, a, c),
            "Empty face_data should never block a diagonal edge"
        );
    }

    // -----------------------------------------------------------------------
    // derive_surface_type tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_derive_surface_type_standing_on_dirt() {
        let world = ground_world(16, 16, 16, 5);
        let fd = no_faces();
        // Air voxel at y=6 above Dirt at y=5 → surface type is Dirt.
        assert_eq!(
            derive_surface_type(&world, &fd, VoxelCoord::new(5, 6, 5)),
            VoxelType::Dirt,
            "Air above Dirt should derive surface type Dirt"
        );
    }
}
