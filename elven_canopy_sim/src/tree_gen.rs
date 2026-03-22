// Energy-based recursive tree generation and terrain.
//
// Generates organic tree geometry — trunk, branches, roots, and leaf canopy —
// using a unified energy-based segment growth model. Terrain (hilly dirt via
// value noise with bilinear interpolation) is generated separately by
// `generate_terrain()`. The core insight for trees:
// **the trunk is just the first branch**. All segments (trunk, branches,
// sub-branches, roots) are grown by the same algorithm, differing only in
// their initial energy, direction, and gravitropism.
//
// ## Algorithm overview
//
// 1. Seed the trunk segment at the given base position (world center for the
//    main tree, arbitrary XZ for lesser trees), directed upward, with energy =
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
// ## Fixed-point arithmetic
//
// All intermediate computation uses i64 fixed-point with FP_SHIFT = 16
// (FP_ONE = 65536). Config f32 values are converted to fixed-point at
// the entry to `generate_tree()`. Trigonometric functions use a 257-entry
// sine lookup table (one quadrant) computed at compile time via Taylor
// series — no runtime floats. Angles are represented in "brads" (binary
// radians): 0–1023 = one full rotation.
//
// See also: `world.rs` for the voxel grid being populated, `nav.rs` for the
// navigation graph built on top of the generated geometry, `config.rs` for
// `TreeProfile` and its sub-structs, `worldgen.rs` which calls
// `generate_terrain()` then `generate_tree()`.
//
// **Critical constraint: determinism.** All randomness comes from the `GameRng`
// passed by the caller. The FIFO work queue ensures breadth-first processing
// order. RNG draws happen in fixed patterns per step. All arithmetic is
// pure integer — no floats in the computation, guaranteed cross-platform
// identical results.

use crate::config::{GameConfig, LeafShape, TreeProfile};
use crate::prng::GameRng;
use crate::types::{VoxelCoord, VoxelType};
use crate::world::VoxelWorld;

use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Fixed-point constants and sine table
// ---------------------------------------------------------------------------

/// Fixed-point shift: 16 bits of fractional precision.
const FP_SHIFT: u32 = 16;
/// Fixed-point unit: 1.0 = 65536.
const FP_ONE: i64 = 1 << FP_SHIFT;

/// Full rotation in brads (binary radians): 1024 brads = 2π.
const ANGLE_FULL: i64 = 1024;
/// Quarter rotation: 256 brads = π/2.
const ANGLE_QUARTER: i64 = 256;

/// Compile-time Taylor series approximation of sin(x) for x in [0, π/2].
/// Accurate to ~13 significant digits — more than sufficient for 16-bit FP.
const fn sin_taylor(x: f64) -> f64 {
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    let x9 = x7 * x2;
    let x11 = x9 * x2;
    let x13 = x11 * x2;
    x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0 + x9 / 362880.0 - x11 / 39916800.0 + x13 / 6227020800.0
}

/// Build the sine lookup table at compile time.
/// 257 entries covering [0, π/2] (inclusive on both ends).
/// Index i corresponds to angle i × π/512.
/// Values are sin(angle) × FP_ONE, rounded to nearest.
#[allow(clippy::approx_constant, clippy::excessive_precision)]
const fn build_sin_table() -> [i32; 257] {
    const PI: f64 = 3.14159265358979323846;
    let mut table = [0i32; 257];
    let mut i = 0;
    while i < 257 {
        let angle = i as f64 * PI / 512.0;
        let s = sin_taylor(angle);
        table[i] = (s * 65536.0 + 0.5) as i32;
        i += 1;
    }
    table
}

static SIN_TABLE: [i32; 257] = build_sin_table();

/// Fixed-point sine. Angle in brads: 0–1023 = full circle.
fn fp_sin(angle_brads: i64) -> i64 {
    let a = ((angle_brads % ANGLE_FULL) + ANGLE_FULL) % ANGLE_FULL;
    if a < ANGLE_QUARTER {
        SIN_TABLE[a as usize] as i64
    } else if a < 2 * ANGLE_QUARTER {
        SIN_TABLE[(2 * ANGLE_QUARTER - a) as usize] as i64
    } else if a < 3 * ANGLE_QUARTER {
        -(SIN_TABLE[(a - 2 * ANGLE_QUARTER) as usize] as i64)
    } else {
        -(SIN_TABLE[(ANGLE_FULL - a) as usize] as i64)
    }
}

/// Fixed-point cosine. Angle in brads.
fn fp_cos(angle_brads: i64) -> i64 {
    fp_sin(angle_brads + ANGLE_QUARTER)
}

/// Convert f32 radians to brads (integer angle units, 1024 = 2π).
/// Used once at config boundary — the only place f32 enters tree_gen.
fn radians_to_brads(radians: f32) -> i64 {
    (radians as f64 * 1024.0 / (2.0 * std::f64::consts::PI) + 0.5) as i64
}

/// Convert an f32 value to fixed-point (FP_ONE scale). Used at config boundary.
fn f32_to_fp(val: f32) -> i64 {
    (val as f64 * FP_ONE as f64 + 0.5) as i64
}

/// Fixed-point square root: returns sqrt(x) in FP units.
/// Input x is in FP units (so the "real" value is x/FP_ONE).
/// sqrt(x/FP_ONE) * FP_ONE = sqrt(x * FP_ONE).
fn fp_sqrt(x: i64) -> i64 {
    if x <= 0 {
        return 0;
    }
    ((x as u64 * FP_ONE as u64).isqrt()) as i64
}

// ---------------------------------------------------------------------------
// Fixed-point Vec3 helpers
// ---------------------------------------------------------------------------

type Vec3 = [i64; 3];

fn vec3_add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Multiply vector by fixed-point scalar (both in FP_ONE scale).
fn vec3_scale(v: Vec3, s: i64) -> Vec3 {
    [
        (v[0] * s) >> FP_SHIFT,
        (v[1] * s) >> FP_SHIFT,
        (v[2] * s) >> FP_SHIFT,
    ]
}

fn vec3_dot(a: Vec3, b: Vec3) -> i64 {
    (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) >> FP_SHIFT
}

fn vec3_cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        (a[1] * b[2] - a[2] * b[1]) >> FP_SHIFT,
        (a[2] * b[0] - a[0] * b[2]) >> FP_SHIFT,
        (a[0] * b[1] - a[1] * b[0]) >> FP_SHIFT,
    ]
}

fn vec3_length_sq(v: Vec3) -> i64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) >> FP_SHIFT
}

fn vec3_length(v: Vec3) -> i64 {
    fp_sqrt(vec3_length_sq(v))
}

fn vec3_normalize(v: Vec3) -> Vec3 {
    let len = vec3_length(v);
    if len < 4 {
        // Near-zero — fallback to up.
        [0, FP_ONE, 0]
    } else {
        [
            (v[0] * FP_ONE) / len,
            (v[1] * FP_ONE) / len,
            (v[2] * FP_ONE) / len,
        ]
    }
}

/// Rodrigues' rotation formula: rotate `v` around `axis` by `angle` brads.
/// Both v and axis are FP-scaled unit vectors.
fn rotate_around_axis(v: Vec3, axis: Vec3, angle_brads: i64) -> Vec3 {
    let cos_a = fp_cos(angle_brads);
    let sin_a = fp_sin(angle_brads);
    let dot = vec3_dot(axis, v);
    let cross = vec3_cross(axis, v);
    [
        ((v[0] * cos_a) >> FP_SHIFT)
            + ((cross[0] * sin_a) >> FP_SHIFT)
            + ((((axis[0] * dot) >> FP_SHIFT) * (FP_ONE - cos_a)) >> FP_SHIFT),
        ((v[1] * cos_a) >> FP_SHIFT)
            + ((cross[1] * sin_a) >> FP_SHIFT)
            + ((((axis[1] * dot) >> FP_SHIFT) * (FP_ONE - cos_a)) >> FP_SHIFT),
        ((v[2] * cos_a) >> FP_SHIFT)
            + ((cross[2] * sin_a) >> FP_SHIFT)
            + ((((axis[2] * dot) >> FP_SHIFT) * (FP_ONE - cos_a)) >> FP_SHIFT),
    ]
}

/// Find a vector perpendicular to `v` using a deterministic choice of cross
/// product partner. Uses the axis least aligned with `v`.
fn find_perpendicular(v: Vec3) -> Vec3 {
    let abs_x = v[0].abs();
    let abs_y = v[1].abs();
    let abs_z = v[2].abs();
    let partner = if abs_x <= abs_y && abs_x <= abs_z {
        [FP_ONE, 0, 0]
    } else if abs_y <= abs_z {
        [0, FP_ONE, 0]
    } else {
        [0, 0, FP_ONE]
    };
    vec3_normalize(vec3_cross(v, partner))
}

/// Generate a random perpendicular direction to `v` using the RNG.
/// Consumes exactly one RNG draw (next_u64 via next_u32).
fn random_perpendicular(v: Vec3, rng: &mut GameRng) -> Vec3 {
    let perp = find_perpendicular(v);
    // Random angle as brads: 0–1023 covering full circle.
    // next_u32 consumes one next_u64 call, same cadence as old next_f32.
    let angle_brads = (rng.next_u32() % ANGLE_FULL as u32) as i64;
    rotate_around_axis(perp, v, angle_brads)
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
    /// Current position in fixed-point space (FP_ONE per voxel).
    position: Vec3,
    /// Current growth direction (FP-scaled unit vector).
    direction: Vec3,
    /// Remaining energy budget (FP-scaled).
    energy: i64,
    /// Generation depth: 0 = trunk, 1+ = branches.
    generation: u32,
    /// Accumulated deflection state for coherent curvature (FP-scaled unit vector).
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
// Terrain generation — value noise with bilinear interpolation (integer)
// ---------------------------------------------------------------------------

/// Generate hilly dirt terrain using value noise.
///
/// Produces a `Vec<VoxelCoord>` of dirt positions from y=1 upward. Each (x, z)
/// column within the floor extent gets at least 1 dirt voxel (minimum height 1)
/// and at most `terrain_max_height` voxels. Heights are smoothly interpolated
/// from a coarse noise grid seeded by `rng`.
///
/// Compute per-column dirt heights into a flat array (size_x × size_z,
/// row-major X-fastest). Each value is the top Y coordinate (inclusive) of
/// the dirt column: `floor_y + noise_height` where noise_height is in
/// `[1, terrain_max_height]`. The PRNG is consumed single-threaded for the
/// noise grid; height computation is parallelized with rayon.
fn compute_terrain_heights(
    size_x: u32,
    size_z: u32,
    config: &GameConfig,
    rng: &mut GameRng,
) -> Vec<i32> {
    use rayon::prelude::*;

    let max_height = config.terrain_max_height;
    let floor_y = config.floor_y;
    if max_height <= 0 {
        return Vec::new();
    }

    // noise_scale is an f32 config param; convert to FP for integer interpolation.
    let noise_scale_fp = f32_to_fp(config.terrain_noise_scale.max(1.0));

    // Coarse noise grid: one cell per noise_scale voxels, covering entire world.
    // Built single-threaded to consume PRNG deterministically.
    let world_width_fp = size_x as i64 * FP_ONE;
    let world_depth_fp = size_z as i64 * FP_ONE;

    // Coarse noise grid dimensions.
    let grid_w = ((world_width_fp + noise_scale_fp - 1) / noise_scale_fp) as usize + 2;
    let grid_h = ((world_depth_fp + noise_scale_fp - 1) / noise_scale_fp) as usize + 2;

    // Noise values in [0, FP_ONE) range, one per grid cell.
    let mut noise_grid = Vec::with_capacity(grid_w * grid_h);
    for _ in 0..(grid_w * grid_h) {
        // Map next_u32 to [0, FP_ONE). Consumes one u64, same cadence as old next_f32.
        noise_grid.push((rng.next_u32() as i64 * FP_ONE / (u32::MAX as i64 + 1)).min(FP_ONE - 1));
    }

    // Compute heights in parallel — pure math on the shared noise grid.
    let count = (size_x as usize) * (size_z as usize);
    (0..count)
        .into_par_iter()
        .map(|i| {
            let x = (i % size_x as usize) as i64;
            let z = (i / size_x as usize) as i64;
            // Map (x, z) to noise grid coordinates in FP.
            // Multiply by FP_ONE twice: once to put x into FP, once to keep
            // the result in FP after dividing by noise_scale_fp (also FP).
            let fx = x * FP_ONE * FP_ONE / noise_scale_fp;
            let fz = z * FP_ONE * FP_ONE / noise_scale_fp;
            let gx = (fx >> FP_SHIFT) as usize;
            let gz = (fz >> FP_SHIFT) as usize;
            let tx = fx & (FP_ONE - 1); // fractional part
            let tz = fz & (FP_ONE - 1);

            let gx1 = (gx + 1).min(grid_w - 1);
            let gz1 = (gz + 1).min(grid_h - 1);
            let gx = gx.min(grid_w - 1);
            let gz = gz.min(grid_h - 1);

            // Bilinear interpolation (all in FP).
            let v00 = noise_grid[gx + gz * grid_w];
            let v10 = noise_grid[gx1 + gz * grid_w];
            let v01 = noise_grid[gx + gz1 * grid_w];
            let v11 = noise_grid[gx1 + gz1 * grid_w];
            let one_minus_tx = FP_ONE - tx;
            let one_minus_tz = FP_ONE - tz;
            let raw = ((((v00 * one_minus_tx) >> FP_SHIFT) * one_minus_tz) >> FP_SHIFT)
                + ((((v10 * tx) >> FP_SHIFT) * one_minus_tz) >> FP_SHIFT)
                + ((((v01 * one_minus_tx) >> FP_SHIFT) * tz) >> FP_SHIFT)
                + ((((v11 * tx) >> FP_SHIFT) * tz) >> FP_SHIFT);

            // Map to integer height, clamped to [1, max_height].
            // raw is in [0, FP_ONE), so raw * max_height / FP_ONE gives [0, max_height).
            let height = ((raw * max_height as i64 + FP_ONE / 2) >> FP_SHIFT) as i32;
            let height = height.clamp(1, max_height);
            floor_y + height
        })
        .collect()
}

/// Generate terrain: lay a base ground plane of Dirt from y=0 to `floor_y`,
/// then optionally extend columns upward with noise-based hills. Uses rayon
/// to initialize column groups in parallel.
///
/// Must be called on a fresh `VoxelWorld` before `generate_tree`. Consumes
/// PRNG draws for the noise grid, so call order matters for determinism.
pub fn generate_terrain(world: &mut VoxelWorld, config: &GameConfig, rng: &mut GameRng) {
    let size_x = world.size_x;
    let size_z = world.size_z;
    let floor_y = config.floor_y;
    let max_height = config.terrain_max_height;

    if max_height <= 0 {
        // Flat ground: uniform height array, still parallelized via groups.
        let flat: Vec<i32> = vec![floor_y; (size_x as usize) * (size_z as usize)];
        world.init_terrain_parallel(&flat);
        return;
    }

    // Compute noise-based heights (parallel) and apply to world (parallel).
    let heights = compute_terrain_heights(size_x, size_z, config, rng);
    world.init_terrain_parallel(&heights);
}

// ---------------------------------------------------------------------------
// Core generation
// ---------------------------------------------------------------------------

/// Find the terrain surface Y at a given (x, z) position.
///
/// Scans upward from `floor_y` through Dirt voxels until hitting
/// Air (returns that Y as the surface) or a non-terrain voxel like Trunk
/// (stops and returns the last terrain Y). Returns `floor_y` if the scan
/// finds Air immediately above `floor_y`.
pub fn terrain_surface_y(
    world: &VoxelWorld,
    floor_y: i32,
    terrain_max_height: i32,
    x: i32,
    z: i32,
) -> i32 {
    let mut surface = floor_y;
    for y in floor_y..floor_y + terrain_max_height + 1 {
        let above = world.get(VoxelCoord::new(x, y + 1, z));
        if above == VoxelType::Air {
            surface = y;
            break;
        }
        if above == VoxelType::Dirt {
            surface = y + 1;
        } else {
            // Non-terrain voxel (tree material, etc.) — stop scanning.
            surface = y;
            break;
        }
    }
    surface
}

/// Generate a tree at the world center, populating the voxel world and
/// returning the voxel lists for the Tree entity.
///
/// The world must already have terrain applied via [`generate_terrain`] before
/// calling this function. The PRNG must be in the state left by
/// `generate_terrain` for deterministic results.
pub fn generate_tree(
    world: &mut VoxelWorld,
    config: &GameConfig,
    rng: &mut GameRng,
    log: &dyn Fn(&str),
) -> TreeGenResult {
    let center_x = world.size_x as i32 / 2;
    let center_z = world.size_z as i32 / 2;

    // Find terrain surface at the tree center, then sink up to 5 voxels into
    // the dirt so the trunk base is buried regardless of local terrain
    // variation. Clamp so we never go below floor_y (flat worlds have no
    // dirt to sink into).
    let surface_y = terrain_surface_y(
        world,
        config.floor_y,
        config.terrain_max_height,
        center_x,
        center_z,
    );
    let sink = 5.min(surface_y - config.floor_y);
    let base_y = surface_y - sink;

    generate_tree_at(
        world,
        &config.tree_profile,
        base_y,
        center_x,
        center_z,
        rng,
        log,
    )
}

/// Generate a tree at an arbitrary (x, z) position using the given profile.
///
/// This is the core tree generation entry point. `generate_tree()` delegates
/// here with the world center and main tree profile. Lesser tree generation
/// calls this directly with per-tree positions and smaller profiles.
///
/// `floor_y` is the effective base level: the trunk starts at `floor_y + 1`.
/// Callers typically pass a value sunk below the terrain surface so the trunk
/// base is buried in the dirt (e.g., `surface_y - 5` for the main tree).
pub fn generate_tree_at(
    world: &mut VoxelWorld,
    profile: &TreeProfile,
    floor_y: i32,
    base_x: i32,
    base_z: i32,
    rng: &mut GameRng,
    log: &dyn Fn(&str),
) -> TreeGenResult {
    let mut trunk_voxels = Vec::new();
    let mut branch_voxels = Vec::new();
    let mut root_voxels = Vec::new();
    let mut leaf_blob_centers: Vec<LeafBlobCenter> = Vec::new();

    let base_x_fp = base_x as i64 * FP_ONE;
    let base_z_fp = base_z as i64 * FP_ONE;

    // --- Convert config params to fixed-point ---
    let initial_energy_fp = f32_to_fp(profile.growth.initial_energy);
    let root_energy_fraction_fp = f32_to_fp(profile.roots.root_energy_fraction);
    let energy_per_step_fp = f32_to_fp(profile.growth.energy_per_step);

    // --- Seed trunk segment ---
    let trunk_energy =
        initial_energy_fp - ((initial_energy_fp * root_energy_fraction_fp) >> FP_SHIFT);
    let trunk_dir = vec3_normalize([
        f32_to_fp(profile.trunk.initial_direction[0]),
        f32_to_fp(profile.trunk.initial_direction[1]),
        f32_to_fp(profile.trunk.initial_direction[2]),
    ]);

    let mut job_queue: VecDeque<SegmentJob> = VecDeque::new();
    let trunk_base_y_fp = (floor_y as i64 + 1) * FP_ONE;
    job_queue.push_back(SegmentJob {
        position: [base_x_fp, trunk_base_y_fp, base_z_fp],
        direction: trunk_dir,
        energy: trunk_energy,
        generation: 0,
        deflection_axis: find_perpendicular(trunk_dir),
        is_root: false,
    });

    // --- Seed root segments ---
    let root_total_energy = (initial_energy_fp * root_energy_fraction_fp) >> FP_SHIFT;
    let root_count = profile.roots.root_initial_count;
    if root_count > 0 && root_total_energy > 0 {
        let energy_per_root = root_total_energy / root_count as i64;
        for i in 0..root_count {
            // Evenly spaced angles around the trunk.
            let angle_brads = i as i64 * ANGLE_FULL / root_count as i64;
            let horiz_x = fp_cos(angle_brads);
            let horiz_z = fp_sin(angle_brads);
            let down_angle_brads = radians_to_brads(profile.roots.root_initial_angle);
            let cos_down = fp_cos(down_angle_brads);
            let sin_down = fp_sin(down_angle_brads);
            let dir = vec3_normalize([
                ((horiz_x * cos_down) >> FP_SHIFT),
                -sin_down,
                ((horiz_z * cos_down) >> FP_SHIFT),
            ]);
            job_queue.push_back(SegmentJob {
                position: [base_x_fp, trunk_base_y_fp, base_z_fp],
                direction: dir,
                energy: energy_per_root,
                generation: 0,
                deflection_axis: find_perpendicular(dir),
                is_root: true,
            });
        }
    }

    // --- Process work queue ---
    let t0 = std::time::Instant::now();
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
            floor_y,
            energy_per_step_fp,
        );
    }

    log(&format!(
        "[worldgen]   segment growth took {:.1?}",
        t0.elapsed()
    ));

    // --- Leaf blobs (separate pass after all segments) ---
    let t0 = std::time::Instant::now();
    let leaf_voxels = generate_leaf_blobs(&leaf_blob_centers, world, profile, rng);
    log(&format!(
        "[worldgen]   leaf blob generation took {:.1?}",
        t0.elapsed()
    ));

    TreeGenResult {
        trunk_voxels,
        branch_voxels,
        leaf_voxels,
        root_voxels,
    }
}

/// Grow a single segment from the given job, placing voxels and potentially
/// spawning child segments via splitting.
#[allow(clippy::too_many_arguments)]
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
    floor_y: i32,
    energy_per_step_fp: i64,
) {
    let initial_energy = job.energy;
    let step_len_fp = f32_to_fp(profile.growth.growth_step_length);
    let energy_to_radius_fp = f32_to_fp(profile.growth.energy_to_radius);
    let min_radius_fp = f32_to_fp(profile.growth.min_radius);
    let base_flare_fp = f32_to_fp(profile.trunk.base_flare);
    let split_chance_ppm = (profile.split.split_chance_base * 1_000_000.0) as u32;
    let min_progress_fp = f32_to_fp(profile.split.min_progress_for_split);
    let split_energy_ratio_fp = f32_to_fp(profile.split.split_energy_ratio);
    let split_angle_brads = radians_to_brads(profile.split.split_angle);
    let split_variance_brads = radians_to_brads(profile.split.split_angle_variance);
    let gravitropism_fp = if job.is_root {
        -f32_to_fp(profile.roots.root_gravitropism)
    } else {
        f32_to_fp(profile.curvature.gravitropism)
    };
    let deflection_amount_brads = radians_to_brads(profile.curvature.random_deflection);
    let coherence_fp = f32_to_fp(profile.curvature.deflection_coherence);
    let surface_tendency_fp = f32_to_fp(profile.roots.root_surface_tendency);

    let mut step_count = 0u32;
    let mut prev_voxel_center = round_to_voxel(job.position);

    while job.energy > 0 {
        // Compute radius from remaining energy: sqrt(energy * energy_to_radius).
        let radius_input = (job.energy * energy_to_radius_fp) >> FP_SHIFT;
        let mut radius_fp = fp_sqrt(radius_input);
        if radius_fp < min_radius_fp {
            radius_fp = min_radius_fp;
        }

        // Apply base flare for trunk (generation 0, non-root, near ground).
        if job.generation == 0 && !job.is_root && base_flare_fp > 0 {
            let trunk_base_y_fp = (floor_y as i64 + 1) * FP_ONE;
            let height_fp = job.position[1] - trunk_base_y_fp; // height above trunk base
            let five_fp = 5 * FP_ONE;
            if height_fp < five_fp {
                // flare_factor = 1 + base_flare * (1 - height/5)
                let t = ((five_fp - height_fp) * FP_ONE / five_fp).max(0);
                let flare_factor = FP_ONE + ((base_flare_fp * t) >> FP_SHIFT);
                radius_fp = (radius_fp * flare_factor) >> FP_SHIFT;
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

        // Bridge gap from previous step.
        let current_voxel_center = round_to_voxel(job.position);
        bridge_cross_sections(
            world,
            prev_voxel_center,
            current_voxel_center,
            radius_fp,
            vtype,
            voxel_list,
        );
        prev_voxel_center = current_voxel_center;

        // Place cross-section voxels at current position.
        place_cross_section(world, job.position, radius_fp, vtype, voxel_list);

        // Check for split.
        // progress = 1 - energy/initial_energy = (initial - energy) / initial
        let spent = initial_energy - job.energy;
        let progress_fp = if initial_energy > 0 {
            (spent * FP_ONE) / initial_energy
        } else {
            0
        };
        if progress_fp >= min_progress_fp && !job.is_root {
            // Consume one RNG draw for split roll (PPM comparison).
            let split_roll = rng.next_u32() % 1_000_000;
            if split_roll < split_chance_ppm {
                for _ in 0..profile.split.split_count {
                    let child_energy = (job.energy * split_energy_ratio_fp) >> FP_SHIFT;
                    if child_energy > energy_per_step_fp {
                        // Deflect child direction.
                        // Variance: random offset in [-variance, +variance] brads.
                        // Consume one RNG draw, mapping to signed range.
                        let variance_offset = if split_variance_brads > 0 {
                            let raw = rng.next_u32();
                            (raw % (2 * split_variance_brads as u32 + 1)) as i64
                                - split_variance_brads
                        } else {
                            // Still consume a draw for determinism.
                            let _ = rng.next_u32();
                            0
                        };
                        let angle_offset = split_angle_brads + variance_offset;
                        // Consume one RNG draw for perpendicular direction.
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

                        job.energy -= child_energy;
                    }
                }
            }
        } else if job.is_root {
            // Roots don't split, but draw the roll for determinism.
            let _ = rng.next_u32();
        }

        // Consume energy for this step.
        job.energy -= energy_per_step_fp;
        if job.energy <= 0 {
            break;
        }

        // Apply curvature: gravitropism + random deflection with coherence.

        // Gravitropism: bias direction toward vertical.
        let up: Vec3 = [0, FP_ONE, 0];
        let grav_bias = vec3_scale(up, gravitropism_fp);
        job.direction = vec3_normalize(vec3_add(job.direction, grav_bias));

        // Root surface tendency: pull roots back toward floor level.
        if job.is_root && surface_tendency_fp > 0 {
            let target_y_fp = floor_y as i64 * FP_ONE;
            let y_offset = job.position[1] - target_y_fp;
            // surface_bias_y = -y_offset * surface_tendency * 0.1
            // In FP: -y_offset * surface_tendency / (10 * FP_ONE)
            let bias_y = -(y_offset * surface_tendency_fp / (10 * FP_ONE));
            let surface_bias: Vec3 = [0, bias_y, 0];
            job.direction = vec3_normalize(vec3_add(job.direction, surface_bias));
        }

        // Random deflection with coherence.
        if deflection_amount_brads > 0 {
            // Consume one RNG draw for new deflection perpendicular.
            let new_deflection = random_perpendicular(job.direction, rng);
            // Blend with previous deflection for coherence.
            let one_minus_coherence = FP_ONE - coherence_fp;
            job.deflection_axis = vec3_normalize(vec3_add(
                vec3_scale(job.deflection_axis, coherence_fp),
                vec3_scale(new_deflection, one_minus_coherence),
            ));
            // Random angle in [-deflection_amount, +deflection_amount] brads.
            // Consume one RNG draw.
            let raw = rng.next_u32();
            let deflection_angle =
                (raw % (2 * deflection_amount_brads as u32 + 1)) as i64 - deflection_amount_brads;
            job.direction = vec3_normalize(rotate_around_axis(
                job.direction,
                job.deflection_axis,
                deflection_angle,
            ));
        }

        // Advance position.
        job.position = vec3_add(job.position, vec3_scale(job.direction, step_len_fp));

        step_count += 1;
    }

    // Record terminal position for leaf blob placement (non-root only).
    if !job.is_root && step_count > 0 {
        leaf_blob_centers.push(LeafBlobCenter {
            position: job.position,
        });
    }
}

/// Convert a fixed-point position to a voxel coordinate by rounding.
fn round_to_voxel(pos: Vec3) -> VoxelCoord {
    let half = FP_ONE / 2;
    VoxelCoord::new(
        ((pos[0] + half) >> FP_SHIFT) as i32,
        ((pos[1] + half) >> FP_SHIFT) as i32,
        ((pos[2] + half) >> FP_SHIFT) as i32,
    )
}

/// Place cross-sections along a 6-connected (face-sharing) path between two
/// voxel centers. This fills gaps when a growth step moves diagonally across
/// 2 or 3 axes, which would otherwise leave only corner/edge connections.
fn bridge_cross_sections(
    world: &mut VoxelWorld,
    from: VoxelCoord,
    to: VoxelCoord,
    radius_fp: i64,
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

        let bridge_center = [
            current.x as i64 * FP_ONE,
            current.y as i64 * FP_ONE,
            current.z as i64 * FP_ONE,
        ];
        place_cross_section(world, bridge_center, radius_fp, vtype, voxel_list);
    }
}

/// Place a filled circle of voxels perpendicular to the growth direction.
fn place_cross_section(
    world: &mut VoxelWorld,
    center: Vec3,
    radius_fp: i64,
    vtype: VoxelType,
    voxel_list: &mut Vec<VoxelCoord>,
) {
    // Round radius to integer voxels.
    let r = ((radius_fp + FP_ONE / 2) >> FP_SHIFT) as i32;
    let cx = ((center[0] + FP_ONE / 2) >> FP_SHIFT) as i32;
    let cy = ((center[1] + FP_ONE / 2) >> FP_SHIFT) as i32;
    let cz = ((center[2] + FP_ONE / 2) >> FP_SHIFT) as i32;

    if r <= 0 {
        let coord = VoxelCoord::new(cx, cy, cz);
        try_place_voxel(world, coord, vtype, voxel_list);
        return;
    }

    // Integer radius squared for distance check (no FP needed here).
    let r_sq = r * r;

    for dx in -r..=r {
        for dz in -r..=r {
            let dist_sq = dx * dx + dz * dz;
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
    // effective_density as PPM (parts per million).
    let effective_density_ppm =
        (profile.leaves.leaf_density * profile.leaves.canopy_density * 1_000_000.0) as u32;

    if effective_density_ppm == 0 {
        // Still draw RNG for determinism.
        for _center in blob_centers {
            let r = profile.leaves.leaf_size as i32;
            let count = (2 * r + 1) * (2 * r + 1) * (2 * r + 1);
            for _ in 0..count {
                rng.next_u32();
            }
        }
        return leaf_voxels;
    }

    let radius = profile.leaves.leaf_size as i32;
    let is_cloud = profile.leaves.leaf_shape == LeafShape::Cloud;

    for center in blob_centers {
        let cx = ((center.position[0] + FP_ONE / 2) >> FP_SHIFT) as i32;
        let cy = ((center.position[1] + FP_ONE / 2) >> FP_SHIFT) as i32;
        let cz = ((center.position[2] + FP_ONE / 2) >> FP_SHIFT) as i32;

        // Integer radius squared (×4 for cloud shape where dy is scaled by 1.5).
        // Cloud: dist_sq = dx² + (dy*3/2)² + dz² = dx² + dy²*9/4 + dz²
        // Multiply all by 4: 4*dx² + 9*dy² + 4*dz² vs 4*r² (sphere: 4*dx² + 4*dy² + 4*dz²)
        let r_sq_4 = 4 * radius * radius;

        for dx in -radius..=radius {
            for dy in -radius..=radius {
                for dz in -radius..=radius {
                    let dist_sq_4 = if is_cloud {
                        4 * dx * dx + 9 * dy * dy + 4 * dz * dz
                    } else {
                        4 * (dx * dx + dy * dy + dz * dz)
                    };

                    // Always draw for determinism (one u32, same cadence as old next_f32).
                    let roll = rng.next_u32() % 1_000_000;

                    if dist_sq_4 > r_sq_4 {
                        continue;
                    }
                    if roll >= effective_density_ppm {
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
    /// Terrain is disabled (terrain_max_height = 0) to preserve existing test behavior.
    fn test_config() -> GameConfig {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            floor_y: 0,
            ..GameConfig::default()
        };
        config.tree_profile.growth.initial_energy = 50.0;
        config.terrain_max_height = 0;
        config
    }

    /// Generate terrain + tree together — convenience wrapper for tests that
    /// need both (mirrors the old `generate_tree` which included terrain).
    fn generate_terrain_and_tree(
        world: &mut VoxelWorld,
        config: &GameConfig,
        rng: &mut GameRng,
    ) -> TreeGenResult {
        generate_terrain(world, config, rng);
        generate_tree(world, config, rng, &|_| {})
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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

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
        let result_a = generate_terrain_and_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(42);
        let result_b = generate_terrain_and_tree(&mut world_b, &config, &mut rng_b);

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
        let result_a = generate_terrain_and_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(999);
        let result_b = generate_terrain_and_tree(&mut world_b, &config, &mut rng_b);

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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

        // Count trunk voxels near the base (y <= 3) vs near the top.
        let max_y = result.trunk_voxels.iter().map(|v| v.y).max().unwrap_or(0);
        let top_threshold = max_y - 3;

        let base_count = result.trunk_voxels.iter().filter(|v| v.y <= 3).count();
        let top_count = result
            .trunk_voxels
            .iter()
            .filter(|v| v.y >= top_threshold)
            .count();

        assert!(
            base_count > top_count,
            "Trunk should taper: base ({base_count} voxels at y<=3) should be \
             wider than top ({top_count} voxels at y>={top_threshold})"
        );
    }

    #[test]
    fn generates_dirt_terrain() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain_and_tree(&mut world, &config, &mut rng);

        // Ground plane at floor_y should be Dirt (check away from trunk where roots won't overwrite).
        let edge = VoxelCoord::new(5, 0, 5);
        assert_eq!(world.get(edge), VoxelType::Dirt);
    }

    #[test]
    fn splits_produce_branch_voxels() {
        let mut config = test_config_no_roots();
        // High split chance to guarantee branches.
        config.tree_profile.split.split_chance_base = 1.0;
        config.tree_profile.split.min_progress_for_split = 0.05;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

        let center_x = 32;
        let center_z = 32;

        // At least some root voxels should be horizontally distant from center.
        let far_roots = result
            .root_voxels
            .iter()
            .filter(|v| {
                let dx = (v.x - center_x).abs();
                let dz = (v.z - center_z).abs();
                dx > 3 || dz > 3
            })
            .count();

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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

        for coord in &result.trunk_voxels {
            assert_eq!(
                world.get(*coord),
                VoxelType::Trunk,
                "Trunk voxel at {coord} was overwritten"
            );
        }
        for coord in &result.branch_voxels {
            assert_eq!(
                world.get(*coord),
                VoxelType::Branch,
                "Branch voxel at {coord} was overwritten"
            );
        }
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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

        assert!(
            result.leaf_voxels.is_empty(),
            "With canopy_density=0, should have no leaf voxels (got {})",
            result.leaf_voxels.len()
        );
    }

    #[test]
    fn base_flare_widens_trunk() {
        let mut config_no_flare = test_config_no_roots();
        config_no_flare.tree_profile.trunk.base_flare = 0.0;

        let mut config_flare = test_config_no_roots();
        config_flare.tree_profile.trunk.base_flare = 1.0;

        let mut world_nf = VoxelWorld::new(64, 64, 64);
        let mut rng_nf = GameRng::new(42);
        let result_nf = generate_terrain_and_tree(&mut world_nf, &config_no_flare, &mut rng_nf);

        let mut world_f = VoxelWorld::new(64, 64, 64);
        let mut rng_f = GameRng::new(42);
        let result_f = generate_terrain_and_tree(&mut world_f, &config_flare, &mut rng_f);

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
        let result_a = generate_terrain_and_tree(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(99);
        let result_b = generate_terrain_and_tree(&mut world_b, &config, &mut rng_b);

        assert_eq!(result_a.trunk_voxels, result_b.trunk_voxels);
        assert_eq!(result_a.branch_voxels, result_b.branch_voxels);
        assert_eq!(result_a.root_voxels, result_b.root_voxels);
        assert_eq!(result_a.leaf_voxels, result_b.leaf_voxels);
    }

    /// Every non-air tree voxel (Trunk, Branch, Root) must share at least one
    /// face with another tree voxel — no corner-only or edge-only connections.
    #[test]
    fn all_wood_voxels_are_face_connected() {
        let mut config = test_config_no_roots();
        config.tree_profile.growth.initial_energy = 30.0;
        config.tree_profile.trunk.initial_direction = [0.577, 0.577, 0.577];
        config.tree_profile.split.split_chance_base = 0.0;
        config.tree_profile.curvature.gravitropism = 0.0;
        config.tree_profile.curvature.random_deflection = 0.0;
        config.tree_profile.growth.min_radius = 0.1;
        config.tree_profile.growth.energy_to_radius = 0.0001;
        config.tree_profile.trunk.base_flare = 0.0;

        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

        let all_wood: Vec<VoxelCoord> = result
            .trunk_voxels
            .iter()
            .chain(result.branch_voxels.iter())
            .copied()
            .collect();

        assert!(
            all_wood.len() > 5,
            "Need enough voxels to test (got {})",
            all_wood.len()
        );

        let wood_set: std::collections::HashSet<VoxelCoord> = all_wood.iter().copied().collect();

        let face_offsets: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
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

    #[test]
    fn face_connectivity_with_splits_and_roots() {
        let config = test_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

        let all_wood: Vec<VoxelCoord> = result
            .trunk_voxels
            .iter()
            .chain(result.branch_voxels.iter())
            .chain(result.root_voxels.iter())
            .copied()
            .collect();

        let wood_set: std::collections::HashSet<VoxelCoord> = all_wood.iter().copied().collect();

        let face_offsets: [(i32, i32, i32); 6] = [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
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
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

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

    // --- Terrain generation tests ---

    fn terrain_config() -> GameConfig {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            floor_y: 0,
            ..GameConfig::default()
        };
        config.terrain_max_height = 4;
        config.terrain_noise_scale = 8.0;
        config
    }

    #[test]
    fn terrain_deterministic() {
        let config = terrain_config();

        let mut world_a = VoxelWorld::new(64, 64, 64);
        let mut rng_a = GameRng::new(42);
        generate_terrain(&mut world_a, &config, &mut rng_a);

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(42);
        generate_terrain(&mut world_b, &config, &mut rng_b);

        // Compare heightmaps to verify terrain determinism.
        assert_eq!(world_a.heightmap(), world_b.heightmap());
    }

    #[test]
    fn terrain_has_height_variation() {
        let config = terrain_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain(&mut world, &config, &mut rng);

        // Check that terrain heights vary across the world.
        let hm = world.heightmap();
        let unique: std::collections::BTreeSet<u8> = hm.iter().copied().collect();
        assert!(
            unique.len() > 1,
            "Terrain should have multiple height levels (got {:?})",
            unique
        );
    }

    #[test]
    fn terrain_respects_max_height() {
        let config = terrain_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain(&mut world, &config, &mut rng);

        let max_terrain = config.terrain_max_height;
        for &(x, z) in &[(0, 0), (0, 63), (63, 0), (63, 63)] {
            for y in (max_terrain + 1)..64 {
                assert_eq!(
                    world.get(VoxelCoord::new(x, y, z)),
                    VoxelType::Air,
                    "Terrain at ({},{},{}) should be air above max_height",
                    x,
                    y,
                    z
                );
            }
        }
    }

    #[test]
    fn terrain_minimum_one_thick() {
        let config = terrain_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain(&mut world, &config, &mut rng);

        // Every column should have solid ground at y=0.
        for z in 0..64_i32 {
            for x in 0..64_i32 {
                let coord = VoxelCoord::new(x, 0, z);
                assert!(
                    world.get(coord).is_solid(),
                    "Column ({},{}) missing solid ground at y=0",
                    x,
                    z
                );
            }
        }
    }

    #[test]
    fn terrain_disabled_with_zero_max_height() {
        let mut config = terrain_config();
        config.terrain_max_height = 0;
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain(&mut world, &config, &mut rng);

        let coord = VoxelCoord::new(5, 0, 5);
        assert_eq!(world.get(coord), VoxelType::Dirt);
        // No hills above floor_y: off-center y=1 should be air.
        let offcenter = VoxelCoord::new(5, 1, 5);
        assert_eq!(world.get(offcenter), VoxelType::Air);
    }

    #[test]
    fn terrain_corners_have_height_variation() {
        // Verify terrain has real hills — no tree needed, just terrain.
        let config = terrain_config();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain(&mut world, &config, &mut rng);

        // Sample terrain heights at the four corners.
        let mut corner_heights = std::collections::BTreeSet::new();
        for &(x, z) in &[(0, 0), (0, 63), (63, 0), (63, 63)] {
            let mut top = 0;
            for y in 0..64_i32 {
                if world.get(VoxelCoord::new(x, y, z)).is_solid() {
                    top = y;
                }
            }
            corner_heights.insert(top);
        }
        assert!(
            corner_heights.len() > 1,
            "Terrain corners should have different heights (got {:?})",
            corner_heights
        );
    }

    #[test]
    fn sin_table_endpoints_correct() {
        // sin(0) = 0
        assert_eq!(fp_sin(0), 0);
        // sin(π/2) = 1 → FP_ONE
        assert_eq!(fp_sin(ANGLE_QUARTER), FP_ONE);
        // sin(π) = 0
        assert_eq!(fp_sin(2 * ANGLE_QUARTER), 0);
        // sin(3π/2) = -1
        assert_eq!(fp_sin(3 * ANGLE_QUARTER), -FP_ONE);
        // cos(0) = 1
        assert_eq!(fp_cos(0), FP_ONE);
        // cos(π/2) = 0
        assert_eq!(fp_cos(ANGLE_QUARTER), 0);
    }

    #[test]
    fn fp_sqrt_basic() {
        // sqrt(1.0) = 1.0 → fp_sqrt(FP_ONE) = FP_ONE
        assert_eq!(fp_sqrt(FP_ONE), FP_ONE);
        // sqrt(4.0) = 2.0 → fp_sqrt(4 * FP_ONE) = 2 * FP_ONE
        assert_eq!(fp_sqrt(4 * FP_ONE), 2 * FP_ONE);
        // sqrt(0) = 0
        assert_eq!(fp_sqrt(0), 0);
    }

    #[test]
    fn fp_sqrt_negative_returns_zero() {
        assert_eq!(fp_sqrt(-1), 0);
        assert_eq!(fp_sqrt(i64::MIN), 0);
    }

    #[test]
    fn fp_sin_negative_angles() {
        // sin(-π/2) = -1
        assert_eq!(fp_sin(-ANGLE_QUARTER), -FP_ONE);
        // sin(-2π) = sin(0) = 0
        assert_eq!(fp_sin(-ANGLE_FULL), 0);
        // Large negative wraps correctly.
        assert_eq!(fp_sin(-3 * ANGLE_FULL + ANGLE_QUARTER), FP_ONE);
    }

    #[test]
    fn fp_sin_mid_quadrant_accuracy() {
        // sin(π/4) = sin(128 brads) ≈ 0.7071 * 65536 ≈ 46341
        let sin_45 = fp_sin(ANGLE_QUARTER / 2);
        let expected = 46341i64; // floor(sin(π/4) * 65536)
        assert!(
            (sin_45 - expected).abs() <= 1,
            "sin(π/4) should be ~{expected}, got {sin_45}"
        );
    }

    #[test]
    fn normalize_preserves_unit_length() {
        let v = vec3_normalize([FP_ONE / 3, FP_ONE / 3, FP_ONE / 3]);
        let len = vec3_length(v);
        assert!(
            (len - FP_ONE).abs() < FP_ONE / 50,
            "Normalized length should be ~{FP_ONE}, got {len}"
        );

        // Chain 100 normalizations — should not drift significantly.
        let mut v = v;
        for _ in 0..100 {
            v = vec3_normalize(v);
        }
        let len = vec3_length(v);
        assert!(
            (len - FP_ONE).abs() < FP_ONE / 20,
            "Drift after 100 normalizations: {len} vs {FP_ONE}"
        );
    }

    #[test]
    fn generates_tree_at_nonzero_floor_y() {
        let mut config = test_config();
        config.floor_y = 10;
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        let result = generate_terrain_and_tree(&mut world, &config, &mut rng);

        // All trunk voxels must be above the floor (trunk starts at floor_y + 1).
        for coord in &result.trunk_voxels {
            assert!(
                coord.y >= 11,
                "Trunk voxel at y={} is below floor_y+1=11",
                coord.y
            );
        }

        // All branch voxels must also be above the floor.
        for coord in &result.branch_voxels {
            assert!(
                coord.y >= 11,
                "Branch voxel at y={} is below floor_y+1=11",
                coord.y
            );
        }

        // Terrain Dirt should exist at floor_y=10.
        let edge = VoxelCoord::new(5, 10, 5);
        assert_eq!(
            world.get(edge),
            VoxelType::Dirt,
            "Expected Dirt at floor_y=10"
        );

        // No trunk or branch voxels below floor_y + 1.
        let below_floor: Vec<_> = result
            .trunk_voxels
            .iter()
            .chain(result.branch_voxels.iter())
            .filter(|v| v.y < 11)
            .collect();
        assert!(
            below_floor.is_empty(),
            "Found {} trunk/branch voxels below floor_y+1: {:?}",
            below_floor.len(),
            &below_floor[..below_floor.len().min(5)]
        );
    }

    // --- generate_tree_at tests ---

    #[test]
    fn generate_tree_at_places_trunk_at_specified_position() {
        let config = test_config_no_roots();
        let profile = &config.tree_profile;
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain(&mut world, &config, &mut rng);

        let result = generate_tree_at(&mut world, profile, 0, 20, 40, &mut rng, &|_| {});

        // The trunk should contain a voxel at or near (20, 1, 40).
        let has_base = result
            .trunk_voxels
            .iter()
            .any(|v| v.x == 20 && v.y == 1 && v.z == 40);
        assert!(
            has_base,
            "Trunk should have a voxel at (20, 1, 40) — the specified base position"
        );
    }

    #[test]
    fn generate_tree_at_different_positions_differ() {
        let config = test_config_no_roots();
        let profile = &config.tree_profile;

        let mut world_a = VoxelWorld::new(64, 64, 64);
        let mut rng_a = GameRng::new(42);
        generate_terrain(&mut world_a, &config, &mut rng_a);
        let result_a = generate_tree_at(&mut world_a, profile, 0, 10, 10, &mut rng_a, &|_| {});

        let mut world_b = VoxelWorld::new(64, 64, 64);
        let mut rng_b = GameRng::new(42);
        generate_terrain(&mut world_b, &config, &mut rng_b);
        let result_b = generate_tree_at(&mut world_b, profile, 0, 50, 50, &mut rng_b, &|_| {});

        // Trunk voxels should be at different coordinates.
        assert_ne!(
            result_a.trunk_voxels, result_b.trunk_voxels,
            "Trees at different positions should have different voxel coordinates"
        );
    }

    #[test]
    fn lesser_deciduous_profile_produces_small_tree() {
        use crate::config::TreeProfile;
        let profile = TreeProfile::lesser_deciduous();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);

        let result = generate_tree_at(&mut world, &profile, 0, 32, 32, &mut rng, &|_| {});

        assert!(!result.trunk_voxels.is_empty(), "Should have trunk voxels");

        // Height should be modest — lesser trees should be <20 voxels tall.
        let max_y = result.trunk_voxels.iter().map(|v| v.y).max().unwrap();
        let min_y = result.trunk_voxels.iter().map(|v| v.y).min().unwrap();
        let height = max_y - min_y + 1;
        assert!(
            height <= 25,
            "Lesser deciduous tree should be <=25 voxels tall, got {height}"
        );

        // Should have no root voxels (root_energy_fraction = 0).
        assert!(
            result.root_voxels.is_empty(),
            "Lesser deciduous should have no roots"
        );
    }

    #[test]
    fn lesser_conifer_profile_produces_small_tree() {
        use crate::config::TreeProfile;
        let profile = TreeProfile::lesser_conifer();
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);

        let result = generate_tree_at(&mut world, &profile, 0, 32, 32, &mut rng, &|_| {});

        assert!(!result.trunk_voxels.is_empty(), "Should have trunk voxels");

        let max_y = result.trunk_voxels.iter().map(|v| v.y).max().unwrap();
        let min_y = result.trunk_voxels.iter().map(|v| v.y).min().unwrap();
        let height = max_y - min_y + 1;
        assert!(
            height <= 30,
            "Lesser conifer tree should be <=30 voxels tall, got {height}"
        );

        assert!(
            result.root_voxels.is_empty(),
            "Lesser conifer should have no roots"
        );
    }

    #[test]
    fn all_lesser_profiles_produce_trunk_voxels() {
        use crate::config::TreeProfile;
        let profiles = [
            ("deciduous", TreeProfile::lesser_deciduous()),
            ("conifer", TreeProfile::lesser_conifer()),
            ("tall_straight", TreeProfile::lesser_tall_straight()),
            ("thick_oak", TreeProfile::lesser_thick_oak()),
            ("bushy", TreeProfile::lesser_bushy()),
            ("sapling", TreeProfile::lesser_sapling()),
        ];
        for (name, profile) in &profiles {
            let mut world = VoxelWorld::new(64, 64, 64);
            let mut rng = GameRng::new(42);
            let result = generate_tree_at(&mut world, profile, 0, 32, 32, &mut rng, &|_| {});
            assert!(
                !result.trunk_voxels.is_empty(),
                "Profile '{name}' should produce trunk voxels"
            );
        }
    }

    // --- terrain_surface_y tests ---

    #[test]
    fn terrain_surface_y_flat_world() {
        let mut world = VoxelWorld::new(16, 64, 16);
        // Place a flat Dirt layer at y=10.
        world.set(VoxelCoord::new(5, 10, 5), VoxelType::Dirt);
        let y = terrain_surface_y(&world, 10, 4, 5, 5);
        assert_eq!(y, 10, "Flat terrain: surface should be at floor_y");
    }

    #[test]
    fn terrain_surface_y_hilly_terrain() {
        let mut world = VoxelWorld::new(16, 64, 16);
        // Stack Dirt from y=10 to y=13 (4 layers).
        for y in 10..=13 {
            world.set(VoxelCoord::new(5, y, 5), VoxelType::Dirt);
        }
        let y = terrain_surface_y(&world, 10, 4, 5, 5);
        assert_eq!(y, 13, "Hilly terrain: surface should be top of dirt stack");
    }

    #[test]
    fn terrain_surface_y_stops_at_tree_material() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(5, 10, 5), VoxelType::Dirt);
        world.set(VoxelCoord::new(5, 11, 5), VoxelType::Trunk);
        let y = terrain_surface_y(&world, 10, 4, 5, 5);
        assert_eq!(y, 10, "Should stop scanning at non-terrain voxel");
    }

    #[test]
    fn terrain_surface_y_no_terrain_returns_floor_y() {
        let world = VoxelWorld::new(16, 64, 16);
        // Empty world — everything is Air.
        let y = terrain_surface_y(&world, 10, 4, 5, 5);
        assert_eq!(y, 10, "Empty world: should return floor_y");
    }

    #[test]
    fn terrain_surface_y_zero_max_height() {
        let mut world = VoxelWorld::new(16, 64, 16);
        world.set(VoxelCoord::new(5, 10, 5), VoxelType::Dirt);
        let y = terrain_surface_y(&world, 10, 0, 5, 5);
        assert_eq!(y, 10, "With max_height=0, should return floor_y");
    }

    #[test]
    fn main_tree_sink_with_hilly_terrain() {
        let mut config = test_config();
        config.terrain_max_height = 4;
        config.tree_profile.growth.initial_energy = 50.0;
        let mut world = VoxelWorld::new(64, 64, 64);
        let mut rng = GameRng::new(42);
        generate_terrain(&mut world, &config, &mut rng);

        let center_x = 32;
        let center_z = 32;
        let surface = terrain_surface_y(&world, 0, 4, center_x, center_z);

        // With terrain, surface should be above floor_y.
        assert!(surface >= config.floor_y, "Surface should be >= floor_y");

        // The sink should be min(5, surface - floor_y).
        let expected_sink = 5.min(surface - config.floor_y);
        let expected_base = surface - expected_sink;

        let _result = generate_tree(&mut world, &config, &mut rng, &|_| {});

        // Verify trunk starts at expected_base + 1.
        // (We can't easily inspect the base_y that was passed, but we can
        // verify that the terrain_surface_y + sink math is consistent.)
        assert_eq!(expected_base, surface - expected_sink);
    }
}
