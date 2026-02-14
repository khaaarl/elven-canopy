// Procedural tree generation.
//
// Generates a tree's geometry — trunk cylinder and axis-aligned branches —
// populating both the `VoxelWorld` grid and the `Tree` entity's voxel lists.
// The generation is fully deterministic given a `GameRng` state, using only
// integer arithmetic for voxel placement (cylinder test: `dx² + dz² ≤ r²`).
//
// Called during `SimState::new()` to build the initial world. Future phases
// may call it again for tree growth events.
//
// See also: `world.rs` for the voxel grid being populated, `nav.rs` for the
// navigation graph built on top of the generated geometry, `config.rs` for
// tree generation parameters, `sim.rs` which calls `generate_tree()`.
//
// **Critical constraint: determinism.** All randomness comes from the
// `GameRng` passed by the caller. Integer arithmetic only for voxel placement.

use crate::config::GameConfig;
use crate::prng::GameRng;
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

/// Result of tree generation — voxel lists to store on the Tree entity.
pub struct TreeGenResult {
    pub trunk_voxels: Vec<VoxelCoord>,
    pub branch_voxels: Vec<VoxelCoord>,
}

/// Generate a tree at the world center, populating the voxel world and
/// returning the voxel lists for the Tree entity.
///
/// Also fills `ForestFloor` at y=0 for the walkable ground plane.
pub fn generate_tree(
    world: &mut VoxelWorld,
    config: &GameConfig,
    rng: &mut GameRng,
) -> TreeGenResult {
    let mut trunk_voxels = Vec::new();
    let mut branch_voxels = Vec::new();

    let center_x = world.size_x as i32 / 2;
    let center_z = world.size_z as i32 / 2;
    let trunk_radius = config.tree_trunk_radius as i32;
    let trunk_height = config.tree_trunk_height;

    // --- Forest floor at y=0 ---
    // Fill a generous area around the tree base.
    let floor_extent = trunk_radius + 20;
    for dx in -floor_extent..=floor_extent {
        for dz in -floor_extent..=floor_extent {
            let coord = VoxelCoord::new(center_x + dx, 0, center_z + dz);
            world.set(coord, VoxelType::ForestFloor);
        }
    }

    // --- Trunk: cylinder ---
    for y in 1..=trunk_height as i32 {
        // Slight radius jitter for organic feel: ±1 per y-level.
        let jitter = (rng.next_u64() % 3) as i32 - 1; // -1, 0, or +1
        let r = (trunk_radius + jitter).max(1);
        let r_sq = r * r;

        for dx in -r..=r {
            for dz in -r..=r {
                if dx * dx + dz * dz <= r_sq {
                    let coord = VoxelCoord::new(center_x + dx, y, center_z + dz);
                    world.set(coord, VoxelType::Trunk);
                    trunk_voxels.push(coord);
                }
            }
        }
    }

    // --- Branches ---
    // Directions: +X, +Z, -X, -Z (axis-aligned, rotate around trunk).
    let directions: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

    let branch_start_y = config.tree_branch_start_y;
    let branch_interval = config.tree_branch_interval;
    let branch_count = config.tree_branch_count;
    let branch_length = config.tree_branch_length as i32;
    let branch_radius = config.tree_branch_radius as i32;

    // Starting direction index — rotated per branch + PRNG jitter.
    let mut dir_idx = rng.next_u64() as usize % 4;

    for i in 0..branch_count {
        let branch_y = branch_start_y + i * branch_interval;
        if branch_y > trunk_height {
            break;
        }

        // Jitter direction: sometimes skip an extra rotation.
        let jitter_rot = (rng.next_u64() % 2) as usize;
        dir_idx = (dir_idx + 1 + jitter_rot) % 4;
        let (dx_dir, dz_dir) = directions[dir_idx];

        // Branch extends outward from trunk surface.
        for step in 0..=branch_length {
            let bx = center_x + dx_dir * (trunk_radius + 1 + step);
            let bz = center_z + dz_dir * (trunk_radius + 1 + step);
            let by = branch_y as i32;

            // Branch cross-section: fill a small cylinder perpendicular to the branch direction.
            let br_sq = branch_radius * branch_radius;
            if dx_dir != 0 {
                // Branch extends along X — cross-section in Y,Z plane.
                for dy in -branch_radius..=branch_radius {
                    for ddz in -branch_radius..=branch_radius {
                        if dy * dy + ddz * ddz <= br_sq {
                            let coord = VoxelCoord::new(bx, by + dy, bz + ddz);
                            // Don't overwrite trunk voxels with branch.
                            if world.get(coord) != VoxelType::Trunk {
                                world.set(coord, VoxelType::Branch);
                                branch_voxels.push(coord);
                            }
                        }
                    }
                }
            } else {
                // Branch extends along Z — cross-section in X,Y plane.
                for dy in -branch_radius..=branch_radius {
                    for ddx in -branch_radius..=branch_radius {
                        if dy * dy + ddx * ddx <= br_sq {
                            let coord = VoxelCoord::new(bx + ddx, by + dy, bz);
                            if world.get(coord) != VoxelType::Trunk {
                                world.set(coord, VoxelType::Branch);
                                branch_voxels.push(coord);
                            }
                        }
                    }
                }
            }
        }
    }

    TreeGenResult {
        trunk_voxels,
        branch_voxels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> GameConfig {
        GameConfig {
            world_size: (64, 64, 64),
            tree_trunk_radius: 3,
            tree_trunk_height: 30,
            tree_branch_start_y: 10,
            tree_branch_interval: 5,
            tree_branch_count: 4,
            tree_branch_length: 6,
            tree_branch_radius: 1,
            ..GameConfig::default()
        }
    }

    #[test]
    fn generates_trunk_voxels() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(!result.trunk_voxels.is_empty());
        // All trunk voxels should be Trunk type in the world.
        for coord in &result.trunk_voxels {
            assert_eq!(world.get(*coord), VoxelType::Trunk);
        }
    }

    #[test]
    fn generates_branch_voxels() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(!result.branch_voxels.is_empty());
        for coord in &result.branch_voxels {
            assert_eq!(world.get(*coord), VoxelType::Branch);
        }
    }

    #[test]
    fn generates_forest_floor() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_tree(&mut world, &config, &mut rng);

        // Center of the world at y=0 should be ForestFloor.
        let center = VoxelCoord::new(32, 0, 32);
        assert_eq!(world.get(center), VoxelType::ForestFloor);
    }

    #[test]
    fn deterministic_generation() {
        let config = test_config();

        let mut world_a = VoxelWorld::new(64, 64, 64);
        let mut rng_a = GameRng::new(42);
        let result_a = generate_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(42);
        let result_b = generate_tree(&mut world_b, &config, &mut rng_b);

        assert_eq!(result_a.trunk_voxels, result_b.trunk_voxels);
        assert_eq!(result_a.branch_voxels, result_b.branch_voxels);
    }

    #[test]
    fn trunk_is_cylindrical() {
        let config = GameConfig {
            world_size: (64, 64, 64),
            tree_trunk_radius: 3,
            tree_trunk_height: 5,
            tree_branch_count: 0, // No branches for this test.
            ..GameConfig::default()
        };
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(0);
        let result = generate_tree(&mut world, &config, &mut rng);

        let center_x = 32;
        let center_z = 32;

        // All trunk voxels should be within radius + jitter of center.
        for coord in &result.trunk_voxels {
            let dx = coord.x - center_x;
            let dz = coord.z - center_z;
            let dist_sq = dx * dx + dz * dz;
            // Radius is 3, jitter is ±1, so max effective radius is 4.
            assert!(dist_sq <= 4 * 4, "Trunk voxel too far from center: {coord:?}");
        }
    }
}
