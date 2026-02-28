// Building face layout computation.
//
// Computes the per-face `FaceData` for each interior voxel of a building.
// A building is an axis-aligned box of `BuildingInterior` voxels sitting on
// a solid foundation (one layer below `anchor.y`). Walls don't consume voxel
// space — they exist as face restrictions on the interior voxels.
//
// The layout rules are:
// - Bottom layer: -Y = Floor on all voxels.
// - Top layer: +Y = Ceiling on all voxels.
// - Exterior side faces: Window (maximizes visibility).
// - One ground-level door: auto-placed at the center of the +Z edge.
// - Interior-facing sides: Open (no restriction).
//
// The `anchor` is the minimum corner of the interior volume at the foundation
// level (y = foundation top). Interior voxels span:
//   x: anchor.x .. anchor.x + width
//   y: anchor.y + 1 .. anchor.y + 1 + height
//   z: anchor.z .. anchor.z + depth
//
// See also: `types.rs` for `FaceDirection`, `FaceType`, `FaceData`,
// `VoxelCoord`. `sim.rs` for the `DesignateBuilding` command that calls this.
// `nav.rs` for how face data affects pathfinding.
//
// **Critical constraint: determinism.** Uses `BTreeMap` for output ordering.

use crate::types::{FaceData, FaceDirection, FaceType, VoxelCoord};
use std::collections::BTreeMap;

/// Compute the face layout for a building.
///
/// `anchor` is the minimum corner of the building footprint at foundation
/// level. Interior voxels occupy the volume above the foundation:
///   x in [anchor.x, anchor.x + width)
///   y in [anchor.y + 1, anchor.y + 1 + height)
///   z in [anchor.z, anchor.z + depth)
///
/// Returns a map from interior voxel coordinate to its `FaceData`.
pub fn compute_building_face_layout(
    anchor: VoxelCoord,
    width: i32,
    depth: i32,
    height: i32,
) -> BTreeMap<VoxelCoord, FaceData> {
    let mut layout = BTreeMap::new();

    let x_min = anchor.x;
    let x_max = anchor.x + width; // exclusive
    let y_min = anchor.y + 1; // interior starts one above foundation
    let y_max = anchor.y + 1 + height; // exclusive
    let z_min = anchor.z;
    let z_max = anchor.z + depth; // exclusive

    for y in y_min..y_max {
        for z in z_min..z_max {
            for x in x_min..x_max {
                let coord = VoxelCoord::new(x, y, z);
                let mut fd = FaceData::default();

                // Bottom layer gets Floor on -Y.
                if y == y_min {
                    fd.set(FaceDirection::NegY, FaceType::Floor);
                }

                // Top layer gets Ceiling on +Y.
                if y == y_max - 1 {
                    fd.set(FaceDirection::PosY, FaceType::Ceiling);
                }

                // Exterior side faces: Window (or Door for the door position).
                if x == x_min {
                    fd.set(FaceDirection::NegX, FaceType::Window);
                }
                if x == x_max - 1 {
                    fd.set(FaceDirection::PosX, FaceType::Window);
                }
                if z == z_min {
                    fd.set(FaceDirection::NegZ, FaceType::Window);
                }
                if z == z_max - 1 {
                    fd.set(FaceDirection::PosZ, FaceType::Window);
                }

                layout.insert(coord, fd);
            }
        }
    }

    // Place door at ground level, center of +Z edge.
    let door_x = x_min + width / 2;
    let door_y = y_min;
    let door_coord = VoxelCoord::new(door_x, door_y, z_max - 1);
    if let Some(fd) = layout.get_mut(&door_coord) {
        fd.set(FaceDirection::PosZ, FaceType::Door);
    }

    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_3x3x1_layout() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);

        // 3x3x1 = 9 interior voxels (at y=1).
        assert_eq!(layout.len(), 9);

        // All voxels should be at y=1.
        for coord in layout.keys() {
            assert_eq!(coord.y, 1);
        }

        // Corner voxel (0,1,0): should have NegX=Window, NegZ=Window,
        // NegY=Floor, PosY=Ceiling (single floor building).
        let corner = layout.get(&VoxelCoord::new(0, 1, 0)).unwrap();
        assert_eq!(corner.get(FaceDirection::NegX), FaceType::Window);
        assert_eq!(corner.get(FaceDirection::NegZ), FaceType::Window);
        assert_eq!(corner.get(FaceDirection::NegY), FaceType::Floor);
        assert_eq!(corner.get(FaceDirection::PosY), FaceType::Ceiling);
        // Interior-facing sides should be Open.
        assert_eq!(corner.get(FaceDirection::PosX), FaceType::Open);
        assert_eq!(corner.get(FaceDirection::PosZ), FaceType::Open);

        // Center voxel (1,1,1): all sides interior, so only Floor+Ceiling.
        let center = layout.get(&VoxelCoord::new(1, 1, 1)).unwrap();
        assert_eq!(center.get(FaceDirection::NegY), FaceType::Floor);
        assert_eq!(center.get(FaceDirection::PosY), FaceType::Ceiling);
        assert_eq!(center.get(FaceDirection::PosX), FaceType::Open);
        assert_eq!(center.get(FaceDirection::NegX), FaceType::Open);
        assert_eq!(center.get(FaceDirection::PosZ), FaceType::Open);
        assert_eq!(center.get(FaceDirection::NegZ), FaceType::Open);
    }

    #[test]
    fn has_exactly_one_door() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);

        let door_count: usize = layout
            .values()
            .flat_map(|fd| FaceDirection::ALL.iter().map(move |&dir| fd.get(dir)))
            .filter(|&ft| ft == FaceType::Door)
            .count();

        assert_eq!(door_count, 1);

        // Door should be at center of +Z edge at ground level.
        let door_voxel = layout.get(&VoxelCoord::new(1, 1, 2)).unwrap();
        assert_eq!(door_voxel.get(FaceDirection::PosZ), FaceType::Door);
    }

    #[test]
    fn interior_faces_are_open() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 5, 5, 1);

        // A voxel fully interior horizontally (2,1,2) should have Open on
        // all 4 horizontal sides.
        let interior = layout.get(&VoxelCoord::new(2, 1, 2)).unwrap();
        assert_eq!(interior.get(FaceDirection::PosX), FaceType::Open);
        assert_eq!(interior.get(FaceDirection::NegX), FaceType::Open);
        assert_eq!(interior.get(FaceDirection::PosZ), FaceType::Open);
        assert_eq!(interior.get(FaceDirection::NegZ), FaceType::Open);
    }

    #[test]
    fn taller_building_ceiling_only_on_top() {
        let anchor = VoxelCoord::new(0, 0, 0);
        let layout = compute_building_face_layout(anchor, 3, 3, 3);

        // 3x3x3 = 27 interior voxels (y=1,2,3).
        assert_eq!(layout.len(), 27);

        // Bottom layer (y=1): Floor on NegY, no Ceiling on PosY.
        let bottom = layout.get(&VoxelCoord::new(1, 1, 1)).unwrap();
        assert_eq!(bottom.get(FaceDirection::NegY), FaceType::Floor);
        assert_eq!(bottom.get(FaceDirection::PosY), FaceType::Open);

        // Middle layer (y=2): no Floor, no Ceiling.
        let middle = layout.get(&VoxelCoord::new(1, 2, 1)).unwrap();
        assert_eq!(middle.get(FaceDirection::NegY), FaceType::Open);
        assert_eq!(middle.get(FaceDirection::PosY), FaceType::Open);

        // Top layer (y=3): no Floor on NegY, Ceiling on PosY.
        let top = layout.get(&VoxelCoord::new(1, 3, 1)).unwrap();
        assert_eq!(top.get(FaceDirection::NegY), FaceType::Open);
        assert_eq!(top.get(FaceDirection::PosY), FaceType::Ceiling);
    }

    #[test]
    fn non_origin_anchor() {
        let anchor = VoxelCoord::new(10, 5, 20);
        let layout = compute_building_face_layout(anchor, 3, 3, 1);

        // Interior at y=6 (one above foundation at y=5).
        assert_eq!(layout.len(), 9);
        assert!(layout.contains_key(&VoxelCoord::new(10, 6, 20)));
        assert!(layout.contains_key(&VoxelCoord::new(12, 6, 22)));
        assert!(!layout.contains_key(&VoxelCoord::new(10, 5, 20))); // foundation level
    }
}
