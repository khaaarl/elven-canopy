// Creature stat multiplier table and helpers.
//
// Implements the exponential scaling system where every +10 in a stat doubles
// its mechanical intensity. A flat precomputed `[i64; 131]` lookup table maps
// stat values from -60 to +70 to multipliers in 2^20 (1,048,576) fixed-point.
//
// Core operations:
// - `stat_multiplier(stat)` — raw table lookup, returns 2^20 fixed-point.
// - `apply_stat_multiplier(base, stat)` — `(base * multiplier) >> 20`.
// - `apply_stat_divisor(base, stat)` — `(base << 20) / multiplier`.
//   Used when higher stat means *fewer* units (e.g., move speed in ticks).
// - `apply_dex_deviation(rng, velocity, dex, base_ppm)` — applies DEX-based
//   angular deviation to a projectile aim velocity vector.
//
// All arithmetic is i64. No floating point anywhere.
//
// See also: `docs/drafts/creature_stats.md` for the design rationale,
// `types.rs` for `TraitKind` stat variants, `sim/creature.rs` for stat
// rolling at spawn.

/// Fixed-point shift: multipliers are in units of 2^20.
pub const STAT_SHIFT: u32 = 20;

/// 1x multiplier in fixed-point (2^20 = 1,048,576).
pub const STAT_ONE: i64 = 1i64 << STAT_SHIFT;

/// Minimum stat value in the lookup table.
const STAT_MIN: i64 = -60;

/// Maximum stat value in the lookup table.
const STAT_MAX: i64 = 70;

/// Table size: covers STAT_MIN..=STAT_MAX.
const TABLE_SIZE: usize = (STAT_MAX - STAT_MIN + 1) as usize; // 131

/// Precomputed multiplier table. Entry `i` corresponds to stat value
/// `i as i64 + STAT_MIN`. Values at multiples of 10 are exact powers of 2;
/// intermediate values are linearly interpolated within each octave.
/// Maximum error vs true `2^(s/10)` is ~6% at octave midpoints.
const STAT_TABLE: [i64; TABLE_SIZE] = generate_stat_table();

/// Generate the lookup table at compile time.
const fn generate_stat_table() -> [i64; TABLE_SIZE] {
    let mut table = [0i64; TABLE_SIZE];
    let mut i = 0;
    while i < TABLE_SIZE {
        let s = i as i64 + STAT_MIN;
        // Euclidean division: floor toward negative infinity.
        let decade = s.div_euclid(10);
        let frac = s.rem_euclid(10); // always 0..9
        let lo = if decade >= 0 {
            STAT_ONE << decade
        } else {
            STAT_ONE >> (-decade)
        };
        let hi = lo * 2;
        table[i] = lo + (hi - lo) * frac / 10;
        i += 1;
    }
    table
}

/// Look up the exponential multiplier for a stat value.
/// Returns a 2^20 fixed-point value. Stat 0 → 1,048,576 (1x).
/// Values outside [-60, +70] clamp to the table endpoints.
pub fn stat_multiplier(stat: i64) -> i64 {
    let idx = (stat - STAT_MIN).clamp(0, (TABLE_SIZE - 1) as i64) as usize;
    STAT_TABLE[idx]
}

/// Apply the stat multiplier to a base value (multiply path).
/// `base * 2^(stat/10)`, computed in integer arithmetic.
///
/// Example: `apply_stat_multiplier(25, 30)` = 25 * 8 = 200.
pub fn apply_stat_multiplier(base: i64, stat: i64) -> i64 {
    (base * stat_multiplier(stat)) >> STAT_SHIFT
}

/// Apply the stat multiplier as a divisor (inverse path).
/// `base / 2^(stat/10)`, computed in integer arithmetic.
/// Used when higher stat should *reduce* the base (e.g., move ticks).
///
/// Example: `apply_stat_divisor(500, 10)` = 500 / 2 = 250 (twice as fast).
pub fn apply_stat_divisor(base: i64, stat: i64) -> i64 {
    let mult = stat_multiplier(stat);
    if mult == 0 {
        return base; // safety: shouldn't happen with valid table
    }
    (base << STAT_SHIFT) / mult
}

/// The 8 stat trait kinds in canonical order, used for iteration during
/// spawn-time rolling.
pub const STAT_TRAIT_KINDS: [crate::types::TraitKind; 8] = {
    use crate::types::TraitKind;
    [
        TraitKind::Strength,
        TraitKind::Agility,
        TraitKind::Dexterity,
        TraitKind::Constitution,
        TraitKind::Willpower,
        TraitKind::Intelligence,
        TraitKind::Perception,
        TraitKind::Charisma,
    ]
};

/// Stat-modified movement speeds for a creature. Precomputed from species
/// base values and the creature's Agility and Strength stats.
pub struct CreatureMoveSpeeds {
    pub walk_tpv: u64,
    pub climb_tpv: Option<u64>,
    pub wood_ladder_tpv: Option<u64>,
    pub rope_ladder_tpv: Option<u64>,
}

impl CreatureMoveSpeeds {
    /// Compute stat-modified movement speeds from species data and creature stats.
    pub fn new(species_data: &crate::species::SpeciesData, agility: i64, strength: i64) -> Self {
        let climb_stat = (agility + strength) / 2; // geometric-mean blend
        Self {
            walk_tpv: apply_stat_divisor(species_data.walk_ticks_per_voxel as i64, agility) as u64,
            climb_tpv: species_data
                .climb_ticks_per_voxel
                .map(|t| apply_stat_divisor(t as i64, climb_stat) as u64),
            wood_ladder_tpv: species_data
                .wood_ladder_tpv
                .map(|t| apply_stat_divisor(t as i64, climb_stat) as u64),
            rope_ladder_tpv: species_data
                .rope_ladder_tpv
                .map(|t| apply_stat_divisor(t as i64, climb_stat) as u64),
        }
    }

    /// Resolve the appropriate tpv for a given edge type.
    pub fn tpv_for_edge(&self, edge_type: crate::nav::EdgeType) -> u64 {
        use crate::nav::EdgeType;
        match edge_type {
            EdgeType::TrunkClimb | EdgeType::GroundToTrunk => {
                self.climb_tpv.unwrap_or(self.walk_tpv)
            }
            EdgeType::WoodLadderClimb => self.wood_ladder_tpv.unwrap_or(self.walk_tpv),
            EdgeType::RopeLadderClimb => self.rope_ladder_tpv.unwrap_or(self.walk_tpv),
            _ => self.walk_tpv,
        }
    }
}

/// Apply dexterity-based angular deviation to a projectile aim velocity.
///
/// The deviation is expressed in parts-per-million of the velocity magnitude,
/// applied as random offsets to the X and Z velocity components (lateral axes).
/// Higher DEX reduces deviation asymptotically via the exponential table.
///
/// Uses 2 PRNG calls (one per lateral axis).
pub fn apply_dex_deviation(
    rng: &mut crate::prng::GameRng,
    velocity: crate::projectile::SubVoxelVec,
    dexterity: i64,
    base_deviation_ppm: i64,
) -> crate::projectile::SubVoxelVec {
    if base_deviation_ppm <= 0 {
        return velocity;
    }
    let dex_mult = stat_multiplier(dexterity);
    // deviation_ppm = base * (1 << 20) / dex_mult
    let deviation_ppm = base_deviation_ppm * STAT_ONE / dex_mult;
    if deviation_ppm <= 0 {
        return velocity;
    }

    // Compute velocity magnitude approximation for scaling.
    // Use the max component as a rough magnitude (cheaper than sqrt, adequate
    // for deviation scaling since we just need an order-of-magnitude scale).
    let vx = velocity.x.abs();
    let vy = velocity.y.abs();
    let vz = velocity.z.abs();
    let v_approx = vx.max(vy).max(vz);
    if v_approx == 0 {
        return velocity;
    }

    // Offset per axis = v_approx * deviation_ppm / 1_000_000
    let max_offset = v_approx * deviation_ppm / 1_000_000;
    if max_offset == 0 {
        return velocity;
    }

    let offset_x = rng.range_i64_inclusive(-max_offset, max_offset);
    let offset_z = rng.range_i64_inclusive(-max_offset, max_offset);

    crate::projectile::SubVoxelVec {
        x: velocity.x + offset_x,
        y: velocity.y, // no vertical deviation — gravity handles arc
        z: velocity.z + offset_z,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_table_zero_is_baseline() {
        assert_eq!(stat_multiplier(0), STAT_ONE);
    }

    #[test]
    fn stat_table_powers_of_two() {
        // Every multiple of 10 should be an exact power of 2.
        for decade in -6..=7i64 {
            let s = decade * 10;
            let expected = if decade >= 0 {
                STAT_ONE << decade
            } else {
                STAT_ONE >> (-decade)
            };
            assert_eq!(
                stat_multiplier(s),
                expected,
                "stat {s}: expected {expected}, got {}",
                stat_multiplier(s)
            );
        }
    }

    #[test]
    fn apply_stat_multiplier_identity() {
        assert_eq!(apply_stat_multiplier(100, 0), 100);
    }

    #[test]
    fn apply_stat_multiplier_double() {
        assert_eq!(apply_stat_multiplier(100, 10), 200);
    }

    #[test]
    fn apply_stat_multiplier_half() {
        assert_eq!(apply_stat_multiplier(100, -10), 50);
    }

    #[test]
    fn apply_stat_multiplier_troll_example() {
        // Troll STR +30, base_melee_damage 25 → 200.
        assert_eq!(apply_stat_multiplier(25, 30), 200);
    }

    #[test]
    fn apply_stat_multiplier_elf_str_example() {
        // Elf STR -3, base_melee_damage 10 → 8.
        assert_eq!(apply_stat_multiplier(10, -3), 8);
    }

    #[test]
    fn apply_stat_divisor_identity() {
        assert_eq!(apply_stat_divisor(500, 0), 500);
    }

    #[test]
    fn apply_stat_divisor_double_speed() {
        // AGI +10 → half the ticks (twice as fast).
        assert_eq!(apply_stat_divisor(500, 10), 250);
    }

    #[test]
    fn apply_stat_divisor_half_speed() {
        // AGI -10 → double the ticks (half as fast).
        assert_eq!(apply_stat_divisor(500, -10), 1000);
    }

    #[test]
    fn stat_table_clamps_extremes() {
        // Values beyond table bounds should clamp.
        assert_eq!(stat_multiplier(-100), stat_multiplier(-60));
        assert_eq!(stat_multiplier(100), stat_multiplier(70));
    }

    #[test]
    fn stat_table_monotonic() {
        // Table should be strictly increasing.
        for i in 1..TABLE_SIZE {
            assert!(
                STAT_TABLE[i] > STAT_TABLE[i - 1],
                "table not monotonic at index {i}: {} <= {}",
                STAT_TABLE[i],
                STAT_TABLE[i - 1]
            );
        }
    }

    #[test]
    fn apply_stat_multiplier_negative_base() {
        // Negative base values (shouldn't happen in practice but shouldn't panic).
        assert_eq!(apply_stat_multiplier(-10, 10), -20);
    }

    #[test]
    fn dex_deviation_zero_base_returns_unchanged() {
        use crate::projectile::SubVoxelVec;
        let mut rng = crate::prng::GameRng::new(42);
        let vel = SubVoxelVec {
            x: 1000,
            y: -500,
            z: 2000,
        };
        let result = apply_dex_deviation(&mut rng, vel, 0, 0);
        assert_eq!(result.x, vel.x);
        assert_eq!(result.y, vel.y);
        assert_eq!(result.z, vel.z);
    }

    #[test]
    fn dex_deviation_zero_velocity_returns_unchanged() {
        use crate::projectile::SubVoxelVec;
        let mut rng = crate::prng::GameRng::new(42);
        let vel = SubVoxelVec { x: 0, y: 0, z: 0 };
        let result = apply_dex_deviation(&mut rng, vel, 0, 50000);
        assert_eq!(result.x, 0);
        assert_eq!(result.y, 0);
        assert_eq!(result.z, 0);
    }

    #[test]
    fn dex_deviation_preserves_y_component() {
        use crate::projectile::SubVoxelVec;
        let mut rng = crate::prng::GameRng::new(42);
        let vel = SubVoxelVec {
            x: 100000,
            y: -50000,
            z: 200000,
        };
        let result = apply_dex_deviation(&mut rng, vel, 0, 50000);
        // Y should never be modified (no vertical deviation).
        assert_eq!(result.y, vel.y);
    }

    #[test]
    fn dex_deviation_high_dex_reduces_spread() {
        use crate::projectile::SubVoxelVec;
        let vel = SubVoxelVec {
            x: 1_000_000,
            y: 0,
            z: 0,
        };
        let base_ppm = 50000;

        // Sample many shots at DEX 0 vs DEX +20, compare deviation magnitudes.
        let mut low_dex_total: i64 = 0;
        let mut high_dex_total: i64 = 0;
        let n = 1000;
        for seed in 0..n {
            let mut rng = crate::prng::GameRng::new(seed);
            let r = apply_dex_deviation(&mut rng, vel, 0, base_ppm);
            low_dex_total += (r.x - vel.x).abs() + (r.z - vel.z).abs();

            let mut rng2 = crate::prng::GameRng::new(seed);
            let r2 = apply_dex_deviation(&mut rng2, vel, 20, base_ppm);
            high_dex_total += (r2.x - vel.x).abs() + (r2.z - vel.z).abs();
        }
        // High DEX should produce significantly less total deviation.
        assert!(
            high_dex_total < low_dex_total / 2,
            "DEX +20 should have <50% the deviation of DEX 0: high={high_dex_total}, low={low_dex_total}"
        );
    }

    #[test]
    fn dex_deviation_deterministic() {
        use crate::projectile::SubVoxelVec;
        let vel = SubVoxelVec {
            x: 500000,
            y: -100000,
            z: 300000,
        };
        let mut rng1 = crate::prng::GameRng::new(99);
        let mut rng2 = crate::prng::GameRng::new(99);
        let r1 = apply_dex_deviation(&mut rng1, vel, 5, 50000);
        let r2 = apply_dex_deviation(&mut rng2, vel, 5, 50000);
        assert_eq!(r1.x, r2.x);
        assert_eq!(r1.y, r2.y);
        assert_eq!(r1.z, r2.z);
    }
}
