// Energy-based recursive tree generation.
//
// Generates organic tree geometry — trunk, branches, roots, and leaf canopy —
// using a unified energy-based segment growth model. The core insight: **the
// trunk is just the first branch**. All segments (trunk, branches, sub-branches,
// roots) are grown by the same algorithm, differing only in their initial energy,
// direction, and gravitropism.
//
// ## Algorithm overview
//
// 1. Seed the trunk segment at world center, directed upward, with energy =
//    `initial_energy * (1 - root_energy_fraction)`.
// 2. Seed `root_initial_count` root segments at the trunk base, directed
//    outward+downward, sharing the remaining root energy equally.
// 3. Process a `VecDeque<SegmentJob>` in FIFO order (breadth-first by
//    generation = deterministic).
// 4. Each segment grows step-by-step, consuming energy, tapering radius as
//    `sqrt(energy * energy_to_radius)`, applying gravitropism + random
//    deflection with coherence. May split into children (continuation + side
//    branches, dividing energy per `split_energy_ratio`). Terminates when
//    energy is exhausted.
// 5. Voxels are classified: generation-0 non-root = Trunk, generation-1+
//    non-root = Branch, root segments = Root.
// 6. Leaf blobs are placed at terminal positions of non-root segments.
//
// ## 6-connectivity invariant
//
// Every tree voxel (Trunk, Branch, Root) must share at least one face with
// another tree voxel. When a growth step moves diagonally across 2 or 3
// axes, `bridge_cross_sections()` fills intermediate voxels along a
// 6-connected path between consecutive step positions. See also
// `round_to_voxel()`.
//
// ## Voxel placement priority
//
// Trunk > Branch > Root > Leaf. Higher-priority types are never overwritten.
//
// ## Vec3 helpers
//
// Free functions for 3D vector math (`normalize`, `cross`, `dot`,
// `rotate_around_axis` via Rodrigues' formula, `random_perpendicular`).
// No external crate dependencies.
//
// See also: `world.rs` for the voxel grid being populated, `nav.rs` for the
// navigation graph built on top of the generated geometry, `config.rs` for
// `TreeProfile` and its sub-structs, `sim.rs` which calls `generate_tree()`.
//
// **Critical constraint: determinism.** All randomness comes from the `GameRng`
// passed by the caller. The FIFO work queue ensures breadth-first processing
// order. RNG draws happen in fixed patterns per step (always draw split roll
// when eligible, always draw curvature rolls). `f32` trig is IEEE 754.
// No HashMap.

use crate::config::{GameConfig, LeafShape, TreeProfile};
use crate::prng::GameRng;
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Vec3 helpers — minimal 3D math, no external crate
// ---------------------------------------------------------------------------

type Vec3 = [f32; 3];

fn vec3_add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn vec3_scale(v: Vec3, s: f32) -> Vec3 {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn vec3_dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn vec3_cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn vec3_length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn vec3_normalize(v: Vec3) -> Vec3 {
    let len = vec3_length(v);
    if len < 1e-10 {
        [0.0, 1.0, 0.0] // fallback to up
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

/// Rodrigues' rotation formula: rotate `v` around `axis` by `angle` radians.
fn rotate_around_axis(v: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    let cos_a = angle.cos();
    let sin_a = angle.sin();
    let dot = vec3_dot(axis, v);
    let cross = vec3_cross(axis, v);
    [
        v[0] * cos_a + cross[0] * sin_a + axis[0] * dot * (1.0 - cos_a),
        v[1] * cos_a + cross[1] * sin_a + axis[1] * dot * (1.0 - cos_a),
        v[2] * cos_a + cross[2] * sin_a + axis[2] * dot * (1.0 - cos_a),
    ]
}

/// Find a vector perpendicular to `v` using a deterministic choice of cross
/// product partner. Uses the axis least aligned with `v`.
fn find_perpendicular(v: Vec3) -> Vec3 {
    let abs_x = v[0].abs();
    let abs_y = v[1].abs();
    let abs_z = v[2].abs();
    let partner = if abs_x <= abs_y && abs_x <= abs_z {
        [1.0, 0.0, 0.0]
    } else if abs_y <= abs_z {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    vec3_normalize(vec3_cross(v, partner))
}

/// Generate a random perpendicular direction to `v` using the RNG.
fn random_perpendicular(v: Vec3, rng: &mut GameRng) -> Vec3 {
    let perp = find_perpendicular(v);
    let angle = rng.next_f32() * std::f32::consts::TAU;
    rotate_around_axis(perp, v, angle)
}

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Result of tree generation — voxel lists to store on the Tree entity.
pub struct TreeGenResult {
    pub trunk_voxels: Vec<VoxelCoord>,
    pub branch_voxels: Vec<VoxelCoord>,
    /// Leaf voxel positions placed as blobs at branch terminals.
    pub leaf_voxels: Vec<VoxelCoord>,
    /// Root voxel positions (at or below ground level).
    pub root_voxels: Vec<VoxelCoord>,
}

// ---------------------------------------------------------------------------
// Segment job — work queue item
// ---------------------------------------------------------------------------

/// A pending segment growth job for the iterative work queue.
struct SegmentJob {
    /// Current position in float space.
    position: Vec3,
    /// Current growth direction (unit vector).
    direction: Vec3,
    /// Remaining energy budget.
    energy: f32,
    /// Generation depth: 0 = trunk, 1+ = branches.
    generation: u32,
    /// Accumulated deflection state for coherent curvature.
    deflection_axis: Vec3,
    /// Whether this is a root segment.
    is_root: bool,
}

// ---------------------------------------------------------------------------
// Leaf blob position
// ---------------------------------------------------------------------------

/// Records where a non-root segment terminated, for later leaf blob placement.
struct LeafBlobCenter {
    position: Vec3,
}

// ---------------------------------------------------------------------------
// Voxel priority helper
// ---------------------------------------------------------------------------

/// Returns the placement priority of a voxel type (higher = more important).
fn voxel_priority(vt: VoxelType) -> u8 {
    match vt {
        VoxelType::Trunk => 4,
        VoxelType::Branch => 3,
        VoxelType::Root => 2,
        VoxelType::Leaf => 1,
        _ => 0,
    }
}

/// Try to place a voxel, respecting priority (won't overwrite higher-priority types).
/// Returns true if the voxel was placed. Silently skips out-of-bounds coordinates.
fn try_place_voxel(
    world: &mut VoxelWorld,
    coord: VoxelCoord,
    vtype: VoxelType,
    voxel_list: &mut Vec<VoxelCoord>,
) -> bool {
    // Skip out-of-bounds to avoid recording phantom voxels.
    if coord.x < 0
        || coord.y < 0
        || coord.z < 0
        || coord.x >= world.size_x as i32
        || coord.y >= world.size_y as i32
        || coord.z >= world.size_z as i32
    {
        return false;
    }
    let existing = world.get(coord);
    if voxel_priority(existing) < voxel_priority(vtype) {
        world.set(coord, vtype);
        voxel_list.push(coord);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Core generation
// ---------------------------------------------------------------------------

/// Generate a tree at the world center, populating the voxel world and
/// returning the voxel lists for the Tree entity.
///
/// Also fills `ForestFloor` at y=0 for the walkable ground plane.
pub fn generate_tree(
    world: &mut VoxelWorld,
    config: &GameConfig,
    rng: &mut GameRng,
) -> TreeGenResult {
    let profile = &config.tree_profile;

    let mut trunk_voxels = Vec::new();
    let mut branch_voxels = Vec::new();
    let mut root_voxels = Vec::new();
    let mut leaf_blob_centers: Vec<LeafBlobCenter> = Vec::new();

    let center_x = world.size_x as f32 / 2.0;
    let center_z = world.size_z as f32 / 2.0;

    // --- Forest floor at y=0 ---
    let floor_extent = config.floor_extent;
    for dx in -floor_extent..=floor_extent {
        for dz in -floor_extent..=floor_extent {
            let coord = VoxelCoord::new(center_x as i32 + dx, 0, center_z as i32 + dz);
            world.set(coord, VoxelType::ForestFloor);
        }
    }

    // --- Seed trunk segment ---
    let trunk_energy = profile.growth.initial_energy * (1.0 - profile.roots.root_energy_fraction);
    let trunk_dir = vec3_normalize(profile.trunk.initial_direction);

    let mut job_queue: VecDeque<SegmentJob> = VecDeque::new();
    job_queue.push_back(SegmentJob {
        position: [center_x, 1.0, center_z],
        direction: trunk_dir,
        energy: trunk_energy,
        generation: 0,
        deflection_axis: find_perpendicular(trunk_dir),
        is_root: false,
    });

    // --- Seed root segments ---
    let root_total_energy = profile.growth.initial_energy * profile.roots.root_energy_fraction;
    let root_count = profile.roots.root_initial_count;
    if root_count > 0 && root_total_energy > 0.0 {
        let energy_per_root = root_total_energy / root_count as f32;
        let tau = std::f32::consts::TAU;
        for i in 0..root_count {
            let angle = i as f32 * tau / root_count as f32;
            // Direction: outward + downward
            let horiz_x = angle.cos();
            let horiz_z = angle.sin();
            let down_angle = profile.roots.root_initial_angle;
            let dir = vec3_normalize([
                horiz_x * down_angle.cos(),
                -down_angle.sin(),
                horiz_z * down_angle.cos(),
            ]);
            job_queue.push_back(SegmentJob {
                position: [center_x, 1.0, center_z],
                direction: dir,
                energy: energy_per_root,
                generation: 0,
                deflection_axis: find_perpendicular(dir),
                is_root: true,
            });
        }
    }

    // --- Process work queue ---
    while let Some(job) = job_queue.pop_front() {
        grow_segment(
            job,
            world,
            profile,
            rng,
            &mut trunk_voxels,
            &mut branch_voxels,
            &mut root_voxels,
            &mut leaf_blob_centers,
            &mut job_queue,
        );
    }

    // --- Leaf blobs (separate pass after all segments) ---
    let leaf_voxels = generate_leaf_blobs(&leaf_blob_centers, world, profile, rng);

    TreeGenResult {
        trunk_voxels,
        branch_voxels,
        leaf_voxels,
        root_voxels,
    }
}

/// Grow a single segment from the given job, placing voxels and potentially
/// spawning child segments via splitting.
fn grow_segment(
    mut job: SegmentJob,
    world: &mut VoxelWorld,
    profile: &TreeProfile,
    rng: &mut GameRng,
    trunk_voxels: &mut Vec<VoxelCoord>,
    branch_voxels: &mut Vec<VoxelCoord>,
    root_voxels: &mut Vec<VoxelCoord>,
    leaf_blob_centers: &mut Vec<LeafBlobCenter>,
    job_queue: &mut VecDeque<SegmentJob>,
) {
    let initial_energy = job.energy;
    let step_len = profile.growth.growth_step_length;
    let energy_per_step = profile.growth.energy_per_step;
    let base_flare = profile.trunk.base_flare;

    let mut step_count = 0u32;
    let mut prev_voxel_center = round_to_voxel(job.position);

    while job.energy > 0.0 {
        // Compute radius from remaining energy.
        let mut radius = (job.energy * profile.growth.energy_to_radius).sqrt();
        if radius < profile.growth.min_radius {
            radius = profile.growth.min_radius;
        }

        // Apply base flare for trunk (generation 0, non-root, near ground).
        if job.generation == 0 && !job.is_root && base_flare > 0.0 {
            let height = job.position[1] - 1.0;
            if height < 5.0 {
                let flare_factor = 1.0 + base_flare * (1.0 - height / 5.0).max(0.0);
                radius *= flare_factor;
            }
        }

        // Determine voxel type for this segment.
        let vtype = if job.is_root {
            VoxelType::Root
        } else if job.generation == 0 {
            VoxelType::Trunk
        } else {
            VoxelType::Branch
        };

        let voxel_list = if job.is_root {
            &mut *root_voxels
        } else if job.generation == 0 {
            &mut *trunk_voxels
        } else {
            &mut *branch_voxels
        };

        // Bridge gap from previous step if the voxel center jumped diagonally
        // (would otherwise create corner-only or edge-only connections).
        let current_voxel_center = round_to_voxel(job.position);
        bridge_cross_sections(
            world,
            prev_voxel_center,
            current_voxel_center,
            job.direction,
            radius,
            vtype,
            voxel_list,
        );
        prev_voxel_center = current_voxel_center;

        // Place cross-section voxels at current position.
        place_cross_section(world, job.position, job.direction, radius, vtype, voxel_list);

        // Check for split.
        let progress = 1.0 - (job.energy / initial_energy);
        if progress >= profile.split.min_progress_for_split && !job.is_root {
            let split_roll = rng.next_f32();
            if split_roll < profile.split.split_chance_base {
                // Spawn child branches.
                for _ in 0..profile.split.split_count {
                    let child_energy = job.energy * profile.split.split_energy_ratio;
                    if child_energy > energy_per_step {
                        // Deflect child direction.
                        let angle_offset = profile.split.split_angle
                            + rng.range_f32(
                                -profile.split.split_angle_variance,
                                profile.split.split_angle_variance,
                            );
                        let perp = random_perpendicular(job.direction, rng);
                        let child_dir =
                            vec3_normalize(rotate_around_axis(job.direction, perp, angle_offset));

                        job_queue.push_back(SegmentJob {
                            position: job.position,
                            direction: child_dir,
                            energy: child_energy,
                            generation: job.generation + 1,
                            deflection_axis: find_perpendicular(child_dir),
                            is_root: false,
                        });

                        // Deduct energy from parent.
                        job.energy -= child_energy;
                    }
                }
            }
        } else if job.is_root {
            // Roots don't split, but draw the roll for determinism.
            let _roll = rng.next_f32();
        }

        // Consume energy for this step.
        job.energy -= energy_per_step;
        if job.energy <= 0.0 {
            break;
        }

        // Apply curvature: gravitropism + random deflection with coherence.
        let gravitropism = if job.is_root {
            -profile.roots.root_gravitropism // roots grow downward
        } else {
            profile.curvature.gravitropism
        };

        // Gravitropism: bias direction toward vertical.
        let up = [0.0_f32, 1.0, 0.0];
        let grav_bias = vec3_scale(up, gravitropism);
        job.direction = vec3_normalize(vec3_add(job.direction, grav_bias));

        // Root surface tendency: pull roots back toward y=0.
        if job.is_root && profile.roots.root_surface_tendency > 0.0 {
            let target_y = 0.0_f32;
            let y_offset = job.position[1] - target_y;
            // If root is below ground, pull up; if above, pull down.
            let surface_bias = [0.0, -y_offset * profile.roots.root_surface_tendency * 0.1, 0.0];
            job.direction = vec3_normalize(vec3_add(job.direction, surface_bias));
        }

        // Random deflection with coherence.
        let deflection_amount = profile.curvature.random_deflection;
        if deflection_amount > 0.0 {
            // Generate a new random deflection axis.
            let new_deflection = random_perpendicular(job.direction, rng);
            // Blend with previous deflection for coherence.
            let coherence = profile.curvature.deflection_coherence;
            job.deflection_axis = vec3_normalize(vec3_add(
                vec3_scale(job.deflection_axis, coherence),
                vec3_scale(new_deflection, 1.0 - coherence),
            ));
            // Apply deflection.
            job.direction = vec3_normalize(rotate_around_axis(
                job.direction,
                job.deflection_axis,
                deflection_amount * rng.range_f32(-1.0, 1.0),
            ));
        }

        // Advance position.
        job.position = vec3_add(job.position, vec3_scale(job.direction, step_len));

        step_count += 1;
    }

    // Record terminal position for leaf blob placement (non-root only).
    if !job.is_root && step_count > 0 {
        leaf_blob_centers.push(LeafBlobCenter {
            position: job.position,
        });
    }
}

/// Convert a float position to a voxel coordinate by rounding.
fn round_to_voxel(pos: Vec3) -> VoxelCoord {
    VoxelCoord::new(
        pos[0].round() as i32,
        pos[1].round() as i32,
        pos[2].round() as i32,
    )
}

/// Place cross-sections along a 6-connected (face-sharing) path between two
/// voxel centers. This fills gaps when a growth step moves diagonally across
/// 2 or 3 axes, which would otherwise leave only corner/edge connections.
///
/// The path is traced greedily: each step advances along whichever axis has
/// the largest remaining distance. This is deterministic and produces the
/// shortest 6-connected path (Manhattan distance).
///
/// Only intermediate points are filled — the caller already places
/// cross-sections at both endpoints.
fn bridge_cross_sections(
    world: &mut VoxelWorld,
    from: VoxelCoord,
    to: VoxelCoord,
    direction: Vec3,
    radius: f32,
    vtype: VoxelType,
    voxel_list: &mut Vec<VoxelCoord>,
) {
    let mut current = from;
    while current != to {
        let dx = to.x - current.x;
        let dy = to.y - current.y;
        let dz = to.z - current.z;

        // Step along the axis with the largest remaining gap.
        if dx.abs() >= dy.abs() && dx.abs() >= dz.abs() {
            current.x += dx.signum();
        } else if dy.abs() >= dz.abs() {
            current.y += dy.signum();
        } else {
            current.z += dz.signum();
        }

        // Skip the final point — the caller places a cross-section there.
        if current == to {
            break;
        }

        let bridge_center = [current.x as f32, current.y as f32, current.z as f32];
        place_cross_section(world, bridge_center, direction, radius, vtype, voxel_list);
    }
}

/// Place a filled circle of voxels perpendicular to the growth direction.
fn place_cross_section(
    world: &mut VoxelWorld,
    center: Vec3,
    _direction: Vec3,
    radius: f32,
    vtype: VoxelType,
    voxel_list: &mut Vec<VoxelCoord>,
) {
    let r = radius.round() as i32;
    if r <= 0 {
        // Single voxel.
        let coord = VoxelCoord::new(
            center[0].round() as i32,
            center[1].round() as i32,
            center[2].round() as i32,
        );
        try_place_voxel(world, coord, vtype, voxel_list);
        return;
    }

    let r_sq = radius * radius;
    let cx = center[0].round() as i32;
    let cy = center[1].round() as i32;
    let cz = center[2].round() as i32;

    // Axis-aligned cross-section (good enough for voxels).
    for dx in -r..=r {
        for dz in -r..=r {
            let dist_sq = (dx * dx + dz * dz) as f32;
            if dist_sq <= r_sq {
                let coord = VoxelCoord::new(cx + dx, cy, cz + dz);
                try_place_voxel(world, coord, vtype, voxel_list);
            }
        }
    }
}

/// Generate leaf blob voxels at branch terminal positions.
fn generate_leaf_blobs(
    blob_centers: &[LeafBlobCenter],
    world: &mut VoxelWorld,
    profile: &TreeProfile,
    rng: &mut GameRng,
) -> Vec<VoxelCoord> {
    let mut leaf_voxels = Vec::new();
    let effective_density = profile.leaves.leaf_density * profile.leaves.canopy_density;

    if effective_density <= 0.0 {
        // Still draw RNG for determinism.
        for _center in blob_centers {
            let r = profile.leaves.leaf_size as i32;
            let count = (2 * r + 1) * (2 * r + 1) * (2 * r + 1);
            for _ in 0..count {
                rng.next_f32();
            }
        }
        return leaf_voxels;
    }

    let radius = profile.leaves.leaf_size as i32;
    let is_cloud = profile.leaves.leaf_shape == LeafShape::Cloud;

    for center in blob_centers {
        let cx = center.position[0].round() as i32;
        let cy = center.position[1].round() as i32;
        let cz = center.position[2].round() as i32;

        let r_sq = (radius * radius) as f32;

        for dx in -radius..=radius {
            for dy in -radius..=radius {
                for dz in -radius..=radius {
                    // Distance check (cloud shape compresses Y).
                    let fy = if is_cloud { dy as f32 * 1.5 } else { dy as f32 };
                    let dist_sq = dx as f32 * dx as f32 + fy * fy + dz as f32 * dz as f32;

                    // Always draw for determinism.
                    let roll = rng.next_f32();

                    if dist_sq > r_sq {
                        continue;
                    }
                    if roll >= effective_density {
                        continue;
                    }

                    let coord = VoxelCoord::new(cx + dx, cy + dy, cz + dz);
                    try_place_voxel(world, coord, VoxelType::Leaf, &mut leaf_voxels);
                }
            }
        }
    }

    leaf_voxels
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default test config with small world and scaled-down energy to fit.
    /// The 64^3 world can hold ~50 voxels of height, so we use 50 energy
    /// (at 1 energy/step) to stay well within bounds.
    fn test_config() -> GameConfig {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        config.tree_profile.growth.initial_energy = 50.0;
        config
    }

    /// Config with roots disabled for simpler tests.
    fn test_config_no_roots() -> GameConfig {
        let mut config = test_config();
        config.tree_profile.roots.root_energy_fraction = 0.0;
        config.tree_profile.roots.root_initial_count = 0;
        config
    }

    #[test]
    fn generates_trunk_voxels() {
        let config = test_config_no_roots();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(!result.trunk_voxels.is_empty());
        for coord in &result.trunk_voxels {
            assert_eq!(world.get(*coord), VoxelType::Trunk);
        }
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
        assert_eq!(result_a.root_voxels, result_b.root_voxels);
        assert_eq!(result_a.leaf_voxels, result_b.leaf_voxels);
    }

    #[test]
    fn different_seeds_produce_different_trees() {
        let config = test_config_no_roots();

        let mut world_a = VoxelWorld::new(64, 64, 64);
        let mut rng_a = GameRng::new(42);
        let result_a = generate_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(999);
        let result_b = generate_tree(&mut world_b, &config, &mut rng_b);

        // Branch geometry uses RNG for angles, growth, forking — must differ.
        assert_ne!(
            result_a.branch_voxels, result_b.branch_voxels,
            "Different seeds must produce different branch geometry"
        );
    }

    #[test]
    fn trunk_tapers() {
        let config = test_config_no_roots();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        // Count trunk voxels near the base (y <= 3) vs near the top.
        let max_y = result.trunk_voxels.iter().map(|v| v.y).max().unwrap_or(0);
        let top_threshold = max_y - 3;

        let base_count = result.trunk_voxels.iter().filter(|v| v.y <= 3).count();
        let top_count = result.trunk_voxels.iter().filter(|v| v.y >= top_threshold).count();

        assert!(
            base_count > top_count,
            "Trunk should taper: base ({base_count} voxels at y<=3) should be \
             wider than top ({top_count} voxels at y>={top_threshold})"
        );
    }

    #[test]
    fn generates_forest_floor() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_tree(&mut world, &config, &mut rng);

        let center = VoxelCoord::new(32, 0, 32);
        assert_eq!(world.get(center), VoxelType::ForestFloor);
    }

    #[test]
    fn splits_produce_branch_voxels() {
        let mut config = test_config_no_roots();
        // High split chance to guarantee branches.
        config.tree_profile.split.split_chance_base = 1.0;
        config.tree_profile.split.min_progress_for_split = 0.05;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(!result.trunk_voxels.is_empty(), "Should have trunk voxels");
        assert!(
            !result.branch_voxels.is_empty(),
            "With split_chance=1.0, should have branch voxels"
        );
    }

    #[test]
    fn generates_root_voxels() {
        let mut config = test_config();
        // Give roots enough energy to actually grow.
        config.tree_profile.growth.initial_energy = 100.0;
        config.tree_profile.roots.root_energy_fraction = 0.3;
        config.tree_profile.roots.root_initial_count = 3;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(
            !result.root_voxels.is_empty(),
            "With roots enabled, should generate root voxels"
        );
        for coord in &result.root_voxels {
            assert_eq!(world.get(*coord), VoxelType::Root);
        }
    }

    #[test]
    fn roots_spread_outward() {
        let mut config = test_config();
        config.tree_profile.growth.initial_energy = 200.0;
        config.tree_profile.roots.root_energy_fraction = 0.3;
        config.tree_profile.roots.root_initial_count = 3;
        // Reduce downward pull so roots stay in bounds and spread horizontally.
        config.tree_profile.roots.root_gravitropism = 0.02;
        config.tree_profile.roots.root_initial_angle = 0.1;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        let center_x = 32;
        let center_z = 32;

        // At least some root voxels should be horizontally distant from center.
        let far_roots = result.root_voxels.iter().filter(|v| {
            let dx = (v.x - center_x).abs();
            let dz = (v.z - center_z).abs();
            dx > 3 || dz > 3
        }).count();

        assert!(
            far_roots > 0,
            "Root voxels should spread outward from trunk base (got {} far roots out of {} total)",
            far_roots,
            result.root_voxels.len()
        );
    }

    #[test]
    fn generates_leaf_voxels() {
        let mut config = test_config_no_roots();
        // Ensure splits happen so we get terminals with leaves.
        config.tree_profile.split.split_chance_base = 0.5;
        config.tree_profile.leaves.leaf_density = 0.8;
        config.tree_profile.leaves.canopy_density = 1.0;
        config.tree_profile.leaves.leaf_size = 3;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(
            !result.leaf_voxels.is_empty(),
            "Should generate leaf voxels at branch terminals"
        );
        for coord in &result.leaf_voxels {
            assert_eq!(world.get(*coord), VoxelType::Leaf);
        }
    }

    #[test]
    fn leaves_do_not_overwrite_wood() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
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
        // All root voxels should still be Root.
        for coord in &result.root_voxels {
            assert_eq!(
                world.get(*coord),
                VoxelType::Root,
                "Root voxel at {coord} was overwritten"
            );
        }
    }

    #[test]
    fn no_leaves_at_zero_density() {
        let mut config = test_config_no_roots();
        config.tree_profile.leaves.canopy_density = 0.0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(
            result.leaf_voxels.is_empty(),
            "With canopy_density=0, should have no leaf voxels (got {})",
            result.leaf_voxels.len()
        );
    }

    #[test]
    fn base_flare_widens_trunk() {
        // Compare trunk width at y=1 with and without base flare.
        let mut config_no_flare = test_config_no_roots();
        config_no_flare.tree_profile.trunk.base_flare = 0.0;

        let mut config_flare = test_config_no_roots();
        config_flare.tree_profile.trunk.base_flare = 1.0;

        let mut world_nf = VoxelWorld::new(64, 64, 64);
        let mut rng_nf = GameRng::new(42);
        let result_nf = generate_tree(&mut world_nf, &config_no_flare, &mut rng_nf);

        let mut world_f = VoxelWorld::new(64, 64, 64);
        let mut rng_f = GameRng::new(42);
        let result_f = generate_tree(&mut world_f, &config_flare, &mut rng_f);

        let base_count_no_flare = result_nf.trunk_voxels.iter().filter(|v| v.y == 1).count();
        let base_count_flare = result_f.trunk_voxels.iter().filter(|v| v.y == 1).count();

        assert!(
            base_count_flare > base_count_no_flare,
            "Base flare should widen trunk at y=1: no_flare={base_count_no_flare}, \
             flare={base_count_flare}"
        );
    }

    #[test]
    fn deterministic_with_splits_and_roots() {
        let mut config = test_config();
        config.tree_profile.split.split_chance_base = 0.5;
        config.tree_profile.roots.root_energy_fraction = 0.15;
        config.tree_profile.roots.root_initial_count = 4;

        let mut world_a = VoxelWorld::new(64, 64, 64);
        let mut rng_a = GameRng::new(99);
        let result_a = generate_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(99);
        let result_b = generate_tree(&mut world_b, &config, &mut rng_b);

        assert_eq!(result_a.trunk_voxels, result_b.trunk_voxels);
        assert_eq!(result_a.branch_voxels, result_b.branch_voxels);
        assert_eq!(result_a.root_voxels, result_b.root_voxels);
        assert_eq!(result_a.leaf_voxels, result_b.leaf_voxels);
    }

    /// Every non-air tree voxel (Trunk, Branch, Root) must share at least one
    /// face with another tree voxel — no corner-only or edge-only connections.
    /// This is the 6-connectivity invariant.
    #[test]
    fn all_wood_voxels_are_face_connected() {
        // Use a diagonal direction to maximize the chance of corner/edge jumps.
        let mut config = test_config_no_roots();
        config.tree_profile.growth.initial_energy = 30.0;
        // Diagonal direction: [1,1,1]/sqrt(3) — worst case for connectivity.
        config.tree_profile.trunk.initial_direction = [0.577, 0.577, 0.577];
        // No splits — just one straight diagonal line.
        config.tree_profile.split.split_chance_base = 0.0;
        // No gravitropism or deflection — keep the direction purely diagonal.
        config.tree_profile.curvature.gravitropism = 0.0;
        config.tree_profile.curvature.random_deflection = 0.0;
        // Min radius to keep branches skinny (single-voxel cross-sections).
        config.tree_profile.growth.min_radius = 0.1;
        config.tree_profile.growth.energy_to_radius = 0.0001;
        config.tree_profile.trunk.base_flare = 0.0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        let all_wood: Vec<VoxelCoord> = result
            .trunk_voxels
            .iter()
            .chain(result.branch_voxels.iter())
            .copied()
            .collect();

        assert!(all_wood.len() > 5, "Need enough voxels to test (got {})", all_wood.len());

        // Build a set for O(1) lookup.
        let wood_set: std::collections::HashSet<VoxelCoord> = all_wood.iter().copied().collect();

        let face_offsets: [(i32, i32, i32); 6] = [
            (1, 0, 0), (-1, 0, 0),
            (0, 1, 0), (0, -1, 0),
            (0, 0, 1), (0, 0, -1),
        ];

        for &coord in &all_wood {
            let has_face_neighbor = face_offsets.iter().any(|&(dx, dy, dz)| {
                let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                wood_set.contains(&neighbor)
            });
            assert!(
                has_face_neighbor,
                "Wood voxel at {} has no face-adjacent wood neighbor (only corner/edge connected)",
                coord
            );
        }
    }

    /// Same face-connectivity check but on a full-featured tree with splits,
    /// roots, and the default fantasy_mega profile. Verifies the fix works
    /// for real-world tree shapes, not just a synthetic diagonal line.
    #[test]
    fn face_connectivity_with_splits_and_roots() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        let all_wood: Vec<VoxelCoord> = result
            .trunk_voxels
            .iter()
            .chain(result.branch_voxels.iter())
            .chain(result.root_voxels.iter())
            .copied()
            .collect();

        let wood_set: std::collections::HashSet<VoxelCoord> = all_wood.iter().copied().collect();

        let face_offsets: [(i32, i32, i32); 6] = [
            (1, 0, 0), (-1, 0, 0),
            (0, 1, 0), (0, -1, 0),
            (0, 0, 1), (0, 0, -1),
        ];

        for &coord in &all_wood {
            let has_face_neighbor = face_offsets.iter().any(|&(dx, dy, dz)| {
                let neighbor = VoxelCoord::new(coord.x + dx, coord.y + dy, coord.z + dz);
                wood_set.contains(&neighbor)
            });
            assert!(
                has_face_neighbor,
                "Wood voxel at {} has no face-adjacent wood neighbor",
                coord
            );
        }
    }

    #[test]
    fn preset_fantasy_mega_valid() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_tree(&mut world, &config, &mut rng);

        assert!(!result.trunk_voxels.is_empty(), "Should have trunk voxels");
        assert!(
            !result.branch_voxels.is_empty(),
            "Fantasy mega should produce branch voxels"
        );
        assert!(
            !result.root_voxels.is_empty(),
            "Fantasy mega should produce root voxels"
        );
        assert!(
            !result.leaf_voxels.is_empty(),
            "Fantasy mega should produce leaf voxels"
        );
    }
}
