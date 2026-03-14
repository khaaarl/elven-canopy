// Dense 3D voxel grid for the game world.
//
// The world is stored as a flat `Vec<VoxelType>` indexed by
// `x + z * size_x + y * size_x * size_z`, giving O(1) read/write access.
// Out-of-bounds reads return `Air`; out-of-bounds writes are no-ops.
//
// Also provides `raycast_hits_solid()`, a 3D DDA (Amanatides & Woo) voxel
// traversal that tests whether any solid voxel lies between two points
// (used by `sim_bridge.rs` to filter nav nodes occluded by geometry), and
// `has_los()`, a similar DDA traversal for line-of-sight checks between
// voxel positions (used by ranged combat). `has_los` skips both origin and
// destination voxels and treats Leaf/Fruit as transparent via
// `VoxelType::blocks_los()`.
//
// The world is regenerated from seed at load time, so it skips
// serialization (`#[serde(skip)]` on `SimState.world`). The `Default`
// impl creates a zero-sized empty world; `SimState::new()` constructs
// the real one from `config.world_size`.
//
// See also: `tree_gen.rs` for populating the world with tree geometry,
// `nav.rs` for the navigation graph built on top of the voxel data,
// `sim/mod.rs` which owns the `VoxelWorld` as part of `SimState`.
//
// **Critical constraint: determinism.** All world modifications must go
// through deterministic sim logic. No concurrent mutation, no random
// access from rendering threads.

use crate::types::{VoxelCoord, VoxelType};

/// Dense 3D voxel grid.
#[derive(Clone, Debug, Default)]
pub struct VoxelWorld {
    /// Flat storage: index = x + z * size_x + y * size_x * size_z.
    voxels: Vec<VoxelType>,
    pub size_x: u32,
    pub size_y: u32,
    pub size_z: u32,
    /// Coordinates modified since the last drain. Used by the mesh cache to
    /// know which chunks need regeneration. Not serialized (the world is
    /// `#[serde(skip)]` on SimState and rebuilt from scratch on load, at which
    /// point the mesh cache does a full rebuild anyway).
    dirty_voxels: Vec<VoxelCoord>,
}

impl VoxelWorld {
    /// Create a new world filled with `Air`.
    pub fn new(size_x: u32, size_y: u32, size_z: u32) -> Self {
        let total = (size_x as usize) * (size_y as usize) * (size_z as usize);
        Self {
            voxels: vec![VoxelType::Air; total],
            size_x,
            size_y,
            size_z,
            dirty_voxels: Vec::new(),
        }
    }

    /// Check whether a coordinate is within bounds.
    pub fn in_bounds(&self, coord: VoxelCoord) -> bool {
        coord.x >= 0
            && coord.y >= 0
            && coord.z >= 0
            && (coord.x as u32) < self.size_x
            && (coord.y as u32) < self.size_y
            && (coord.z as u32) < self.size_z
    }

    /// Convert a coordinate to a flat index. Returns `None` if out of bounds.
    fn index(&self, coord: VoxelCoord) -> Option<usize> {
        if self.in_bounds(coord) {
            let x = coord.x as usize;
            let y = coord.y as usize;
            let z = coord.z as usize;
            let sx = self.size_x as usize;
            let sz = self.size_z as usize;
            Some(x + z * sx + y * sx * sz)
        } else {
            None
        }
    }

    /// Read a voxel. Returns `Air` for out-of-bounds coordinates.
    pub fn get(&self, coord: VoxelCoord) -> VoxelType {
        self.index(coord)
            .map(|i| self.voxels[i])
            .unwrap_or(VoxelType::Air)
    }

    /// Write a voxel. No-op for out-of-bounds coordinates. Appends the
    /// coordinate to `dirty_voxels` so the mesh cache knows which chunks
    /// need regeneration.
    pub fn set(&mut self, coord: VoxelCoord, voxel: VoxelType) {
        if let Some(i) = self.index(coord) {
            self.voxels[i] = voxel;
            self.dirty_voxels.push(coord);
        }
    }

    /// Drain all dirty voxel coordinates accumulated since the last drain.
    /// Returns the list and clears the internal buffer.
    pub fn drain_dirty_voxels(&mut self) -> Vec<VoxelCoord> {
        std::mem::take(&mut self.dirty_voxels)
    }

    /// Discard all accumulated dirty voxel coordinates without returning them.
    /// Called after world rebuild (tree generation / save load) where the mesh
    /// cache will do a full rebuild anyway, so the dirty entries are not needed.
    pub fn clear_dirty_voxels(&mut self) {
        self.dirty_voxels.clear();
    }

    /// Returns `true` if any of the 6 face-adjacent voxels (±x, ±y, ±z) is solid.
    ///
    /// Out-of-bounds neighbors return Air (from `get()`), so boundary coords
    /// are handled correctly without special cases.
    pub fn has_solid_face_neighbor(&self, coord: VoxelCoord) -> bool {
        const FACE_OFFSETS: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
            self.get(VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz))
                .is_solid()
        })
    }

    /// Returns `true` if any of the 6 face-adjacent voxels is the given type.
    pub fn has_face_neighbor_of_type(&self, coord: VoxelCoord, voxel_type: VoxelType) -> bool {
        const FACE_OFFSETS: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];
        FACE_OFFSETS.iter().any(|&(dx, dy, dz)| {
            self.get(VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz)) == voxel_type
        })
    }

    /// 3D DDA raycast: returns `true` if any solid (non-Air) voxel lies on the
    /// line segment from `from` to `to` (both in world-space floats).
    ///
    /// Uses the Amanatides & Woo voxel traversal algorithm. Stops early when a
    /// solid voxel is found or the ray leaves the grid. The destination voxel
    /// itself is NOT tested (a nav node sitting on a surface should not
    /// self-occlude).
    pub fn raycast_hits_solid(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        let dir = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];

        // Current voxel coordinates.
        let mut voxel = [
            from[0].floor() as i32,
            from[1].floor() as i32,
            from[2].floor() as i32,
        ];

        // Destination voxel (we stop before testing this one).
        let end_voxel = [
            to[0].floor() as i32,
            to[1].floor() as i32,
            to[2].floor() as i32,
        ];

        // Step direction (+1 or -1) and tMax/tDelta for each axis.
        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
            // If dir[axis] == 0, step/t_max/t_delta stay at 0/INF/INF — axis never advances.
        }

        // March through voxels until we reach the destination or exceed t=1.
        loop {
            // Don't test the destination voxel (nav node surface shouldn't self-occlude).
            if voxel == end_voxel {
                return false;
            }

            // Test current voxel.
            let vt = self.get(VoxelCoord::new(voxel[0], voxel[1], voxel[2]));
            if vt != VoxelType::Air {
                return true;
            }

            // Advance along the axis with the smallest t_max.
            let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            // If t_max exceeds 1.0, we've passed the destination without hitting anything.
            if t_max[min_axis] > 1.0 {
                return false;
            }

            voxel[min_axis] += step[min_axis];
            t_max[min_axis] += t_delta[min_axis];
        }
    }

    /// Returns `true` if line-of-sight exists between two voxel positions.
    /// Uses the same DDA algorithm as `raycast_hits_solid`, but only blocks
    /// on voxels where `VoxelType::blocks_los()` is true (Leaf and Fruit are
    /// transparent). Neither the origin nor destination voxel self-occlude.
    ///
    /// For multi-voxel targets, the caller should check LOS to each occupied
    /// voxel and succeed if any ray is clear.
    pub fn has_los(&self, from: VoxelCoord, to: VoxelCoord) -> bool {
        if from == to {
            return true;
        }

        let from_f = [
            from.x as f32 + 0.5,
            from.y as f32 + 0.5,
            from.z as f32 + 0.5,
        ];
        let to_f = [to.x as f32 + 0.5, to.y as f32 + 0.5, to.z as f32 + 0.5];
        let dir = [
            to_f[0] - from_f[0],
            to_f[1] - from_f[1],
            to_f[2] - from_f[2],
        ];

        let mut voxel = [from.x, from.y, from.z];
        let end_voxel = [to.x, to.y, to.z];

        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];

        for axis in 0..3 {
            if dir[axis] > 0.0 {
                step[axis] = 1;
                t_delta[axis] = 1.0 / dir[axis];
                t_max[axis] = ((voxel[axis] as f32 + 1.0) - from_f[axis]) / dir[axis];
            } else if dir[axis] < 0.0 {
                step[axis] = -1;
                t_delta[axis] = 1.0 / (-dir[axis]);
                t_max[axis] = (from_f[axis] - voxel[axis] as f32) / (-dir[axis]);
            }
        }

        // Skip the origin voxel — advance once before checking.
        let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };
        if t_max[min_axis] > 1.0 {
            return true; // Adjacent voxels, nothing between them.
        }
        voxel[min_axis] += step[min_axis];
        t_max[min_axis] += t_delta[min_axis];

        loop {
            if voxel == end_voxel {
                return true; // Reached destination without obstruction.
            }

            let vt = self.get(VoxelCoord::new(voxel[0], voxel[1], voxel[2]));
            if vt.blocks_los() {
                return false;
            }

            let min_axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };

            if t_max[min_axis] > 1.0 {
                return true;
            }

            voxel[min_axis] += step[min_axis];
            t_max[min_axis] += t_delta[min_axis];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_world_is_all_air() {
        let world = VoxelWorld::new(4, 4, 4);
        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    assert_eq!(world.get(VoxelCoord::new(x, y, z)), VoxelType::Air);
                }
            }
        }
    }

    #[test]
    fn set_and_get() {
        let mut world = VoxelWorld::new(8, 8, 8);
        let coord = VoxelCoord::new(3, 5, 2);
        world.set(coord, VoxelType::Trunk);
        assert_eq!(world.get(coord), VoxelType::Trunk);
        // Neighbors are still air.
        assert_eq!(world.get(VoxelCoord::new(3, 5, 3)), VoxelType::Air);
    }

    #[test]
    fn out_of_bounds_read_returns_air() {
        let world = VoxelWorld::new(4, 4, 4);
        assert_eq!(world.get(VoxelCoord::new(-1, 0, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, -1, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(4, 0, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(0, 4, 0)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(100, 100, 100)), VoxelType::Air);
    }

    #[test]
    fn out_of_bounds_write_is_noop() {
        let mut world = VoxelWorld::new(4, 4, 4);
        // Should not panic.
        world.set(VoxelCoord::new(-1, 0, 0), VoxelType::Trunk);
        world.set(VoxelCoord::new(100, 0, 0), VoxelType::Trunk);
    }

    #[test]
    fn default_world_is_empty() {
        let world = VoxelWorld::default();
        assert_eq!(world.size_x, 0);
        assert_eq!(world.size_y, 0);
        assert_eq!(world.size_z, 0);
        // Out-of-bounds read on empty world should still return Air.
        assert_eq!(world.get(VoxelCoord::new(0, 0, 0)), VoxelType::Air);
    }

    #[test]
    fn raycast_hits_solid_voxel() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(8, 4, 8), VoxelType::Trunk);

        // Ray from outside, through the solid voxel, to the other side.
        assert!(world.raycast_hits_solid([0.5, 4.5, 8.5], [15.5, 4.5, 8.5]));
        // Ray that doesn't pass through any solid voxel.
        assert!(!world.raycast_hits_solid([0.5, 0.5, 0.5], [15.5, 0.5, 0.5]));
    }

    #[test]
    fn raycast_does_not_self_occlude_destination() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // Place a solid voxel at the destination — should not count as occluded.
        world.set(VoxelCoord::new(8, 4, 8), VoxelType::Trunk);
        assert!(!world.raycast_hits_solid([0.5, 4.5, 0.5], [8.5, 4.5, 8.5]));
    }

    #[test]
    fn raycast_blocked_before_destination() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // Blocker in the middle, destination beyond it.
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Trunk);
        assert!(world.raycast_hits_solid([0.5, 4.5, 8.5], [10.5, 4.5, 8.5]));
    }

    #[test]
    fn indexing_is_correct() {
        // Verify the specific indexing scheme: x + z * size_x + y * size_x * size_z
        let mut world = VoxelWorld::new(10, 8, 6);
        // Set a voxel and verify only that exact coord is affected.
        let coord = VoxelCoord::new(5, 3, 4);
        world.set(coord, VoxelType::Branch);
        assert_eq!(world.get(coord), VoxelType::Branch);
        // Adjacent coords should still be air.
        assert_eq!(world.get(VoxelCoord::new(4, 3, 4)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(5, 2, 4)), VoxelType::Air);
        assert_eq!(world.get(VoxelCoord::new(5, 3, 3)), VoxelType::Air);
    }

    #[test]
    fn set_tracks_dirty_voxels() {
        let mut world = VoxelWorld::new(8, 8, 8);
        assert!(world.drain_dirty_voxels().is_empty());

        world.set(VoxelCoord::new(1, 2, 3), VoxelType::Trunk);
        world.set(VoxelCoord::new(4, 5, 6), VoxelType::Branch);
        let dirty = world.drain_dirty_voxels();
        assert_eq!(dirty.len(), 2);
        assert_eq!(dirty[0], VoxelCoord::new(1, 2, 3));
        assert_eq!(dirty[1], VoxelCoord::new(4, 5, 6));
        // Second drain is empty.
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn clear_dirty_voxels_discards_entries() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(1, 2, 3), VoxelType::Trunk);
        assert!(!world.drain_dirty_voxels().is_empty());

        world.set(VoxelCoord::new(4, 5, 6), VoxelType::Branch);
        world.clear_dirty_voxels();
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn out_of_bounds_set_does_not_dirty() {
        let mut world = VoxelWorld::new(4, 4, 4);
        world.set(VoxelCoord::new(-1, 0, 0), VoxelType::Trunk);
        world.set(VoxelCoord::new(100, 0, 0), VoxelType::Trunk);
        assert!(world.drain_dirty_voxels().is_empty());
    }

    #[test]
    fn has_solid_face_neighbor_true_when_adjacent() {
        let mut world = VoxelWorld::new(8, 8, 8);
        world.set(VoxelCoord::new(4, 3, 4), VoxelType::Trunk);
        // Air voxel directly above the trunk.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(4, 4, 4)));
        // Air voxel to the +x side.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(5, 3, 4)));
        // Air voxel to the -z side.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(4, 3, 3)));
    }

    #[test]
    fn has_solid_face_neighbor_false_when_isolated() {
        let world = VoxelWorld::new(8, 8, 8);
        // All-air world — no face neighbor is solid.
        assert!(!world.has_solid_face_neighbor(VoxelCoord::new(4, 4, 4)));
    }

    #[test]
    fn has_solid_face_neighbor_at_boundary() {
        let mut world = VoxelWorld::new(8, 8, 8);
        // Place solid at the edge of the world.
        world.set(VoxelCoord::new(0, 0, 0), VoxelType::ForestFloor);
        // Neighbor at (1,0,0) should detect the solid.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(1, 0, 0)));
        // Out-of-bounds neighbors return Air, so (-1,0,0) has no solid neighbor
        // besides (0,0,0) itself.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(0, 1, 0)));
        // Coord at (-1,0,0) is OOB; its neighbors include (0,0,0) which is solid.
        assert!(world.has_solid_face_neighbor(VoxelCoord::new(-1, 0, 0)));
    }

    // -- has_los tests --

    #[test]
    fn los_clear_path() {
        let world = VoxelWorld::new(16, 16, 16);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
        assert!(world.has_los(b, a)); // symmetry
    }

    #[test]
    fn los_same_voxel() {
        let world = VoxelWorld::new(8, 8, 8);
        let v = VoxelCoord::new(3, 3, 3);
        assert!(world.has_los(v, v));
    }

    #[test]
    fn los_blocked_by_trunk() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Trunk);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(!world.has_los(a, b));
    }

    #[test]
    fn los_leaf_transparent() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Leaf);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_fruit_transparent() {
        let mut world = VoxelWorld::new(16, 16, 16);
        world.set(VoxelCoord::new(5, 4, 8), VoxelType::Fruit);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_origin_and_dest_not_self_occluding() {
        let mut world = VoxelWorld::new(16, 16, 16);
        // Even if the destination voxel is solid, it shouldn't block LOS.
        world.set(VoxelCoord::new(10, 4, 8), VoxelType::Trunk);
        let a = VoxelCoord::new(2, 4, 8);
        let b = VoxelCoord::new(10, 4, 8);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_adjacent_voxels() {
        let mut world = VoxelWorld::new(8, 8, 8);
        // Adjacent voxels should always have LOS.
        let a = VoxelCoord::new(3, 3, 3);
        let b = VoxelCoord::new(4, 3, 3);
        assert!(world.has_los(a, b));
        // Even diagonally adjacent.
        let c = VoxelCoord::new(4, 4, 4);
        assert!(world.has_los(a, c));

        // Still clear even with solid at destination.
        world.set(VoxelCoord::new(4, 3, 3), VoxelType::Trunk);
        assert!(world.has_los(a, b));
    }

    #[test]
    fn los_diagonal_path() {
        let mut world = VoxelWorld::new(16, 16, 16);
        let a = VoxelCoord::new(2, 4, 2);
        let b = VoxelCoord::new(10, 4, 10);
        assert!(world.has_los(a, b));

        // Block a voxel along the diagonal.
        world.set(VoxelCoord::new(6, 4, 6), VoxelType::Branch);
        assert!(!world.has_los(a, b));
    }
}
