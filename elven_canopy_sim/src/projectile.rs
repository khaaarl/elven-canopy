// Projectile ballistics — integer-only deterministic trajectory simulation.
//
// Implements sub-voxel coordinate types and ballistic trajectory computation
// for arrows (and future projectile types). All arithmetic is integer-only
// to maintain the sim's determinism guarantees — no floating-point operations
// in any code path that affects sim state.
//
// ## Sub-voxel coordinate system
//
// Projectile positions use `SubVoxelCoord` (an alias for `FixedVec3` from
// `elven_canopy_utils::fixed`) with 2^30 sub-units per voxel (~1.07 billion),
// stored as `i64` per axis. This precision ensures that gravity and velocity
// accumulation over long flight times does not lose significant bits.
// Velocity and acceleration use the same scale (sub-units per tick and
// sub-units per tick², respectively).
//
// ## Physical scale
//
// Each voxel is 2m × 2m × 2m (see `docs/design_doc.md` §Voxel Grid).
// Earth gravity (9.81 m/s²) in voxel units is 4.905 voxels/s². At 1000
// ticks/sec with 2^30 sub-units per voxel, gravity is:
//   4.905 / 1_000_000 × 2^30 ≈ 5267 sub-units per tick²
//
// ## Integration method
//
// Symplectic Euler: velocity is updated before position each tick. This is
// more stable than standard Euler for ballistic trajectories and must not be
// reordered. See `docs/drafts/combat_military.md` §4 for design rationale.
//
// ## Aim computation
//
// `compute_aim_velocity()` uses iterative guess-and-simulate to find a
// launch velocity that sends a projectile from shooter to target. This
// avoids floating-point trig/sqrt while naturally handling all edge cases
// (uphill, downhill, obstructed arcs). See §5.2 of the combat draft.
//
// ## Sim integration
//
// The `Projectile` entity table lives in `db.rs`. Collision detection (solid
// voxels, creature hits via spatial index), the `ProjectileTick` batched
// event, and projectile spawn/despawn/impact logic live in `sim/combat.rs`
// (`spawn_projectile`, `process_projectile_tick`, `resolve_projectile_*`).
// This module provides the pure math: trajectory stepping and aim solving.
//
// See also: `docs/drafts/combat_military.md` §4–§5, `types.rs` for
// `VoxelCoord`, `config.rs` for `GameConfig` (arrow_gravity, arrow_base_speed).

use crate::types::VoxelCoord;
use elven_canopy_utils::fixed::{FRAC_ONE, FRAC_SHIFT, FixedVec3, isqrt_i128};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of sub-voxel units per voxel (2^30 ≈ 1.07 billion).
/// Re-exported from the shared fixed-point library for domain clarity.
pub const SUB_VOXEL_SHIFT: u32 = FRAC_SHIFT;

/// Convenience: 1 voxel in sub-voxel units.
pub const SUB_VOXEL_ONE: i64 = FRAC_ONE;

/// Earth gravity in sub-voxel units per tick².
///
/// Derivation: 1 voxel = 2 meters. g = 9.81 m/s² = 4.905 voxels/s².
/// At 1000 ticks/sec: 4.905 / 1_000_000 × 2^30 = 5266.7... ≈ 5267.
///
/// This is the reference value used throughout the codebase. Config
/// `arrow_gravity` defaults to this but can be overridden for gameplay tuning.
pub const EARTH_GRAVITY_SUB_VOXEL: i64 = 5267;

// ---------------------------------------------------------------------------
// Sub-voxel coordinate types
// ---------------------------------------------------------------------------

/// High-precision integer position for projectiles. Each axis stores
/// position in sub-voxel units (2^30 per voxel).
///
/// This is an alias for `FixedVec3` from `elven_canopy_utils::fixed`.
/// Arithmetic operators (Add, AddAssign, Sub), magnitude, and serde are
/// provided by the library type. Voxel-specific conversions (to/from
/// `VoxelCoord`) are free functions below.
pub type SubVoxelCoord = FixedVec3;

/// Velocity or acceleration vector in sub-voxel units per tick (or per tick²).
/// Same representation as `SubVoxelCoord`, different semantics.
pub type SubVoxelVec = FixedVec3;

// ---------------------------------------------------------------------------
// VoxelCoord conversions (sim-specific, depend on crate::types::VoxelCoord)
// ---------------------------------------------------------------------------

/// Create a sub-voxel coordinate from a voxel coordinate, placing the point
/// at the center of the voxel (offset by half a voxel in each axis).
pub const fn sub_voxel_from_voxel_center(v: VoxelCoord) -> SubVoxelCoord {
    SubVoxelCoord {
        x: (v.x as i64) * SUB_VOXEL_ONE + SUB_VOXEL_ONE / 2,
        y: (v.y as i64) * SUB_VOXEL_ONE + SUB_VOXEL_ONE / 2,
        z: (v.z as i64) * SUB_VOXEL_ONE + SUB_VOXEL_ONE / 2,
    }
}

/// Extract the containing voxel coordinate via arithmetic right-shift.
///
/// Rust guarantees arithmetic right-shift for signed integers (rounds
/// toward negative infinity), so this correctly maps negative sub-voxel
/// coordinates to the containing voxel.
///
/// **SAFETY PRECONDITION:** The caller must ensure the sub-voxel
/// coordinates, when right-shifted by `SUB_VOXEL_SHIFT`, fit in `i32`.
/// For projectiles, this means bounds-checking the raw `i64` sub-voxel
/// position against world extents *before* calling this function. Use
/// `sub_voxel_to_voxel_checked()` if you need a fallible version.
/// Violating this precondition causes silent `i64 → i32` truncation,
/// which can map far-out-of-bounds positions to apparently-valid coordinates.
pub const fn sub_voxel_to_voxel(sv: SubVoxelCoord) -> VoxelCoord {
    VoxelCoord {
        x: (sv.x >> SUB_VOXEL_SHIFT) as i32,
        y: (sv.y >> SUB_VOXEL_SHIFT) as i32,
        z: (sv.z >> SUB_VOXEL_SHIFT) as i32,
    }
}

/// Checked version of `sub_voxel_to_voxel()`. Returns `None` if any axis,
/// after right-shifting, would overflow `i32`.
pub fn sub_voxel_to_voxel_checked(sv: SubVoxelCoord) -> Option<VoxelCoord> {
    let x = sv.x >> SUB_VOXEL_SHIFT;
    let y = sv.y >> SUB_VOXEL_SHIFT;
    let z = sv.z >> SUB_VOXEL_SHIFT;
    if x < i32::MIN as i64
        || x > i32::MAX as i64
        || y < i32::MIN as i64
        || y > i32::MAX as i64
        || z < i32::MIN as i64
        || z > i32::MAX as i64
    {
        return None;
    }
    Some(VoxelCoord {
        x: x as i32,
        y: y as i32,
        z: z as i32,
    })
}

/// Convert sub-voxel coordinates to floating-point for rendering.
/// NOT for sim logic — uses the `to_render_floats()` method on `FixedVec3`.
pub fn sub_voxel_to_render_floats(sv: SubVoxelCoord) -> (f32, f32, f32) {
    sv.to_render_floats()
}

// ---------------------------------------------------------------------------
// Ballistic trajectory step
// ---------------------------------------------------------------------------

/// Advance a projectile by one tick using symplectic Euler integration.
///
/// Updates velocity first (gravity), then position. This ordering is
/// critical — do not reorder. Returns the new (position, velocity).
///
/// `gravity` is in sub-voxel units per tick² (positive value = downward).
pub fn ballistic_step(
    position: SubVoxelCoord,
    velocity: SubVoxelVec,
    gravity: i64,
) -> (SubVoxelCoord, SubVoxelVec) {
    // Step 1: apply gravity to velocity (symplectic Euler: v before p)
    let new_velocity = SubVoxelVec {
        x: velocity.x,
        y: velocity.y - gravity,
        z: velocity.z,
    };
    // Step 2: apply velocity to position
    let new_position = position + new_velocity;
    (new_position, new_velocity)
}

/// Simulate a full ballistic trajectory from launch to either hitting a
/// target voxel or exceeding `max_ticks`. Returns the tick number at which
/// the projectile enters the target voxel, or `None` if it never does.
///
/// `tolerance_voxels` widens the hit check: the projectile is considered
/// to hit if it's within this many voxels of the target in each axis
/// (Chebyshev distance).
///
/// This does NOT check for solid-voxel collisions or world bounds — it's
/// a pure ballistic arc in free space, used by the aim solver.
pub fn simulate_trajectory(
    start: SubVoxelCoord,
    velocity: SubVoxelVec,
    gravity: i64,
    target: VoxelCoord,
    tolerance_voxels: i32,
    max_ticks: u32,
) -> Option<u32> {
    let mut pos = start;
    let mut vel = velocity;

    for tick in 1..=max_ticks {
        (pos, vel) = ballistic_step(pos, vel, gravity);
        let voxel = sub_voxel_to_voxel(pos);

        if (voxel.x - target.x).abs() <= tolerance_voxels
            && (voxel.y - target.y).abs() <= tolerance_voxels
            && (voxel.z - target.z).abs() <= tolerance_voxels
        {
            return Some(tick);
        }

        // Early exit: if the projectile has fallen well below the target
        // and is still moving down, it will never reach the target.
        if voxel.y < target.y - tolerance_voxels - 5 && vel.y < 0 {
            return None;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Aim computation
// ---------------------------------------------------------------------------

/// Result of an aim computation.
#[derive(Clone, Debug)]
pub struct AimResult {
    /// The launch velocity vector (sub-voxel units per tick).
    pub velocity: SubVoxelVec,
    /// The tick at which the simulated trajectory reaches the target voxel,
    /// or `None` if the best guess doesn't actually reach it.
    pub hit_tick: Option<u32>,
    /// Number of adjustment iterations used.
    pub iterations_used: u32,
}

/// Compute a launch velocity to send a projectile from `origin` to `target`
/// with the given `speed` (magnitude in sub-voxel units per tick) under
/// `gravity` (sub-voxel units per tick², positive = downward).
///
/// Uses iterative guess-and-simulate (no floating-point trig/sqrt). The
/// algorithm:
/// 1. Compute the displacement vector from origin to target in sub-voxel
///    units.
/// 2. Estimate initial direction: aim directly at target, then apply a
///    first-order gravity compensation (raise the arc).
/// 3. Normalize the direction to the requested speed using integer
///    approximation (no sqrt — uses iterative refinement).
/// 4. Simulate the trajectory and check if it hits.
/// 5. If not, adjust the vertical component and retry.
///
/// `max_iterations`: maximum adjustment attempts (design default: 5).
/// `max_flight_ticks`: maximum ticks to simulate per attempt.
///
/// Returns the best velocity found. If no attempt hits the target exactly,
/// returns the closest attempt (the archer fires their best guess and may
/// miss — this is intentional per the design).
pub fn compute_aim_velocity(
    origin: SubVoxelCoord,
    target_voxel: VoxelCoord,
    speed: i64,
    gravity: i64,
    max_iterations: u32,
    max_flight_ticks: u32,
) -> AimResult {
    let target_center = sub_voxel_from_voxel_center(target_voxel);
    let dx = target_center.x - origin.x;
    let dy = target_center.y - origin.y;
    let dz = target_center.z - origin.z;

    // Estimate flight time in ticks: distance / speed.
    // distance ≈ sqrt(dx² + dy² + dz²). We approximate using Manhattan-ish
    // heuristic to avoid sqrt: use the largest axis component * ~1.2.
    let dist_approx = {
        let ax = dx.unsigned_abs();
        let ay = dy.unsigned_abs();
        let az = dz.unsigned_abs();
        let max_comp = ax.max(ay).max(az);
        let min_comp = ax.min(ay).min(az);
        let mid_comp = ax + ay + az - max_comp - min_comp;
        // Approximation: max + 0.4*mid + 0.3*min ≈ euclidean distance
        // Using integer: (10*max + 4*mid + 3*min) / 10
        (10 * max_comp + 4 * mid_comp + 3 * min_comp) / 10
    };

    let est_flight_ticks = if speed > 0 {
        (dist_approx / speed as u64).max(1)
    } else {
        return AimResult {
            velocity: SubVoxelVec::new(0, 0, 0),
            hit_tick: None,
            iterations_used: 0,
        };
    };

    // First-order gravity compensation: during flight, gravity pulls the
    // projectile down by approximately 0.5 * g * t² sub-voxel units.
    // We add this to the y-component of our aim direction.
    let gravity_drop = gravity as i128 * est_flight_ticks as i128 * est_flight_ticks as i128 / 2;

    // Base aim direction (sub-voxel displacement + gravity compensation)
    let aim_dy_base = dy as i128 + gravity_drop;

    let mut best_result: Option<AimResult> = None;

    // Try several vertical adjustments around the base aim
    for iteration in 0..max_iterations {
        // Adjust vertical aim: iteration 0 = base, then alternate above/below
        let adjustment = match iteration {
            0 => 0i128,
            n => {
                // Adjustment step size. When gravity_drop is small (close
                // targets or low gravity), step may be 0 — all iterations
                // try the same velocity. Harmless: the base aim (iteration 0)
                // is accurate for short-range/low-gravity shots.
                let step = gravity_drop / 4;
                let half = (n as i128 + 1) / 2;
                if n % 2 == 1 {
                    step * half
                } else {
                    -step * half
                }
            }
        };

        let aim_dy = aim_dy_base + adjustment;

        // Scale direction vector to the requested speed.
        // We need: velocity = direction_normalized * speed
        // direction = (dx, aim_dy, dz), but aim_dy is i128.
        //
        // To normalize without sqrt, compute magnitude² then use iterative
        // scaling: velocity = direction * speed / magnitude.
        // We compute magnitude via integer sqrt approximation.

        let dir_x = dx as i128;
        let dir_z = dz as i128;
        let mag_sq = dir_x * dir_x + aim_dy * aim_dy + dir_z * dir_z;

        if mag_sq == 0 {
            continue;
        }

        // Integer square root via Newton's method
        let mag = isqrt_i128(mag_sq);
        if mag == 0 {
            continue;
        }

        let vel = SubVoxelVec {
            x: (dir_x * speed as i128 / mag) as i64,
            y: (aim_dy * speed as i128 / mag) as i64,
            z: (dir_z * speed as i128 / mag) as i64,
        };

        // Simulate trajectory
        let hit_tick = simulate_trajectory(
            origin,
            vel,
            gravity,
            target_voxel,
            0, // exact voxel match
            max_flight_ticks,
        );

        let result = AimResult {
            velocity: vel,
            hit_tick,
            iterations_used: iteration + 1,
        };

        if hit_tick.is_some() {
            return result;
        }

        // Keep the first attempt as fallback (it has the base gravity comp)
        if best_result.is_none() {
            best_result = Some(result);
        }
    }

    // Also try with tolerance=1 on the first velocity if nothing hit exactly
    if let Some(ref mut best) = best_result
        && best.hit_tick.is_none()
    {
        best.hit_tick = simulate_trajectory(
            origin,
            best.velocity,
            gravity,
            target_voxel,
            1,
            max_flight_ticks,
        );
    }

    best_result.unwrap_or(AimResult {
        velocity: SubVoxelVec::new(0, 0, 0),
        hit_tick: None,
        iterations_used: max_iterations,
    })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // SubVoxelCoord basics
    // -----------------------------------------------------------------------

    #[test]
    fn test_sub_voxel_to_voxel_positive() {
        // Point at sub-voxel (1.5, 2.5, 3.5) in voxel units
        let pos = sub_voxel_from_voxel_center(VoxelCoord::new(1, 2, 3));
        let voxel = sub_voxel_to_voxel(pos);
        assert_eq!(voxel, VoxelCoord::new(1, 2, 3));
    }

    #[test]
    fn test_sub_voxel_to_voxel_negative() {
        // Arithmetic right-shift should map negative coords correctly.
        // A point just below voxel 0 should be in voxel -1.
        let pos = SubVoxelCoord::new(-1, -1, -1);
        let voxel = sub_voxel_to_voxel(pos);
        assert_eq!(voxel, VoxelCoord::new(-1, -1, -1));
    }

    #[test]
    fn test_sub_voxel_to_voxel_boundary() {
        // Exactly at the boundary between voxel 0 and voxel 1
        let pos = SubVoxelCoord::new(SUB_VOXEL_ONE, 0, 0);
        assert_eq!(sub_voxel_to_voxel(pos), VoxelCoord::new(1, 0, 0));

        // One sub-unit below the boundary
        let pos = SubVoxelCoord::new(SUB_VOXEL_ONE - 1, 0, 0);
        assert_eq!(sub_voxel_to_voxel(pos), VoxelCoord::new(0, 0, 0));
    }

    #[test]
    fn test_from_voxel_center() {
        let coord = sub_voxel_from_voxel_center(VoxelCoord::new(5, 10, 3));
        // Should be at (5.5, 10.5, 3.5) in voxel units
        assert_eq!(coord.x, 5 * SUB_VOXEL_ONE + SUB_VOXEL_ONE / 2);
        assert_eq!(coord.y, 10 * SUB_VOXEL_ONE + SUB_VOXEL_ONE / 2);
        assert_eq!(coord.z, 3 * SUB_VOXEL_ONE + SUB_VOXEL_ONE / 2);
        // Round-trip
        assert_eq!(sub_voxel_to_voxel(coord), VoxelCoord::new(5, 10, 3));
    }

    #[test]
    fn test_magnitude_sq() {
        let v = SubVoxelVec::new(SUB_VOXEL_ONE, 0, 0);
        assert_eq!(v.magnitude_sq(), (SUB_VOXEL_ONE as i128).pow(2));

        let v = SubVoxelVec::new(3, 4, 0);
        assert_eq!(v.magnitude_sq(), 25);
    }

    #[test]
    fn test_magnitude_sq_i64() {
        let v = SubVoxelVec::new(1000, 2000, 3000);
        let sq_i128 = v.magnitude_sq();
        let sq_i64 = v.magnitude_sq_i64();
        assert_eq!(sq_i128, sq_i64 as i128);
    }

    #[test]
    fn test_magnitude_sq_i64_realistic() {
        // Realistic arrow velocity: SUB_VOXEL_ONE / 20 per tick
        let speed = SUB_VOXEL_ONE / 20;
        let v = SubVoxelVec::new(speed, speed, speed);
        let sq = v.magnitude_sq_i64();
        // If the multiplication overflowed, sq would wrap negative.
        assert!(sq > 0);
    }

    #[test]
    fn test_to_voxel_checked_valid() {
        let pos = sub_voxel_from_voxel_center(VoxelCoord::new(10, 20, 30));
        assert_eq!(
            sub_voxel_to_voxel_checked(pos),
            Some(VoxelCoord::new(10, 20, 30))
        );
    }

    #[test]
    fn test_to_voxel_checked_negative() {
        let pos = SubVoxelCoord::new(-SUB_VOXEL_ONE, -SUB_VOXEL_ONE, -SUB_VOXEL_ONE);
        assert_eq!(
            sub_voxel_to_voxel_checked(pos),
            Some(VoxelCoord::new(-1, -1, -1))
        );
    }

    #[test]
    fn test_to_voxel_checked_overflow() {
        // Position so far out that the shifted value overflows i32
        let huge = (i32::MAX as i64 + 1) << SUB_VOXEL_SHIFT;
        let pos = SubVoxelCoord::new(huge, 0, 0);
        assert_eq!(sub_voxel_to_voxel_checked(pos), None);
    }

    #[test]
    fn test_to_voxel_checked_negative_overflow() {
        let huge_neg = ((i32::MIN as i64) - 1) << SUB_VOXEL_SHIFT;
        let pos = SubVoxelCoord::new(0, huge_neg, 0);
        assert_eq!(sub_voxel_to_voxel_checked(pos), None);
    }

    // -----------------------------------------------------------------------
    // Ballistic step
    // -----------------------------------------------------------------------

    #[test]
    fn test_ballistic_step_no_gravity() {
        let pos = SubVoxelCoord::new(0, 0, 0);
        let vel = SubVoxelVec::new(100, 200, 300);
        let (new_pos, new_vel) = ballistic_step(pos, vel, 0);
        // With no gravity, velocity should be unchanged
        assert_eq!(new_vel, vel);
        // Position should advance by velocity
        assert_eq!(new_pos, SubVoxelCoord::new(100, 200, 300));
    }

    #[test]
    fn test_ballistic_step_with_gravity() {
        let pos = SubVoxelCoord::new(0, SUB_VOXEL_ONE * 100, 0);
        let vel = SubVoxelVec::new(1000, 0, 0);
        let gravity = EARTH_GRAVITY_SUB_VOXEL;

        let (new_pos, new_vel) = ballistic_step(pos, vel, gravity);

        // Velocity y should decrease by gravity (symplectic: vel updated first)
        assert_eq!(new_vel.y, -gravity);
        assert_eq!(new_vel.x, 1000);

        // Position uses the NEW velocity (symplectic Euler)
        assert_eq!(new_pos.x, 1000);
        assert_eq!(new_pos.y, SUB_VOXEL_ONE * 100 - gravity);
    }

    #[test]
    fn test_symplectic_euler_ordering() {
        // Verify that velocity is applied AFTER gravity update (symplectic).
        // If we used standard Euler (position first), the y position after
        // one tick with zero initial velocity would be 0 (no movement).
        // With symplectic Euler, it should be -gravity (velocity updated to
        // -gravity, then applied to position).
        let pos = SubVoxelCoord::new(0, 0, 0);
        let vel = SubVoxelVec::new(0, 0, 0);
        let gravity = 100;

        let (new_pos, new_vel) = ballistic_step(pos, vel, gravity);
        assert_eq!(new_vel.y, -100);
        assert_eq!(new_pos.y, -100); // symplectic: uses updated velocity
    }

    // -----------------------------------------------------------------------
    // Trajectory simulation
    // -----------------------------------------------------------------------

    #[test]
    fn test_flat_trajectory_hits_target() {
        // Shoot horizontally at a nearby target with no gravity.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 10, 0));
        let speed_per_tick = SUB_VOXEL_ONE / 20; // 0.05 voxels per tick
        let vel = SubVoxelVec::new(speed_per_tick, 0, 0);

        let target = VoxelCoord::new(5, 10, 0);
        let result = simulate_trajectory(origin, vel, 0, target, 0, 5000);
        assert!(result.is_some(), "Should hit a target 5 voxels away");
        // At 0.05 voxels/tick, 5 voxels takes ~100 ticks
        let tick = result.unwrap();
        assert!(
            (90..=110).contains(&tick),
            "Expected ~100 ticks, got {tick}"
        );
    }

    #[test]
    fn test_trajectory_falls_to_ground() {
        // Launch horizontally with gravity — should eventually fall below
        // the target at the same height, and the simulation exits early.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 50, 0));
        let speed = SUB_VOXEL_ONE / 20;
        let vel = SubVoxelVec::new(speed, 0, 0);

        // Target far away at same height — gravity should pull arrow down
        let target = VoxelCoord::new(1000, 50, 0);
        let result = simulate_trajectory(origin, vel, EARTH_GRAVITY_SUB_VOXEL, target, 0, 100_000);
        assert!(
            result.is_none(),
            "Arrow should fall before reaching 1000 voxels"
        );
    }

    #[test]
    fn test_arcing_trajectory_rises_then_falls() {
        // Launch upward at 45 degrees with enough speed to visibly arc.
        // Use a higher speed than typical arrows so the arc is pronounced.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 10, 0));
        let speed = SUB_VOXEL_ONE / 5; // 200 voxels/sec (fast projectile)
        let vel = SubVoxelVec::new(speed, speed, 0); // 45 degrees

        let mut pos = origin;
        let mut v = vel;
        let mut max_y = pos.y;
        let mut went_below_origin = false;

        for _ in 1..=100_000 {
            (pos, v) = ballistic_step(pos, v, EARTH_GRAVITY_SUB_VOXEL);
            if pos.y > max_y {
                max_y = pos.y;
            }
            if pos.y < origin.y {
                went_below_origin = true;
                break;
            }
        }

        assert!(max_y > origin.y, "Arrow should rise above launch height");
        assert!(went_below_origin, "Arrow should fall back below origin");
        assert!(pos.x > origin.x, "Arrow should travel forward in x");
    }

    #[test]
    fn test_trajectory_realistic_arrow() {
        // Realistic arrow: ~50 voxels/sec = 25 m/s (modest bow).
        // At 1000 ticks/sec: 50/1000 = 0.05 voxels/tick.
        let speed_sub = SUB_VOXEL_ONE / 20; // 0.05 voxels/tick in sub-voxel units

        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 20, 0));
        // Aim slightly upward to compensate for gravity
        // At 45 degrees: vx = vy = speed / sqrt(2) ≈ speed * 7071 / 10000
        let component = speed_sub * 7071 / 10000;
        let vel = SubVoxelVec::new(component, component, 0);

        // Simulate and check the arrow eventually comes back down
        let mut pos = origin;
        let mut v = vel;
        let mut max_y = pos.y;
        let mut final_tick = 0u32;

        for tick in 1..=20_000 {
            (pos, v) = ballistic_step(pos, v, EARTH_GRAVITY_SUB_VOXEL);
            if pos.y > max_y {
                max_y = pos.y;
            }
            if sub_voxel_to_voxel(pos).y < 0 {
                final_tick = tick;
                break;
            }
        }

        assert!(final_tick > 0, "Arrow should eventually fall below y=0");
        assert!(max_y > origin.y, "Arrow should rise above origin");

        // Check the arrow traveled some horizontal distance
        let final_x_voxels = sub_voxel_to_voxel(pos).x;
        assert!(
            final_x_voxels > 0,
            "Arrow should travel horizontally, got {final_x_voxels}"
        );
    }

    // -----------------------------------------------------------------------
    // Aim computation
    // -----------------------------------------------------------------------

    #[test]
    fn test_aim_flat_short_range() {
        // Aim at a target 10 voxels away, same height, no gravity.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 20, 0));
        let target = VoxelCoord::new(10, 20, 0);
        let speed = SUB_VOXEL_ONE / 20; // 0.05 voxels/tick = 50 voxels/sec

        let result = compute_aim_velocity(origin, target, speed, 0, 5, 5000);
        assert!(
            result.hit_tick.is_some(),
            "Should hit target with no gravity"
        );
        assert!(result.velocity.x > 0, "Should aim in +x direction");
        assert_eq!(result.velocity.y, 0, "No vertical component needed");
    }

    #[test]
    fn test_aim_with_gravity() {
        // Aim at a target 10 voxels away, same height, with gravity.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 20, 0));
        let target = VoxelCoord::new(10, 20, 0);
        let speed = SUB_VOXEL_ONE / 20;

        let result = compute_aim_velocity(origin, target, speed, EARTH_GRAVITY_SUB_VOXEL, 5, 5000);
        // Should aim upward to compensate for gravity
        assert!(
            result.velocity.y > 0,
            "Should aim upward to compensate gravity"
        );
        assert!(result.velocity.x > 0, "Should aim in +x direction");
    }

    #[test]
    fn test_aim_downward() {
        // Aim at a target below — should need less/no upward compensation.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 50, 0));
        let target = VoxelCoord::new(10, 30, 0);
        let speed = SUB_VOXEL_ONE / 20;

        let result = compute_aim_velocity(origin, target, speed, EARTH_GRAVITY_SUB_VOXEL, 5, 5000);
        // Velocity should point somewhat downward
        assert!(result.velocity.y < 0, "Should aim downward at lower target");
    }

    #[test]
    fn test_aim_unreachable_target() {
        // Target extremely far away — arrow can't reach it.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 20, 0));
        let target = VoxelCoord::new(10000, 20, 0);
        let speed = SUB_VOXEL_ONE / 20;

        let result =
            compute_aim_velocity(origin, target, speed, EARTH_GRAVITY_SUB_VOXEL, 5, 10_000);
        // Should return a best-guess velocity (may not hit exactly,
        // though the tolerance=1 fallback may find a near-miss)
        assert!(result.velocity.x > 0, "Should still aim toward target");
    }

    #[test]
    fn test_aim_3d_target() {
        // Target offset in all three axes.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 20, 0));
        let target = VoxelCoord::new(7, 22, 5);
        let speed = SUB_VOXEL_ONE / 20;

        let result = compute_aim_velocity(origin, target, speed, EARTH_GRAVITY_SUB_VOXEL, 5, 5000);
        assert!(result.velocity.x > 0, "Should have +x component");
        assert!(result.velocity.z > 0, "Should have +z component");
    }

    #[test]
    fn test_aim_deterministic() {
        // Same inputs must produce identical outputs (determinism).
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 20, 0));
        let target = VoxelCoord::new(10, 20, 0);
        let speed = SUB_VOXEL_ONE / 20;

        let r1 = compute_aim_velocity(origin, target, speed, EARTH_GRAVITY_SUB_VOXEL, 5, 5000);
        let r2 = compute_aim_velocity(origin, target, speed, EARTH_GRAVITY_SUB_VOXEL, 5, 5000);

        assert_eq!(r1.velocity, r2.velocity);
        assert_eq!(r1.hit_tick, r2.hit_tick);
        assert_eq!(r1.iterations_used, r2.iterations_used);
    }

    #[test]
    fn test_aim_then_simulate_hits() {
        // Integration test: aim solver produces velocity, then full
        // simulation with that velocity actually reaches the target.
        let cases = [
            // (origin, target) — various distances and height differences
            (VoxelCoord::new(0, 20, 0), VoxelCoord::new(10, 20, 0)), // flat 10
            (VoxelCoord::new(0, 20, 0), VoxelCoord::new(5, 20, 5)),  // diagonal
            (VoxelCoord::new(0, 50, 0), VoxelCoord::new(8, 30, 0)),  // downhill
            (VoxelCoord::new(0, 20, 0), VoxelCoord::new(5, 25, 3)),  // uphill 3D
        ];
        let speed = SUB_VOXEL_ONE / 20; // 50 voxels/sec

        for (orig_v, tgt) in &cases {
            let origin = sub_voxel_from_voxel_center(*orig_v);
            let aim = compute_aim_velocity(origin, *tgt, speed, EARTH_GRAVITY_SUB_VOXEL, 5, 5000);

            // Simulate with the computed velocity
            let hit =
                simulate_trajectory(origin, aim.velocity, EARTH_GRAVITY_SUB_VOXEL, *tgt, 1, 5000);
            assert!(
                hit.is_some(),
                "Aim solver velocity should reach target {tgt:?} from {orig_v:?} \
                 (vel={:?}, aim_hit={:?})",
                aim.velocity,
                aim.hit_tick
            );
        }
    }

    #[test]
    fn test_sub_voxel_arithmetic() {
        let a = SubVoxelCoord::new(10, 20, 30);
        let b = SubVoxelCoord::new(3, 7, 11);

        let sum = a + b;
        assert_eq!(sum, SubVoxelCoord::new(13, 27, 41));

        let diff = a - b;
        assert_eq!(diff, SubVoxelCoord::new(7, 13, 19));

        let mut c = a;
        c += b;
        assert_eq!(c, SubVoxelCoord::new(13, 27, 41));
    }

    #[test]
    fn test_to_render_floats() {
        let pos = SubVoxelCoord::new(SUB_VOXEL_ONE * 3 / 2, SUB_VOXEL_ONE * 5, 0);
        let (rx, ry, rz) = sub_voxel_to_render_floats(pos);
        assert!((rx - 1.5).abs() < 0.001);
        assert!((ry - 5.0).abs() < 0.001);
        assert!(rz.abs() < 0.001);
    }

    #[test]
    fn test_aim_zero_speed() {
        // Zero speed should return zero velocity without panicking.
        let origin = sub_voxel_from_voxel_center(VoxelCoord::new(0, 20, 0));
        let target = VoxelCoord::new(10, 20, 0);
        let result = compute_aim_velocity(origin, target, 0, EARTH_GRAVITY_SUB_VOXEL, 5, 5000);
        assert_eq!(result.velocity, SubVoxelVec::new(0, 0, 0));
        assert_eq!(result.iterations_used, 0);
    }

    #[test]
    fn test_sub_voxel_coord_serde_roundtrip() {
        let coord = SubVoxelCoord::new(-12345, SUB_VOXEL_ONE * 50, 999999);
        let json = serde_json::to_string(&coord).unwrap();
        let roundtripped: SubVoxelCoord = serde_json::from_str(&json).unwrap();
        assert_eq!(coord, roundtripped);
    }

    // -----------------------------------------------------------------------
    // Gravity constant validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_gravity_constant_is_physically_correct() {
        // 1 voxel = 2m. g = 9.81 m/s² = 4.905 voxels/s².
        // In sub-voxel units per tick²: 4.905 / 1_000_000 * 2^30
        let expected = (4.905 / 1_000_000.0 * (1u64 << 30) as f64).round() as i64;
        // Allow ±1 for rounding
        assert!(
            (EARTH_GRAVITY_SUB_VOXEL - expected).abs() <= 1,
            "Gravity constant {EARTH_GRAVITY_SUB_VOXEL} doesn't match \
             expected {expected} (4.905 voxels/s² at 2^30 sub-units, 1000 ticks/sec)"
        );
    }

    #[test]
    fn test_free_fall_distance_one_second() {
        // Object in free fall for 1 second (1000 ticks) should drop ~4.905 voxels.
        // Using symplectic Euler, the accumulated displacement is:
        //   sum_{t=1}^{1000} (t * gravity) = gravity * 1000 * 1001 / 2
        let mut pos = SubVoxelCoord::new(0, 0, 0);
        let mut vel = SubVoxelVec::new(0, 0, 0);

        for _ in 0..1000 {
            (pos, vel) = ballistic_step(pos, vel, EARTH_GRAVITY_SUB_VOXEL);
        }

        // Expected drop in voxel units (negative y)
        let drop_voxels = (-pos.y as f64) / SUB_VOXEL_ONE as f64;
        // Physics: drop = 0.5 * g * t² = 0.5 * 4.905 * 1² = 2.4525 voxels
        // (each voxel is 2m, so 2.4525 voxels = 4.905 meters)
        // With symplectic Euler: drop = g_sub * n*(n+1)/2 / 2^30
        // = 5267 * 500500 / 1073741824 ≈ 2.455 voxels
        assert!(
            (drop_voxels - 2.4525).abs() < 0.05,
            "Free fall for 1 sec should drop ~2.4525 voxels (4.905m), got {drop_voxels:.4}"
        );
    }
}
