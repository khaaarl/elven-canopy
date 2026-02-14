// Dense 3D voxel grid for the game world.
//
// The world is stored as a flat `Vec<VoxelType>` indexed by
// `x + z * size_x + y * size_x * size_z`, giving O(1) read/write access.
// Out-of-bounds reads return `Air`; out-of-bounds writes are no-ops.
//
// The world is regenerated from seed at load time, so it skips
// serialization (`#[serde(skip)]` on `SimState.world`). The `Default`
// impl creates a zero-sized empty world; `SimState::new()` constructs
// the real one from `config.world_size`.
//
// See also: `tree_gen.rs` for populating the world with tree geometry,
// `nav.rs` for the navigation graph built on top of the voxel data,
// `sim.rs` which owns the `VoxelWorld` as part of `SimState`.
//
// **Critical constraint: determinism.** All world modifications must go
// through deterministic sim logic. No concurrent mutation, no random
// access from rendering threads.

use crate::types::{VoxelCoord, VoxelType};

/// Dense 3D voxel grid.
#[derive(Clone, Debug)]
pub struct VoxelWorld {
    /// Flat storage: index = x + z * size_x + y * size_x * size_z.
    voxels: Vec<VoxelType>,
    pub size_x: u32,
    pub size_y: u32,
    pub size_z: u32,
}

impl Default for VoxelWorld {
    fn default() -> Self {
        Self {
            voxels: Vec::new(),
            size_x: 0,
            size_y: 0,
            size_z: 0,
        }
    }
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

    /// Write a voxel. No-op for out-of-bounds coordinates.
    pub fn set(&mut self, coord: VoxelCoord, voxel: VoxelType) {
        if let Some(i) = self.index(coord) {
            self.voxels[i] = voxel;
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
}
