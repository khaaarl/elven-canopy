// Procedural tree generation.
//
// Generates a tree's geometry — smooth trunk cylinder, stochastically curving
// branches with recursive forking, and leaf blobs at branch tips — populating
// both the `VoxelWorld` grid and the `Tree` entity's voxel lists. The trunk is
// a uniform cylinder with no per-level jitter. Branches emerge from the trunk
// surface at evenly-spaced angles (with small random jitter) and taper
// linearly from full radius at the trunk to radius 1 at the tip.
//
// Each branch grows outward step-by-step using a float cursor walk (the
// `grow_branch()` function). The branch angle determines both the outward
// direction (cos/sin) and the lateral axis (-sin/cos). The cursor advances
// ~1 voxel outward each step, with stochastic vertical movement and lateral
// jitter for natural curves. Cross-sections are oriented circles
// perpendicular to the branch direction. The centerline cursor positions are
// recorded in `branch_paths` for use by `nav.rs`.
//
// **Branch forking:** At each eligible step (after `fork_min_step`), a fork
// roll determines whether a sub-branch spawns. Sub-branches grow at an angle
// offset from the parent, with reduced length and radius. Forks are processed
// via an iterative `VecDeque<BranchJob>` work queue (FIFO order: primaries
// first, then depth-1 forks, then depth-2, etc.) to avoid recursion and
// maintain deterministic RNG ordering. The `branch_parents` vec tracks
// parent-child relationships for nav graph construction.
//
// **Leaf blobs:** After all branches are grown, `generate_leaf_blobs()` places
// semi-spherical clusters of `VoxelType::Leaf` voxels at branch tips and
// (optionally) along the outer portion of branches. Density, radius, spacing,
// and coverage are configurable. Leaves never overwrite Trunk or Branch voxels.
// Leaf RNG draws happen in a separate pass after all branch draws, so existing
// branch-only tests remain unaffected.
//
// Called during `SimState::new()` to build the initial world. Future phases
// may call it again for tree growth events.
//
// See also: `world.rs` for the voxel grid being populated, `nav.rs` for the
// navigation graph built on top of the generated geometry, `config.rs` for
// tree generation parameters (including 6 fork params and 5 leaf params),
// `sim.rs` which calls `generate_tree()`.
//
// **Critical constraint: determinism.** All randomness comes from the
// `GameRng` passed by the caller. Float cursor positions are rounded to
// integers for voxel placement. `f32::cos()`/`sin()` are IEEE 754 and
// produce identical results across platforms. Fork rolls are always drawn
// for eligible steps when `fork_max_depth > 0`, regardless of the current
// branch's depth, to keep RNG draw counts consistent. Leaf generation always
// draws `rng.next_f32()` for every in-sphere voxel to maintain deterministic
// RNG ordering.

use crate::config::GameConfig;
use crate::prng::GameRng;
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

use serde::{Deserialize, Serialize};

/// Tracks a sub-branch's parent relationship.
///
/// Primary branches (grown directly from the trunk) have `None` for their
/// parent. Sub-branches spawned via forking store the index of their parent
/// path in `branch_paths` and the step along that path where the fork occurs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BranchParent {
    /// Index into `branch_paths` for the parent branch.
    pub parent_path_idx: usize,
    /// Step along the parent's path where this fork originates.
    pub fork_step: usize,
}

/// Result of tree generation — voxel lists to store on the Tree entity.
pub struct TreeGenResult {
    pub trunk_voxels: Vec<VoxelCoord>,
    pub branch_voxels: Vec<VoxelCoord>,
    /// Centerline cursor positions for each branch (one path per branch).
    /// Sub-branches are additional entries appended after primary branches.
    pub branch_paths: Vec<Vec<VoxelCoord>>,
    /// Parent relationship for each branch path. `None` = primary branch from
    /// trunk, `Some(BranchParent)` = sub-branch forked from a parent.
    /// Length always matches `branch_paths`.
    pub branch_parents: Vec<Option<BranchParent>>,
    /// Leaf voxel positions placed as semi-spherical blobs at/near branch tips.
    pub leaf_voxels: Vec<VoxelCoord>,
}

/// A pending branch growth job for the iterative work queue.
///
/// Primary branches start from the trunk surface; sub-branches start from
/// a fork point on their parent. The queue processes jobs in FIFO order
/// (primary branches first, then depth-1 forks, then depth-2, etc.) which
/// keeps RNG ordering deterministic.
struct BranchJob {
    /// Starting cursor X (float, for sub-voxel positioning).
    start_x: f32,
    /// Starting cursor Y (integer voxel level).
    start_y: i32,
    /// Starting cursor Z (float).
    start_z: f32,
    /// Outward angle in radians.
    angle: f32,
    /// Number of steps to grow (determines branch length).
    length: i32,
    /// Cross-section radius at the branch base.
    radius: i32,
    /// Fork generation depth (0 = primary branch from trunk).
    depth: u32,
    /// Parent relationship for sub-branches, `None` for primary branches.
    parent: Option<BranchParent>,
}

/// Grow a single branch from the given job, placing voxels into the world
/// and recording the centerline path. Returns the path and any sub-branch
/// fork jobs spawned during growth.
///
/// The fork roll is drawn once per eligible step (step >= `fork_min_step`
/// and not the final step) whenever `fork_max_depth > 0`, regardless of
/// the current branch's depth. This keeps the RNG draw count per step
/// consistent across branches of different depths, preserving determinism.
fn grow_branch(
    job: &BranchJob,
    path_idx: usize,
    world: &mut VoxelWorld,
    config: &GameConfig,
    rng: &mut GameRng,
    branch_voxels: &mut Vec<VoxelCoord>,
) -> (Vec<VoxelCoord>, Vec<BranchJob>) {
    let dir_x = job.angle.cos();
    let dir_z = job.angle.sin();
    let lat_x = -job.angle.sin();
    let lat_z = job.angle.cos();

    let mut cursor_x = job.start_x;
    let mut cursor_y = job.start_y;
    let mut cursor_z = job.start_z;

    let branch_length = job.length;
    let branch_radius = job.radius;

    let mut path = Vec::with_capacity(branch_length as usize + 1);
    let mut fork_jobs = Vec::new();

    let fork_enabled = config.tree_branch_fork_max_depth > 0;
    let fork_min_step = config.tree_branch_fork_min_step as i32;

    for step in 0..=branch_length {
        let vx = cursor_x.round() as i32;
        let vz = cursor_z.round() as i32;
        path.push(VoxelCoord::new(vx, cursor_y, vz));

        // Linearly taper: full radius at base (step 0), radius 1 at tip.
        let effective_r = if branch_radius > 1 && branch_length > 0 {
            let r = 1 + ((branch_radius - 1) * (branch_length - step)
                + branch_length / 2) / branch_length;
            r.max(1)
        } else {
            branch_radius
        };

        // Branch cross-section: oriented circle perpendicular to the
        // branch direction. `dl` offsets along the lateral axis.
        let er_sq = effective_r * effective_r;
        for dy in -effective_r..=effective_r {
            for dl in -effective_r..=effective_r {
                if dy * dy + dl * dl <= er_sq {
                    let cx = (cursor_x + dl as f32 * lat_x).round() as i32;
                    let cy = cursor_y + dy;
                    let cz = (cursor_z + dl as f32 * lat_z).round() as i32;
                    let coord = VoxelCoord::new(cx, cy, cz);
                    if world.get(coord) != VoxelType::Trunk {
                        world.set(coord, VoxelType::Branch);
                        branch_voxels.push(coord);
                    }
                }
            }
        }

        // Fork check: always draw the fork roll for eligible steps when
        // forking is globally enabled, to maintain consistent RNG ordering.
        if fork_enabled && step >= fork_min_step && step < branch_length {
            let fork_roll = rng.next_f32();
            if job.depth < config.tree_branch_fork_max_depth
                && fork_roll < config.tree_branch_fork_chance
            {
                // Fork succeeds — pick left or right, compute sub-branch params.
                let sign_roll = rng.next_f32();
                let sign = if sign_roll < 0.5 { 1.0_f32 } else { -1.0_f32 };
                let fork_angle = job.angle + sign * config.tree_branch_fork_angle;

                let remaining = branch_length - step;
                let sub_length =
                    ((remaining as f32 * config.tree_branch_fork_length_ratio) as i32).max(2);
                let sub_radius =
                    ((effective_r as f32 * config.tree_branch_fork_radius_ratio) as i32).max(1);

                fork_jobs.push(BranchJob {
                    start_x: cursor_x,
                    start_y: cursor_y,
                    start_z: cursor_z,
                    angle: fork_angle,
                    length: sub_length,
                    radius: sub_radius,
                    depth: job.depth + 1,
                    parent: Some(BranchParent {
                        parent_path_idx: path_idx,
                        fork_step: step as usize,
                    }),
                });
            }
        }

        // Advance cursor for the next step (skip on the last step).
        if step < branch_length {
            // Primary direction: ~1 voxel outward.
            cursor_x += dir_x;
            cursor_z += dir_z;

            // Vertical movement: weighted random based on progress t.
            let t = step as f32 / branch_length as f32;
            let roll = rng.next_f32();
            if t < 0.4 {
                // Rise phase: P(up)=70%, P(level)=25%, P(down)=5%
                if roll < 0.70 {
                    cursor_y += 1;
                } else if roll >= 0.95 {
                    cursor_y -= 1;
                }
            } else if t < 0.75 {
                // Level phase: P(up)=15%, P(level)=75%, P(down)=10%
                if roll < 0.15 {
                    cursor_y += 1;
                } else if roll >= 0.90 {
                    cursor_y -= 1;
                }
            } else {
                // Droop phase: P(up)=5%, P(level)=55%, P(down)=40%
                if roll < 0.05 {
                    cursor_y += 1;
                } else if roll >= 0.60 {
                    cursor_y -= 1;
                }
            }

            // Lateral jitter: 7.5% chance each direction.
            let lat_roll = rng.next_f32();
            if lat_roll < 0.075 {
                cursor_x += lat_x;
                cursor_z += lat_z;
            } else if lat_roll >= 0.925 {
                cursor_x -= lat_x;
                cursor_z -= lat_z;
            }
        }
    }

    (path, fork_jobs)
}

/// Generate a tree at the world center, populating the voxel world and
/// returning the voxel lists for the Tree entity.
///
/// Also fills `ForestFloor` at y=0 for the walkable ground plane.
///
/// Branches are grown via an iterative work queue (`VecDeque<BranchJob>`).
/// Primary branches are enqueued first, then each branch may spawn fork
/// sub-branches that are appended to the queue. This avoids recursion and
/// ensures deterministic FIFO processing order.
pub fn generate_tree(
    world: &mut VoxelWorld,
    config: &GameConfig,
    rng: &mut GameRng,
) -> TreeGenResult {
    use std::collections::VecDeque;

    let mut trunk_voxels = Vec::new();
    let mut branch_voxels = Vec::new();
    let mut branch_paths: Vec<Vec<VoxelCoord>> = Vec::new();
    let mut branch_parents: Vec<Option<BranchParent>> = Vec::new();

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

    // --- Trunk: smooth cylinder ---
    let r_sq = trunk_radius * trunk_radius;
    for y in 1..=trunk_height as i32 {
        for dx in -trunk_radius..=trunk_radius {
            for dz in -trunk_radius..=trunk_radius {
                if dx * dx + dz * dz <= r_sq {
                    let coord = VoxelCoord::new(center_x + dx, y, center_z + dz);
                    world.set(coord, VoxelType::Trunk);
                    trunk_voxels.push(coord);
                }
            }
        }
    }

    // --- Branches (iterative work queue) ---
    let tau: f32 = std::f32::consts::TAU;

    let branch_start_y = config.tree_branch_start_y;
    let branch_interval = config.tree_branch_interval;
    let branch_count = config.tree_branch_count;
    let branch_length = config.tree_branch_length as i32;
    let branch_radius = config.tree_branch_radius as i32;
    let center_xf = center_x as f32;
    let center_zf = center_z as f32;
    let trunk_rf = trunk_radius as f32;

    // Enqueue primary branches.
    let mut job_queue: VecDeque<BranchJob> = VecDeque::new();
    for i in 0..branch_count {
        let branch_y = branch_start_y + i * branch_interval;
        if branch_y > trunk_height {
            break;
        }

        // Angle: evenly spaced with ±0.3 radian jitter (~±17°).
        let base_angle = i as f32 * tau / branch_count as f32;
        let jitter = rng.range_f32(-0.3, 0.3);
        let angle = base_angle + jitter;

        // Start on the trunk surface. Pull inward by 0.5 so that rounding
        // to integer voxel coordinates always lands inside the cylinder.
        let dir_x = angle.cos();
        let dir_z = angle.sin();
        let start_r = trunk_rf - 0.5;

        job_queue.push_back(BranchJob {
            start_x: center_xf + dir_x * start_r,
            start_y: branch_y as i32,
            start_z: center_zf + dir_z * start_r,
            angle,
            length: branch_length,
            radius: branch_radius,
            depth: 0,
            parent: None,
        });
    }

    // Process the work queue: grow each branch, enqueue any forks.
    while let Some(job) = job_queue.pop_front() {
        let path_idx = branch_paths.len();
        let parent = job.parent.clone();
        let (path, sub_jobs) = grow_branch(&job, path_idx, world, config, rng, &mut branch_voxels);
        branch_paths.push(path);
        branch_parents.push(parent);

        for sub_job in sub_jobs {
            job_queue.push_back(sub_job);
        }
    }

    // --- Leaf blobs (separate pass after all branches) ---
    let leaf_voxels = generate_leaf_blobs(&branch_paths, world, config, rng);

    TreeGenResult {
        trunk_voxels,
        branch_voxels,
        branch_paths,
        branch_parents,
        leaf_voxels,
    }
}

/// Generate leaf blob voxels along branch tips and (optionally) outer portions.
///
/// Runs as a separate pass **after** all branch growth is complete, so the RNG
/// draws for leaves never interleave with branch logic. For each branch path:
///
/// 1. Always place a blob at the tip (last position).
/// 2. If `!tip_only`, walk backward from the tip along the last `coverage`
///    fraction of the path, placing additional blobs every `spacing` steps.
/// 3. For each blob center, iterate a sphere of `blob_radius` and
///    stochastically place `Leaf` voxels at `density` probability, skipping
///    existing solid voxels (Trunk, Branch, Leaf).
///
/// Always draws `rng.next_f32()` for every in-sphere voxel (even if skipped)
/// to maintain deterministic RNG ordering.
fn generate_leaf_blobs(
    branch_paths: &[Vec<VoxelCoord>],
    world: &mut VoxelWorld,
    config: &GameConfig,
    rng: &mut GameRng,
) -> Vec<VoxelCoord> {
    let mut leaf_voxels = Vec::new();
    let radius = config.tree_leaf_blob_radius as i32;
    let density = config.tree_leaf_blob_density;
    let tip_only = config.tree_leaf_tip_only;
    let coverage = config.tree_leaf_branch_coverage;
    let spacing = config.tree_leaf_blob_spacing as usize;
    let r_sq = radius * radius;

    for path in branch_paths {
        if path.is_empty() {
            continue;
        }

        // Collect blob centers for this branch.
        let mut blob_centers: Vec<VoxelCoord> = Vec::new();

        // Always place a blob at the tip.
        blob_centers.push(*path.last().unwrap());

        // If not tip_only, walk backward from tip along the coverage fraction.
        if !tip_only && path.len() > 1 {
            let covered_steps = ((path.len() as f32 * coverage).ceil() as usize).min(path.len());
            let start_idx = path.len().saturating_sub(covered_steps);
            // Walk backward from tip, placing blobs every `spacing` steps.
            let mut steps_since_last = 0usize;
            for i in (start_idx..path.len().saturating_sub(1)).rev() {
                steps_since_last += 1;
                if steps_since_last >= spacing {
                    blob_centers.push(path[i]);
                    steps_since_last = 0;
                }
            }
        }

        // Place voxels for each blob center.
        for center in &blob_centers {
            for dx in -radius..=radius {
                for dy in -radius..=radius {
                    for dz in -radius..=radius {
                        if dx * dx + dy * dy + dz * dz > r_sq {
                            continue;
                        }
                        // Always draw for determinism.
                        let roll = rng.next_f32();
                        if roll >= density {
                            continue;
                        }
                        let coord = VoxelCoord::new(
                            center.x + dx,
                            center.y + dy,
                            center.z + dz,
                        );
                        let existing = world.get(coord);
                        if matches!(existing, VoxelType::Trunk | VoxelType::Branch | VoxelType::Leaf)
                        {
                            continue;
                        }
                        world.set(coord, VoxelType::Leaf);
                        leaf_voxels.push(coord);
                    }
                }
            }
        }
    }

    leaf_voxels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> GameConfig {
        GameConfig {
            world_size: (64, 64, 64),
            tree_trunk_radius: 4,
            tree_trunk_height: 30,
            tree_branch_start_y: 10,
            tree_branch_interval: 5,
            tree_branch_count: 4,
            tree_branch_length: 6,
            tree_branch_radius: 2,
            tree_branch_fork_max_depth: 0, // Disable forks in legacy tests.
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
    fn branches_taper_toward_tip() {
        let config = GameConfig {
            tree_branch_radius: 3, // Larger radius makes taper more pronounced.
            ..test_config()
        };
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // Use branch_paths[0] to get root and tip positions.
        let path = &result.branch_paths[0];
        let root_pos = path[0];
        let tip_pos = path[path.len() - 1];
        let proximity = config.tree_branch_radius as u32;

        // Count branch voxels near the root vs. near the tip (by manhattan distance).
        let mut voxels_near_root = 0u32;
        let mut voxels_near_tip = 0u32;
        for v in &result.branch_voxels {
            if v.manhattan_distance(root_pos) <= proximity {
                voxels_near_root += 1;
            }
            if v.manhattan_distance(tip_pos) <= proximity {
                voxels_near_tip += 1;
            }
        }

        assert!(
            voxels_near_root > voxels_near_tip,
            "Branch should taper: near-root cross-section ({voxels_near_root}) \
             should be larger than tip ({voxels_near_tip})"
        );
    }

    #[test]
    fn branches_spread_around_trunk() {
        // With 4 branches at ~90° spacing + jitter, each branch should end
        // up in a distinct quadrant (classify by sign of root-to-tip delta).
        let config = GameConfig {
            world_size: (64, 64, 64),
            tree_trunk_radius: 4,
            tree_trunk_height: 40,
            tree_branch_start_y: 10,
            tree_branch_interval: 5,
            tree_branch_count: 4,
            tree_branch_length: 6,
            tree_branch_radius: 2,
            tree_branch_fork_max_depth: 0,
            ..GameConfig::default()
        };
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // Classify each branch into a quadrant by root-to-tip delta.
        let mut quadrants_seen = std::collections::BTreeSet::new();
        for path in &result.branch_paths {
            let root = path[0];
            let tip = path[path.len() - 1];
            let dx = tip.x - root.x;
            let dz = tip.z - root.z;
            // Quadrant by sign of dominant axis.
            let quadrant = if dx.abs() > dz.abs() {
                if dx > 0 { "+X" } else { "-X" }
            } else if dz > 0 {
                "+Z"
            } else {
                "-Z"
            };
            quadrants_seen.insert(quadrant);
        }

        assert_eq!(
            quadrants_seen.len(),
            4,
            "Expected branches in all 4 quadrants, got: {quadrants_seen:?}"
        );
    }

    #[test]
    fn branch_paths_populated() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert_eq!(
            result.branch_paths.len(),
            config.tree_branch_count as usize,
            "Should have one path per branch"
        );
        for (i, path) in result.branch_paths.iter().enumerate() {
            assert_eq!(
                path.len(),
                config.tree_branch_length as usize + 1,
                "Branch {i} path should have branch_length + 1 positions"
            );
        }
    }

    #[test]
    fn branches_curve_upward() {
        // Use longer branches so the rise phase (t < 0.4) has enough steps
        // to reliably push the midpoint above the root.
        let config = GameConfig {
            world_size: (128, 128, 128),
            tree_trunk_radius: 4,
            tree_trunk_height: 60,
            tree_branch_start_y: 20,
            tree_branch_interval: 10,
            tree_branch_count: 4,
            tree_branch_length: 20,
            tree_branch_radius: 2,
            tree_branch_fork_max_depth: 0,
            ..GameConfig::default()
        };
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // For each branch, the midpoint of the path should be above the root Y.
        for (i, path) in result.branch_paths.iter().enumerate() {
            let root_y = path[0].y;
            let mid_idx = path.len() / 2;
            let mid_y = path[mid_idx].y;
            assert!(
                mid_y > root_y,
                "Branch {i}: midpoint Y ({mid_y}) should be above root Y ({root_y})"
            );
        }
    }

    #[test]
    fn branches_vary_in_y() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // At least one branch should have non-flat Y variation (proves RNG is used).
        let has_y_variation = result.branch_paths.iter().any(|path| {
            let first_y = path[0].y;
            path.iter().any(|coord| coord.y != first_y)
        });
        assert!(
            has_y_variation,
            "At least one branch should have Y variation in its path"
        );
    }

    #[test]
    fn branch_roots_touch_trunk() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // Every branch's root (path[0]) should be on the trunk surface —
        // i.e. the voxel at path[0] is itself a trunk voxel. The branch
        // starts IN the wood, not floating outside it.
        for (i, path) in result.branch_paths.iter().enumerate() {
            let root = path[0];
            let on_trunk = result.trunk_voxels.contains(&root);
            assert!(
                on_trunk,
                "Branch {i} root {root} is not on the trunk surface"
            );
        }
    }

    #[test]
    fn trunk_is_cylindrical() {
        let config = GameConfig {
            world_size: (64, 64, 64),
            tree_trunk_radius: 4,
            tree_trunk_height: 5,
            tree_branch_count: 0, // No branches for this test.
            ..GameConfig::default()
        };
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(0);
        let result = generate_tree(&mut world, &config, &mut rng);

        let center_x = 32;
        let center_z = 32;
        let r = config.tree_trunk_radius as i32;
        let r_sq = r * r;

        // Every trunk voxel must be within the exact configured radius.
        // No jitter tolerance — the trunk should be a smooth cylinder.
        for coord in &result.trunk_voxels {
            let dx = coord.x - center_x;
            let dz = coord.z - center_z;
            let dist_sq = dx * dx + dz * dz;
            assert!(dist_sq <= r_sq, "Trunk voxel outside radius {r}: {coord:?}");
        }

        // Every y-level should have the same voxel count (uniform cylinder).
        let mut counts_per_y = std::collections::BTreeMap::new();
        for coord in &result.trunk_voxels {
            *counts_per_y.entry(coord.y).or_insert(0u32) += 1;
        }
        let expected_count = *counts_per_y.values().next().unwrap();
        for (&y, &count) in &counts_per_y {
            assert_eq!(count, expected_count, "Y-level {y} has {count} voxels, expected {expected_count}");
        }
    }

    // --- Fork tests ---

    /// Helper config for fork tests: longer branches to give forks room to grow.
    fn fork_test_config() -> GameConfig {
        GameConfig {
            world_size: (128, 128, 128),
            tree_trunk_radius: 4,
            tree_trunk_height: 60,
            tree_branch_start_y: 20,
            tree_branch_interval: 10,
            tree_branch_count: 4,
            tree_branch_length: 15,
            tree_branch_radius: 3,
            ..GameConfig::default()
        }
    }

    #[test]
    fn branch_parents_none_for_primary_branches() {
        // With fork_max_depth=0, all branches are primary (no forks).
        let config = GameConfig {
            tree_branch_fork_max_depth: 0,
            ..fork_test_config()
        };
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert_eq!(
            result.branch_parents.len(),
            result.branch_paths.len(),
            "branch_parents length must match branch_paths"
        );
        for (i, parent) in result.branch_parents.iter().enumerate() {
            assert!(
                parent.is_none(),
                "Branch {i} should have no parent when fork_max_depth=0"
            );
        }
    }

    #[test]
    fn no_forks_when_max_depth_zero() {
        // fork_chance=1.0 but max_depth=0 — no forks should spawn.
        let config = GameConfig {
            tree_branch_fork_chance: 1.0,
            tree_branch_fork_max_depth: 0,
            ..fork_test_config()
        };
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert_eq!(
            result.branch_paths.len(),
            config.tree_branch_count as usize,
            "No forks when max_depth=0"
        );
    }

    #[test]
    fn forks_spawn_with_high_probability() {
        // fork_chance=1.0, max_depth=1, min_step=2 — every eligible step
        // spawns a fork, so we should get more paths than branch_count.
        let config = GameConfig {
            tree_branch_fork_chance: 1.0,
            tree_branch_fork_min_step: 2,
            tree_branch_fork_max_depth: 1,
            ..fork_test_config()
        };
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(
            result.branch_paths.len() > config.tree_branch_count as usize,
            "With fork_chance=1.0, should have more paths ({}) than branch_count ({})",
            result.branch_paths.len(),
            config.tree_branch_count,
        );

        // At least some branch_parents should be Some.
        let fork_count = result.branch_parents.iter().filter(|p| p.is_some()).count();
        assert!(
            fork_count > 0,
            "Should have at least one sub-branch with a parent"
        );
    }

    #[test]
    fn fork_roots_adjacent_to_parent() {
        // Sub-branch path[0] should be within manhattan distance <= 2 of
        // the parent's path at the fork step.
        let config = GameConfig {
            tree_branch_fork_chance: 1.0,
            tree_branch_fork_min_step: 2,
            tree_branch_fork_max_depth: 1,
            ..fork_test_config()
        };
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        for (i, parent) in result.branch_parents.iter().enumerate() {
            if let Some(bp) = parent {
                let fork_root = result.branch_paths[i][0];
                let parent_pos = result.branch_paths[bp.parent_path_idx][bp.fork_step];
                let dist = fork_root.manhattan_distance(parent_pos);
                assert!(
                    dist <= 2,
                    "Sub-branch {i} root {fork_root} too far from parent fork point \
                     {parent_pos} (manhattan distance {dist}, expected <= 2)"
                );
            }
        }
    }

    #[test]
    fn multi_level_forks_respect_depth() {
        // max_depth=2, fork_chance=1.0: should see depth-2 forks but no depth-3.
        let config = GameConfig {
            tree_branch_fork_chance: 1.0,
            tree_branch_fork_min_step: 2,
            tree_branch_fork_max_depth: 2,
            ..fork_test_config()
        };
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // Compute depth for each branch by following parent chains.
        let mut depths: Vec<u32> = vec![0; result.branch_paths.len()];
        for (i, parent) in result.branch_parents.iter().enumerate() {
            if let Some(bp) = parent {
                depths[i] = depths[bp.parent_path_idx] + 1;
            }
        }

        let max_depth_seen = *depths.iter().max().unwrap_or(&0);
        assert!(
            max_depth_seen >= 2,
            "With max_depth=2 and fork_chance=1.0, should see depth-2 forks \
             (max seen: {max_depth_seen})"
        );
        assert!(
            max_depth_seen <= 2,
            "Should never exceed max_depth=2 (max seen: {max_depth_seen})"
        );
    }

    #[test]
    fn deterministic_fork_generation() {
        // Same seed -> identical paths and parents.
        let config = GameConfig {
            tree_branch_fork_chance: 0.5,
            tree_branch_fork_min_step: 2,
            tree_branch_fork_max_depth: 2,
            ..fork_test_config()
        };

        let mut world_a = VoxelWorld::new(128, 128, 128);
        let mut rng_a = GameRng::new(42);
        let result_a = generate_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(128, 128, 128);
        let mut rng_b = GameRng::new(42);
        let result_b = generate_tree(&mut world_b, &config, &mut rng_b);

        assert_eq!(result_a.branch_paths.len(), result_b.branch_paths.len());
        for (i, (pa, pb)) in result_a
            .branch_paths
            .iter()
            .zip(result_b.branch_paths.iter())
            .enumerate()
        {
            assert_eq!(pa, pb, "Branch path {i} differs between runs");
        }
        assert_eq!(result_a.branch_voxels, result_b.branch_voxels);
    }

    // --- Leaf tests ---

    /// Helper config for leaf tests: uses branches long enough to test coverage.
    fn leaf_test_config() -> GameConfig {
        GameConfig {
            world_size: (128, 128, 128),
            tree_trunk_radius: 4,
            tree_trunk_height: 60,
            tree_branch_start_y: 20,
            tree_branch_interval: 10,
            tree_branch_count: 4,
            tree_branch_length: 15,
            tree_branch_radius: 2,
            tree_branch_fork_max_depth: 0,
            tree_leaf_blob_radius: 3,
            tree_leaf_blob_density: 0.65,
            tree_leaf_tip_only: false,
            tree_leaf_branch_coverage: 0.4,
            tree_leaf_blob_spacing: 3,
            ..GameConfig::default()
        }
    }

    #[test]
    fn generates_leaf_voxels() {
        let config = leaf_test_config();
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(!result.leaf_voxels.is_empty(), "Should generate leaf voxels");
        for coord in &result.leaf_voxels {
            assert_eq!(
                world.get(*coord),
                VoxelType::Leaf,
                "Leaf voxel at {coord} should be Leaf type in world"
            );
        }
    }

    #[test]
    fn leaves_do_not_overwrite_trunk_or_branch() {
        let config = leaf_test_config();
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // All trunk voxels should still be Trunk.
        for coord in &result.trunk_voxels {
            assert_eq!(
                world.get(*coord),
                VoxelType::Trunk,
                "Trunk voxel at {coord} was overwritten"
            );
        }
        // All branch voxels should still be Branch.
        for coord in &result.branch_voxels {
            assert_eq!(
                world.get(*coord),
                VoxelType::Branch,
                "Branch voxel at {coord} was overwritten"
            );
        }
    }

    #[test]
    fn leaf_generation_deterministic() {
        let config = leaf_test_config();

        let mut world_a = VoxelWorld::new(128, 128, 128);
        let mut rng_a = GameRng::new(42);
        let result_a = generate_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(128, 128, 128);
        let mut rng_b = GameRng::new(42);
        let result_b = generate_tree(&mut world_b, &config, &mut rng_b);

        assert_eq!(result_a.leaf_voxels, result_b.leaf_voxels);
    }

    #[test]
    fn no_leaves_at_zero_density() {
        let config = GameConfig {
            tree_leaf_blob_density: 0.0,
            ..leaf_test_config()
        };
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(
            result.leaf_voxels.is_empty(),
            "With density=0, should have no leaf voxels (got {})",
            result.leaf_voxels.len()
        );
    }

    #[test]
    fn leaves_near_branch_tips() {
        let config = leaf_test_config();
        let mut world = VoxelWorld::new(128, 128, 128);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        let blob_radius = config.tree_leaf_blob_radius;
        // Collect all branch tips.
        let tips: Vec<VoxelCoord> = result
            .branch_paths
            .iter()
            .filter_map(|p| p.last().copied())
            .collect();

        // Every leaf voxel should be within blob_radius of some branch tip
        // or some path position in the covered range.
        for coord in &result.leaf_voxels {
            let near_tip = tips.iter().any(|tip| {
                coord.manhattan_distance(*tip) <= blob_radius * 2
            });
            let near_path = result.branch_paths.iter().any(|path| {
                path.iter()
                    .any(|p| coord.manhattan_distance(*p) <= blob_radius * 2)
            });
            assert!(
                near_tip || near_path,
                "Leaf voxel at {coord} is not near any branch tip or path"
            );
        }
    }
}
