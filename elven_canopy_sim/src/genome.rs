// Creature genome — additive SNP bitfield model for heritable traits.
//
// Every creature carries two immutable bitfields: a generic genome (ability
// scores as 32-bit weighted-prime-sum SNP regions + Big Five personality as
// 8-bit regions) and a species-specific genome (pigmentation, morphology).
// Continuous traits use weighted-sum (ability scores) or bit-count (everything
// else) scaled to species mean/stdev. Categorical traits use max weighted sum
// across competing sections with PRNG tiebreak.
//
// The genome is immutable after creation. Expressed traits are stored separately
// in the creature_traits table and can diverge (dye, injury, aging). Inheritance
// uses per-bit 50/50 parent selection with a small mutation rate.
//
// Key design files:
// - `docs/drafts/genetics.md` — full design document
// - `species.rs` — SpeciesData, StatDistribution, SpeciesGenomeConfig
// - `sim/creature.rs` — spawn-time genome generation and trait expression
// - `db.rs` — CreatureGenome tabulosity table
//
// Critical constraints:
// - **Determinism.** All arithmetic is integer-only. No floating-point.
// - **Append-only layout.** New SNP regions are only added at the end of each
//   bitfield. Never reorder or remove existing regions. Serde uses the stored
//   `bit_len` to detect shorter genomes from old saves and backfills trailing
//   bits deterministically from SplitMix64 seeded by the creature's ID.

use elven_canopy_prng::GameRng;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Genome type
// ---------------------------------------------------------------------------

/// Compact bitfield genome. Bit ordering is LSB-first within each byte.
/// SNP regions are packed without padding and may straddle byte boundaries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genome {
    bytes: Vec<u8>,
    bit_len: u32,
}

impl Genome {
    /// Create a zero-filled genome with the given number of bits.
    pub fn new(bit_len: u32) -> Self {
        let byte_len = (bit_len as usize).div_ceil(8);
        Self {
            bytes: vec![0u8; byte_len],
            bit_len,
        }
    }

    /// Create a genome with random bits drawn from the PRNG.
    pub fn random(rng: &mut GameRng, bit_len: u32) -> Self {
        let byte_len = (bit_len as usize).div_ceil(8);
        let mut bytes = Vec::with_capacity(byte_len);

        // Fill 8 bytes at a time from u64s.
        let full_u64s = byte_len / 8;
        let remainder = byte_len % 8;
        for _ in 0..full_u64s {
            bytes.extend_from_slice(&rng.next_u64().to_le_bytes());
        }
        if remainder > 0 {
            let last = rng.next_u64().to_le_bytes();
            bytes.extend_from_slice(&last[..remainder]);
        }

        // Mask off trailing bits beyond bit_len so they are always zero.
        let trailing = bit_len % 8;
        if trailing != 0 && !bytes.is_empty() {
            let last_idx = bytes.len() - 1;
            bytes[last_idx] &= (1u8 << trailing) - 1;
        }

        Self { bytes, bit_len }
    }

    /// Create an offspring genome from two parents via per-bit crossover
    /// with optional mutation.
    ///
    /// For each bit position, the offspring bit is taken from parent_a or
    /// parent_b with equal probability. Then each bit has
    /// `mutation_rate_per_mille / 1000` probability of being flipped
    /// (e.g., 2 = 0.2% = ~1 in 500 bits).
    ///
    /// Both parents must have the same `bit_len`. Panics otherwise.
    pub fn from_parents(
        parent_a: &Genome,
        parent_b: &Genome,
        rng: &mut GameRng,
        mutation_rate_per_mille: u32,
    ) -> Self {
        assert_eq!(
            parent_a.bit_len, parent_b.bit_len,
            "parent genomes must have the same bit_len"
        );
        let bit_len = parent_a.bit_len;
        let byte_len = (bit_len as usize).div_ceil(8);
        let mut bytes = vec![0u8; byte_len];

        // Per-bit crossover: for each bit, pick from parent_a or parent_b.
        // We process one bit at a time for clarity. The genome is small
        // (~40-60 bytes) so this is not a bottleneck.
        for i in 0..bit_len {
            let parent_bit = if rng.next_u64() & 1 == 0 {
                parent_a.get_bit(i)
            } else {
                parent_b.get_bit(i)
            };
            if parent_bit {
                let byte_idx = (i / 8) as usize;
                let bit_idx = i % 8;
                bytes[byte_idx] |= 1u8 << bit_idx;
            }
        }

        // Mutation: flip each bit with probability mutation_rate_per_mille / 1000.
        for i in 0..bit_len {
            if rng.next_u64() % 1000 < mutation_rate_per_mille as u64 {
                let byte_idx = (i / 8) as usize;
                let bit_idx = i % 8;
                bytes[byte_idx] ^= 1u8 << bit_idx;
            }
        }

        Self { bytes, bit_len }
    }

    /// Number of logical bits in this genome.
    pub fn bit_len(&self) -> u32 {
        self.bit_len
    }

    /// Get the value of bit `i` (0-indexed, LSB-first within each byte).
    ///
    /// Panics if `i >= bit_len`.
    pub fn get_bit(&self, i: u32) -> bool {
        assert!(
            i < self.bit_len,
            "bit index {i} out of range (len {})",
            self.bit_len
        );
        let byte_idx = (i / 8) as usize;
        let bit_idx = i % 8;
        (self.bytes[byte_idx] >> bit_idx) & 1 != 0
    }

    /// Set the value of bit `i`.
    ///
    /// Panics if `i >= bit_len`.
    pub fn set_bit(&mut self, i: u32, value: bool) {
        assert!(
            i < self.bit_len,
            "bit index {i} out of range (len {})",
            self.bit_len
        );
        let byte_idx = (i / 8) as usize;
        let bit_idx = i % 8;
        if value {
            self.bytes[byte_idx] |= 1u8 << bit_idx;
        } else {
            self.bytes[byte_idx] &= !(1u8 << bit_idx);
        }
    }

    /// Count the number of 1-bits in the region `[start..start+len)`.
    ///
    /// Panics if the region exceeds `bit_len`.
    pub fn count_ones_in_region(&self, start: u32, len: u32) -> u32 {
        assert!(
            start + len <= self.bit_len,
            "region [{start}..{}) exceeds bit_len {}",
            start + len,
            self.bit_len,
        );
        let mut count = 0u32;
        for i in start..start + len {
            if self.get_bit(i) {
                count += 1;
            }
        }
        count
    }

    /// Compute the weighted sum of 1-bits in the region `[start..start+len)`.
    ///
    /// Each bit position `j` (0-indexed within the region) has weight
    /// `weights[j]`. Only 1-bits contribute their weight to the sum.
    ///
    /// Panics if `weights.len() < len` or if the region exceeds `bit_len`.
    pub fn weighted_sum_in_region(&self, start: u32, len: u32, weights: &[i64]) -> i64 {
        assert!(
            weights.len() >= len as usize,
            "need at least {len} weights, got {}",
            weights.len(),
        );
        assert!(
            start + len <= self.bit_len,
            "region [{start}..{}) exceeds bit_len {}",
            start + len,
            self.bit_len,
        );
        let mut sum = 0i64;
        for j in 0..len {
            if self.get_bit(start + j) {
                sum += weights[j as usize];
            }
        }
        sum
    }

    /// Extend this genome to `new_bit_len` by filling new trailing bits
    /// deterministically from a SplitMix64 seeded by `backfill_seed`.
    ///
    /// Used during deserialization when loading an old-format genome that
    /// is shorter than the current layout. The backfill seed is derived
    /// from the creature's ID (XOR-folded to u64) so that each creature
    /// gets plausible genetics for newly-added SNP regions.
    ///
    /// No-op if `new_bit_len <= self.bit_len`.
    pub fn backfill_to(&mut self, new_bit_len: u32, backfill_seed: u64) {
        if new_bit_len <= self.bit_len {
            return;
        }

        // Extend bytes vector to accommodate new bits.
        let new_byte_len = (new_bit_len as usize).div_ceil(8);
        self.bytes.resize(new_byte_len, 0);

        // Fill new bits from SplitMix64.
        let mut sm_state = backfill_seed;
        let mut random_bits = 0u64;
        let mut bits_remaining = 0u32;

        for i in self.bit_len..new_bit_len {
            if bits_remaining == 0 {
                random_bits = elven_canopy_prng::splitmix64(&mut sm_state);
                bits_remaining = 64;
            }
            if random_bits & 1 != 0 {
                let byte_idx = (i / 8) as usize;
                let bit_idx = i % 8;
                self.bytes[byte_idx] |= 1u8 << bit_idx;
            }
            random_bits >>= 1;
            bits_remaining -= 1;
        }

        self.bit_len = new_bit_len;
    }
}

// ---------------------------------------------------------------------------
// Prime weights for ability score SNP regions (32 bits each)
// ---------------------------------------------------------------------------

/// 32 distinct primes in the range [1,000,000 .. 5,000,000], used as weights
/// for ability score SNP regions. Each of the 32 bits in a stat's SNP region
/// has a distinct weight; the stat's raw value is the sum of weights for all
/// 1-bits. Using large primes dramatically reduces sum collisions compared
/// to simple bit-counting, yielding many more distinct trait values.
///
/// The ~5× ratio between smallest and largest weights gives natural
/// "major SNP / minor SNP" texture — some bits matter more than others.
pub const PRIME_WEIGHTS: [i64; 32] = [
    1_000_003, 1_256_863, 1_489_321, 1_738_127, 1_913_533, 2_087_483, 2_341_637, 2_503_637,
    2_749_067, 2_918_857, 3_124_567, 3_308_953, 3_517_867, 3_742_741, 3_891_431, 4_123_891,
    4_297_043, 4_501_817, 4_718_617, 4_903_733, 1_137_427, 1_373_539, 1_621_793, 1_847_327,
    2_011_543, 2_213_963, 2_428_003, 2_637_493, 2_833_921, 3_041_173, 3_219_497, 3_407_647,
];

/// Sum of all prime weights. The expected mean of the weighted sum when each
/// bit is 50/50 is `WEIGHT_SUM / 2`.
pub const WEIGHT_SUM: i64 = {
    let mut sum = 0i64;
    let mut i = 0;
    while i < 32 {
        sum += PRIME_WEIGHTS[i];
        i += 1;
    }
    sum
};

/// Expected mean of the weighted sum = WEIGHT_SUM / 2.
pub const EXPECTED_MEAN: i64 = WEIGHT_SUM / 2;

/// Sum of squares of all prime weights, used for theoretical stdev computation.
const WEIGHT_SUM_OF_SQUARES: i64 = {
    let mut sum = 0i64;
    let mut i = 0;
    while i < 32 {
        sum += PRIME_WEIGHTS[i] * PRIME_WEIGHTS[i];
        i += 1;
    }
    sum
};

/// Theoretical standard deviation of the weighted sum (integer approximation).
///
/// For independent Bernoulli(0.5) variables with weights w_i:
///   variance = sum(w_i^2) / 4
///   stdev = sqrt(sum(w_i^2)) / 2
///
/// We store this as an integer via const-time isqrt.
pub const THEORETICAL_STDEV: i64 = {
    let sum_sq = WEIGHT_SUM_OF_SQUARES;
    // isqrt(sum_sq) via Newton's method (const-compatible).
    let mut x = sum_sq;
    if x > 1 {
        let mut x1 = x / 2;
        while x1 < x {
            x = x1;
            x1 = (x + sum_sq / x) / 2;
        }
    }
    // stdev = isqrt(sum_sq) / 2
    x / 2
};

// ---------------------------------------------------------------------------
// Generic genome layout constants
// ---------------------------------------------------------------------------

// GENOME LAYOUT (generic) — append only, never reorder!
// [0..32)    STR
// [32..64)   AGI
// [64..96)   DEX
// [96..128)  CON
// [128..160) WIL
// [160..192) INT
// [192..224) PER
// [224..256) CHA
// [256..264) Openness
// [264..272) Conscientiousness
// [272..280) Extraversion
// [280..288) Agreeableness
// [288..296) Neuroticism
// --- add new regions here ---

/// Number of bits per ability score SNP region.
pub const STAT_BITS: u32 = 32;

/// Number of bits per personality axis SNP region.
pub const PERSONALITY_BITS: u32 = 8;

/// Total number of bits in the generic genome.
pub const GENERIC_GENOME_BITS: u32 = 8 * STAT_BITS + 5 * PERSONALITY_BITS; // 296

/// Bit offset for a given stat index (0=STR, 1=AGI, ..., 7=CHA).
pub const fn stat_offset(stat_index: u32) -> u32 {
    stat_index * STAT_BITS
}

/// Personality axis indices.
pub const OPENNESS: u32 = 0;
pub const CONSCIENTIOUSNESS: u32 = 1;
pub const EXTRAVERSION: u32 = 2;
pub const AGREEABLENESS: u32 = 3;
pub const NEUROTICISM: u32 = 4;

/// Bit offset for a given personality axis index (0=Openness, ..., 4=Neuroticism).
pub const fn personality_offset(axis_index: u32) -> u32 {
    8 * STAT_BITS + axis_index * PERSONALITY_BITS
}

/// Personality TraitKinds in genome-layout order (Openness through Neuroticism).
pub const PERSONALITY_TRAIT_KINDS: [crate::types::TraitKind; 5] = {
    use crate::types::TraitKind;
    [
        TraitKind::Openness,
        TraitKind::Conscientiousness,
        TraitKind::Extraversion,
        TraitKind::Agreeableness,
        TraitKind::Neuroticism,
    ]
};

/// Express a personality axis value from the generic genome.
///
/// Uses bit-count expression on the 8-bit personality SNP region, scaled
/// to the species personality distribution (mean, stdev).
pub fn express_personality(
    genome: &Genome,
    axis_index: u32,
    species_mean: i64,
    species_stdev: i64,
) -> i64 {
    let offset = personality_offset(axis_index);
    express_bitcount(
        genome,
        offset,
        PERSONALITY_BITS,
        species_mean,
        species_stdev,
    )
}

// ---------------------------------------------------------------------------
// Trait expression functions
// ---------------------------------------------------------------------------

/// Round-to-nearest integer division, symmetric around zero.
/// This matches the rounding behavior of `quasi_normal` in the PRNG crate.
fn symmetric_div(numerator: i64, divisor: i64) -> i64 {
    if numerator >= 0 {
        (numerator + divisor / 2) / divisor
    } else {
        (numerator - divisor / 2) / divisor
    }
}

/// Express an ability score from a 32-bit weighted-sum SNP region.
///
/// Maps the genome's weighted sum to the species stat scale:
///   `trait_value = (raw - EXPECTED_MEAN) * species_stdev / THEORETICAL_STDEV + species_mean`
///
/// Returns an i64 on the same scale as existing stat values (centered near 0,
/// where +100 ≈ 2× power via exponential scaling).
pub fn express_stat(
    genome: &Genome,
    stat_index: u32,
    species_mean: i64,
    species_stdev: i64,
) -> i64 {
    let offset = stat_offset(stat_index);
    let raw = genome.weighted_sum_in_region(offset, STAT_BITS, &PRIME_WEIGHTS);
    let centered = raw - EXPECTED_MEAN;
    symmetric_div(centered * species_stdev, THEORETICAL_STDEV) + species_mean
}

/// Fixed-point scaling factors for bit-count trait expression, indexed by
/// bit-width. For bit-width `n`, the theoretical stdev of B(n, 0.5) is
/// `sqrt(n) / 2`. To avoid floating-point, we store:
///   `BITCOUNT_STDEV_SCALE[n] = round(SCALE / (2 * sqrt(n/4)))
///                             = round(SCALE / sqrt(n))`
/// where SCALE = 2^20 = 1,048,576.
///
/// Trait expression: `value = (2*k - n) * species_stdev * BITCOUNT_STDEV_SCALE[n] / SCALE + mean`
///
/// Only even widths 2..=32 are populated; others are zero.
const BITCOUNT_SCALE: i64 = 1 << 20; // 1,048,576
const BITCOUNT_STDEV_SCALE: [i64; 33] = {
    // Precompute round(2^20 / sqrt(n)) for n = 0..32 using const isqrt.
    // For n=0 and n=1, the scale is meaningless (0 or 1 distinct values).
    let mut table = [0i64; 33];
    let mut n = 2u32;
    while n <= 32 {
        // We need round(SCALE / sqrt(n)).
        // sqrt(n) = isqrt(n * SCALE^2) / SCALE, so:
        // SCALE / sqrt(n) = SCALE^2 / isqrt(n * SCALE^2)
        //
        // But n * SCALE^2 can overflow i64 for large n. Instead:
        // SCALE / sqrt(n) = SCALE * isqrt(SCALE^2) / isqrt(n * SCALE^2)
        //                  = SCALE * SCALE / isqrt(n * SCALE^2)
        //
        // For n <= 32 and SCALE = 2^20: n * SCALE^2 = 32 * 2^40 = 2^45, fits i64.
        let product = (n as i64) * BITCOUNT_SCALE * BITCOUNT_SCALE;
        // isqrt(product)
        let mut x = product;
        if x > 1 {
            let mut x1 = x / 2;
            while x1 < x {
                x = x1;
                x1 = (x + product / x) / 2;
            }
        }
        let isqrt_val = x;
        // round(SCALE^2 / isqrt_val)
        table[n as usize] = (BITCOUNT_SCALE * BITCOUNT_SCALE + isqrt_val / 2) / isqrt_val;
        n += 1;
    }
    table
};

/// Express a continuous trait from a bit-count SNP region (personality,
/// pigmentation axes, etc.).
///
/// Maps the region's bit-count to the species scale:
///   `trait_value = (2*k - n) * species_stdev * STDEV_SCALE / SCALE + species_mean`
///
/// where `k = count_ones`, `n = bit_width`.
pub fn express_bitcount(
    genome: &Genome,
    start: u32,
    bit_width: u32,
    species_mean: i64,
    species_stdev: i64,
) -> i64 {
    assert!(
        (2..=32).contains(&bit_width),
        "bit_width must be in [2, 32], got {bit_width}"
    );
    let k = genome.count_ones_in_region(start, bit_width) as i64;
    let n = bit_width as i64;
    let centered = 2 * k - n;
    let scale_factor = BITCOUNT_STDEV_SCALE[bit_width as usize];
    symmetric_div(centered * species_stdev * scale_factor, BITCOUNT_SCALE) + species_mean
}

/// Small prime weight tables for categorical SNP scoring. Each bit in a
/// category's SNP region gets a distinct weight. Using weighted sums instead
/// of bit-count gives far more distinct scores, making ties rare and blend
/// weights smooth.
const CATEGORICAL_WEIGHTS_4: [i64; 4] = [1013, 1597, 2311, 3109];
const CATEGORICAL_WEIGHTS_6: [i64; 6] = [1013, 1597, 2311, 3109, 4007, 4973];

/// Get the prime weight table for a given bit-width.
fn categorical_weights(bits: u32) -> &'static [i64] {
    match bits {
        4 => &CATEGORICAL_WEIGHTS_4,
        6 => &CATEGORICAL_WEIGHTS_6,
        _ => panic!("no categorical weight table for {bits}-bit regions"),
    }
}

/// Score each category by weighted sum and return (category_index, score) pairs
/// sorted by score descending. Used by both blended and non-blended categorical
/// expression.
fn score_categories(
    genome: &Genome,
    start: u32,
    bits_per_category: u32,
    num_categories: u32,
) -> Vec<(u32, i64)> {
    let weights = categorical_weights(bits_per_category);
    let mut scores: Vec<(u32, i64)> = (0..num_categories)
        .map(|cat| {
            let cat_start = start + cat * bits_per_category;
            let score = genome.weighted_sum_in_region(cat_start, bits_per_category, weights);
            (cat, score)
        })
        .collect();
    scores.sort_by(|a, b| b.1.cmp(&a.1));
    scores
}

/// Express a categorical trait from competing SNP sections.
///
/// Each category has a section of `bits_per_category` bits starting at
/// successive offsets from `start`. Categories are scored by weighted prime
/// sum. The category with the highest weighted sum wins. Exact ties are
/// broken by PRNG seeded from `tiebreak_seed`.
///
/// Returns the 0-indexed winning category.
pub fn express_categorical(
    genome: &Genome,
    start: u32,
    bits_per_category: u32,
    num_categories: u32,
    tiebreak_seed: u64,
) -> u32 {
    assert!(num_categories > 0, "must have at least one category");
    let scores = score_categories(genome, start, bits_per_category, num_categories);

    // Collect all categories tied at the top score.
    let best_score = scores[0].1;
    let best: Vec<u32> = scores
        .iter()
        .take_while(|(_, s)| *s == best_score)
        .map(|(cat, _)| *cat)
        .collect();

    if best.len() == 1 {
        best[0]
    } else {
        let mut sm_state = tiebreak_seed;
        let r = elven_canopy_prng::splitmix64(&mut sm_state);
        best[(r % best.len() as u64) as usize]
    }
}

/// Result of categorical expression with hue-wheel blending.
#[derive(Clone, Debug, PartialEq)]
pub enum CategoricalResult {
    /// Single winner — top two are not adjacent on the hue wheel.
    Single(u32),
    /// Blend of two adjacent hue-wheel categories. `weight` is 0–255,
    /// where 0 = fully `primary` and 255 = fully `secondary`.
    Blend {
        primary: u32,
        secondary: u32,
        /// Blend weight toward secondary, in 0–255 range (0 = fully primary).
        weight: u8,
    },
}

/// Express a categorical trait with hue-wheel blending.
///
/// Like `express_categorical`, but when the top two scoring categories are
/// adjacent on the hue wheel (indices differ by 1, or wrap-around), the
/// result is a blend with weight proportional to their score ratio.
///
/// `num_categories` defines the hue wheel size. Adjacency wraps: category
/// 0 is adjacent to category `num_categories - 1`.
pub fn express_categorical_blended(
    genome: &Genome,
    start: u32,
    bits_per_category: u32,
    num_categories: u32,
    tiebreak_seed: u64,
) -> CategoricalResult {
    assert!(
        num_categories >= 2,
        "blending requires at least 2 categories"
    );
    let scores = score_categories(genome, start, bits_per_category, num_categories);

    let (top_cat, top_score) = scores[0];
    let (second_cat, second_score) = scores[1];

    // Check if top two are adjacent on the hue wheel (with wrap-around).
    let diff = top_cat.abs_diff(second_cat);
    let adjacent = diff == 1 || diff == num_categories - 1;

    if !adjacent || second_score == 0 {
        // Not adjacent or second has zero score — winner-take-all.
        // Handle ties at top score.
        let best_score = top_score;
        let best: Vec<u32> = scores
            .iter()
            .take_while(|(_, s)| *s == best_score)
            .map(|(cat, _)| *cat)
            .collect();
        let winner = if best.len() == 1 {
            best[0]
        } else {
            let mut sm_state = tiebreak_seed;
            let r = elven_canopy_prng::splitmix64(&mut sm_state);
            best[(r % best.len() as u64) as usize]
        };
        return CategoricalResult::Single(winner);
    }

    // Blend: weight toward secondary = secondary_score / (top + secondary).
    // Pure integer arithmetic with symmetric rounding (no floating-point).
    let total = top_score + second_score;
    let weight = ((second_score * 255 + total / 2) / total) as u8;

    CategoricalResult::Blend {
        primary: top_cat,
        secondary: second_cat,
        weight,
    }
}

// ---------------------------------------------------------------------------
// Species genome expression helpers
// ---------------------------------------------------------------------------

/// Express all traits from a species-specific genome given its SNP layout.
///
/// Walks the `species_snps` config, computes bit offsets, and calls the
/// appropriate expression function for each region. Returns a list of
/// (region_name, value) pairs.
///
/// Categorical regions sharing the same `group` are expressed together:
/// only the first occurrence triggers expression, and the result is stored
/// under the group name.
///
/// `tiebreak_seed` is used for categorical tiebreaks (typically creature ID
/// XOR-folded to u64, mixed with a trait-specific salt).
pub fn express_species_genome(
    genome: &Genome,
    config: &crate::species::SpeciesGenomeConfig,
    tiebreak_seed: u64,
) -> Vec<(String, i64)> {
    use std::collections::BTreeMap;

    let mut results = Vec::new();
    let mut offset = 0u32;

    // First pass: compute offsets and group categorical regions.
    struct RegionInfo {
        start: u32,
        bits: u32,
    }
    let mut categorical_groups: BTreeMap<String, Vec<RegionInfo>> = BTreeMap::new();
    let mut continuous_regions: Vec<(String, u32, u32)> = Vec::new(); // (name, start, bits)

    for snp in &config.species_snps {
        match &snp.kind {
            crate::species::SnpKind::Continuous => {
                continuous_regions.push((snp.name.clone(), offset, snp.bits));
            }
            crate::species::SnpKind::Categorical { group } => {
                categorical_groups
                    .entry(group.clone())
                    .or_default()
                    .push(RegionInfo {
                        start: offset,
                        bits: snp.bits,
                    });
            }
        }
        offset += snp.bits;
    }

    for (group, regions) in &categorical_groups {
        let bits_per = regions[0].bits;
        debug_assert!(
            regions.iter().all(|r| r.bits == bits_per),
            "all regions in categorical group '{group}' must have the same bit width"
        );
    }

    // Express continuous regions (bit-count centered on 0, stdev 50).
    for (name, start, bits) in &continuous_regions {
        let value = express_bitcount(genome, *start, *bits, 0, 50);
        results.push((name.clone(), value));
    }

    // Express categorical groups.
    for (group, regions) in &categorical_groups {
        // All regions in a group must have the same bit width.
        let bits_per = regions[0].bits;
        let num_categories = regions.len() as u32;
        let group_start = regions[0].start;

        // Mix tiebreak seed with group name for per-trait determinism.
        let group_hash = group
            .bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let seed = tiebreak_seed ^ group_hash;

        let winner = express_categorical(genome, group_start, bits_per, num_categories, seed);
        results.push((group.clone(), winner as i64));
    }

    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Genome bit manipulation --

    #[test]
    fn test_genome_new_zeroed() {
        let g = Genome::new(16);
        assert_eq!(g.bit_len(), 16);
        assert_eq!(g.bytes.len(), 2);
        for i in 0..16 {
            assert!(!g.get_bit(i));
        }
    }

    #[test]
    fn test_genome_set_get_bit() {
        let mut g = Genome::new(24);
        g.set_bit(0, true);
        g.set_bit(7, true);
        g.set_bit(8, true);
        g.set_bit(23, true);
        assert!(g.get_bit(0));
        assert!(!g.get_bit(1));
        assert!(g.get_bit(7));
        assert!(g.get_bit(8));
        assert!(!g.get_bit(9));
        assert!(g.get_bit(23));

        // Clear a bit.
        g.set_bit(7, false);
        assert!(!g.get_bit(7));
    }

    #[test]
    fn test_genome_non_byte_aligned() {
        // 10 bits = 2 bytes, last byte has 2 valid bits.
        let mut g = Genome::new(10);
        g.set_bit(9, true);
        assert!(g.get_bit(9));
        assert!(!g.get_bit(8));
    }

    #[test]
    #[should_panic(expected = "bit index")]
    fn test_genome_get_bit_out_of_range() {
        let g = Genome::new(8);
        g.get_bit(8);
    }

    // -- count_ones_in_region --

    #[test]
    fn test_count_ones_in_region() {
        let mut g = Genome::new(16);
        // Set bits 2, 3, 4 in region [0..8).
        g.set_bit(2, true);
        g.set_bit(3, true);
        g.set_bit(4, true);
        // Set bit 10 in region [8..16).
        g.set_bit(10, true);

        assert_eq!(g.count_ones_in_region(0, 8), 3);
        assert_eq!(g.count_ones_in_region(8, 8), 1);
        assert_eq!(g.count_ones_in_region(0, 16), 4);
        assert_eq!(g.count_ones_in_region(2, 3), 3); // bits 2,3,4 all set
        assert_eq!(g.count_ones_in_region(5, 3), 0); // bits 5,6,7 all clear
    }

    // -- weighted_sum_in_region --

    #[test]
    fn test_weighted_sum_in_region() {
        let mut g = Genome::new(8);
        g.set_bit(0, true); // weight 10
        g.set_bit(2, true); // weight 30
        g.set_bit(4, true); // weight 50

        let weights: [i64; 8] = [10, 20, 30, 40, 50, 60, 70, 80];
        assert_eq!(g.weighted_sum_in_region(0, 8, &weights), 10 + 30 + 50);
    }

    #[test]
    fn test_weighted_sum_all_ones() {
        let mut g = Genome::new(32);
        for i in 0..32 {
            g.set_bit(i, true);
        }
        assert_eq!(g.weighted_sum_in_region(0, 32, &PRIME_WEIGHTS), WEIGHT_SUM);
    }

    #[test]
    fn test_weighted_sum_all_zeros() {
        let g = Genome::new(32);
        assert_eq!(g.weighted_sum_in_region(0, 32, &PRIME_WEIGHTS), 0);
    }

    // -- Genome::random --

    #[test]
    fn test_genome_random_deterministic() {
        let mut rng1 = GameRng::new(42);
        let mut rng2 = GameRng::new(42);
        let g1 = Genome::random(&mut rng1, GENERIC_GENOME_BITS);
        let g2 = Genome::random(&mut rng2, GENERIC_GENOME_BITS);
        assert_eq!(g1, g2);
    }

    #[test]
    fn test_genome_random_trailing_bits_masked() {
        let mut rng = GameRng::new(99);
        let g = Genome::random(&mut rng, 10); // 10 bits = 2 bytes
        // Bits 10-15 in the second byte should be zero.
        assert_eq!(g.bytes[1] & 0b11111100, 0);
    }

    #[test]
    fn test_genome_random_has_variety() {
        // A random 296-bit genome should have a mix of 0s and 1s.
        let mut rng = GameRng::new(123);
        let g = Genome::random(&mut rng, GENERIC_GENOME_BITS);
        let ones = g.count_ones_in_region(0, GENERIC_GENOME_BITS);
        // With 296 bits and 50% chance, expect ~148 ones.
        // Allow wide range to avoid flakiness.
        assert!(
            ones > 100 && ones < 200,
            "expected ~148 ones in 296 bits, got {ones}"
        );
    }

    // -- Genome::from_parents --

    #[test]
    fn test_from_parents_same_parents_produces_same_genome() {
        let mut rng = GameRng::new(42);
        let parent = Genome::random(&mut rng, 64);
        // If both parents are identical, offspring should be identical
        // (before mutation).
        let mut rng2 = GameRng::new(99);
        let child = Genome::from_parents(&parent, &parent, &mut rng2, 0);
        assert_eq!(child, parent);
    }

    #[test]
    fn test_from_parents_inherits_bits_from_both() {
        let mut rng = GameRng::new(42);
        // Parent A: all zeros. Parent B: all ones.
        let parent_a = Genome::new(64);
        let mut parent_b = Genome::new(64);
        for i in 0..64 {
            parent_b.set_bit(i, true);
        }

        let child = Genome::from_parents(&parent_a, &parent_b, &mut rng, 0);
        let ones = child.count_ones_in_region(0, 64);
        // Should be roughly 50% ones (from parent B).
        assert!(
            ones > 20 && ones < 44,
            "expected ~32 ones from 50/50 crossover, got {ones}"
        );
    }

    // -- backfill_to --

    #[test]
    fn test_backfill_extends_genome() {
        let mut g = Genome::new(8);
        g.set_bit(0, true);
        g.set_bit(7, true);

        g.backfill_to(16, 12345);
        assert_eq!(g.bit_len(), 16);
        // Original bits preserved.
        assert!(g.get_bit(0));
        assert!(g.get_bit(7));
        // New bits are deterministic.
        let mut g2 = Genome::new(8);
        g2.set_bit(0, true);
        g2.set_bit(7, true);
        g2.backfill_to(16, 12345);
        assert_eq!(g, g2);
    }

    #[test]
    fn test_backfill_noop_if_already_big_enough() {
        let mut rng = GameRng::new(42);
        let g = Genome::random(&mut rng, 32);
        let mut g2 = g.clone();
        g2.backfill_to(32, 999);
        assert_eq!(g, g2);
        g2.backfill_to(16, 999);
        assert_eq!(g, g2);
    }

    #[test]
    fn test_backfill_different_seeds_different_bits() {
        let mut g1 = Genome::new(8);
        let mut g2 = Genome::new(8);
        g1.backfill_to(64, 111);
        g2.backfill_to(64, 222);
        // The original 8 bits are the same (all zero), but backfilled bits differ.
        let ones1 = g1.count_ones_in_region(8, 56);
        let ones2 = g2.count_ones_in_region(8, 56);
        // Very unlikely to be exactly equal with different seeds.
        assert_ne!(
            ones1, ones2,
            "different seeds should produce different backfill"
        );
    }

    // -- express_stat --

    #[test]
    fn test_express_stat_all_zeros() {
        let g = Genome::new(GENERIC_GENOME_BITS);
        // All zeros: raw = 0, centered = -EXPECTED_MEAN.
        // Result = -EXPECTED_MEAN * stdev / THEORETICAL_STDEV + mean
        let val = express_stat(&g, 0, 0, 50);
        // Should be well below the mean.
        assert!(
            val < -100,
            "all-zero bits should produce low stat, got {val}"
        );
    }

    #[test]
    fn test_express_stat_all_ones() {
        let mut g = Genome::new(GENERIC_GENOME_BITS);
        for i in 0..STAT_BITS {
            g.set_bit(i, true);
        }
        let val = express_stat(&g, 0, 0, 50);
        // Should be well above the mean.
        assert!(
            val > 100,
            "all-one bits should produce high stat, got {val}"
        );
    }

    #[test]
    fn test_express_stat_distribution() {
        // Generate many random genomes and check the distribution of expressed stats.
        let mut rng = GameRng::new(777);
        let species_mean = 0i64;
        let species_stdev = 50i64;
        let n = 10_000;
        let mut sum = 0i64;
        let mut sum_sq = 0i128;

        for _ in 0..n {
            let g = Genome::random(&mut rng, GENERIC_GENOME_BITS);
            let val = express_stat(&g, 0, species_mean, species_stdev);
            sum += val;
            sum_sq += (val as i128) * (val as i128);
        }

        let mean = sum as f64 / n as f64;
        let variance = sum_sq as f64 / n as f64 - mean * mean;
        let stdev = variance.sqrt();

        // Mean should be near species_mean (0).
        assert!(
            mean.abs() < 3.0,
            "mean {mean:.1} too far from species_mean {species_mean}"
        );
        // Stdev should be near species_stdev (50).
        assert!(
            (stdev - species_stdev as f64).abs() < 5.0,
            "stdev {stdev:.1} too far from species_stdev {species_stdev}"
        );
    }

    #[test]
    fn test_express_stat_covers_five_sigma() {
        // With mean=0 stdev=50, 5-sigma range is [-250, +250].
        // All-zeros and all-ones should be near or beyond 5-sigma.
        let g_lo = Genome::new(GENERIC_GENOME_BITS);
        let val_lo = express_stat(&g_lo, 0, 0, 50);

        let mut g_hi = Genome::new(GENERIC_GENOME_BITS);
        for i in 0..STAT_BITS {
            g_hi.set_bit(i, true);
        }
        let val_hi = express_stat(&g_hi, 0, 0, 50);

        let range = val_hi - val_lo;
        // Should span at least 400 (8 sigma for stdev=50).
        assert!(
            range >= 400,
            "stat range {val_lo}..{val_hi} = {range}, expected >= 400"
        );
    }

    // -- express_bitcount --

    #[test]
    fn test_express_bitcount_personality() {
        // 8 bits, all zeros → minimum value.
        let g = Genome::new(GENERIC_GENOME_BITS);
        let lo = express_bitcount(&g, personality_offset(OPENNESS), PERSONALITY_BITS, 0, 50);

        // 8 bits, all ones → maximum value.
        let mut g2 = Genome::new(GENERIC_GENOME_BITS);
        for i in 0..PERSONALITY_BITS {
            g2.set_bit(personality_offset(OPENNESS) + i, true);
        }
        let hi = express_bitcount(&g2, personality_offset(OPENNESS), PERSONALITY_BITS, 0, 50);

        // Should be symmetric around 0.
        assert!(lo < 0, "all-zero bitcount should be negative, got {lo}");
        assert!(hi > 0, "all-one bitcount should be positive, got {hi}");
        assert_eq!(lo, -hi, "should be symmetric: lo={lo}, hi={hi}");
    }

    // -- express_categorical --

    #[test]
    fn test_express_categorical_clear_winner() {
        // 3 categories × 4 bits each. Set all bits in category 1.
        let mut g = Genome::new(12);
        for i in 4..8 {
            g.set_bit(i, true);
        }
        let winner = express_categorical(&g, 0, 4, 3, 0);
        assert_eq!(winner, 1);
    }

    #[test]
    fn test_express_categorical_tiebreak_deterministic() {
        // Two categories tied at 2 ones each.
        let mut g = Genome::new(8);
        g.set_bit(0, true);
        g.set_bit(1, true); // category 0: 2 ones
        g.set_bit(4, true);
        g.set_bit(5, true); // category 1: 2 ones

        let w1 = express_categorical(&g, 0, 4, 2, 42);
        let w2 = express_categorical(&g, 0, 4, 2, 42);
        assert_eq!(w1, w2, "tiebreak should be deterministic");
    }

    #[test]
    fn test_express_categorical_different_seeds_can_differ() {
        // Two categories tied — different tiebreak seeds should (usually) pick differently.
        let mut g = Genome::new(8);
        g.set_bit(0, true);
        g.set_bit(1, true);
        g.set_bit(4, true);
        g.set_bit(5, true);

        let mut results = std::collections::BTreeSet::new();
        for seed in 0..100u64 {
            results.insert(express_categorical(&g, 0, 4, 2, seed));
        }
        // With 100 different seeds breaking a 2-way tie, we should see both categories.
        assert_eq!(results.len(), 2, "different seeds should produce variety");
    }

    // -- PRIME_WEIGHTS validation --

    #[test]
    fn test_prime_weights_in_range() {
        for (i, &w) in PRIME_WEIGHTS.iter().enumerate() {
            assert!(
                w >= 1_000_000 && w <= 5_000_000,
                "PRIME_WEIGHTS[{i}] = {w} out of [1M, 5M]"
            );
        }
    }

    #[test]
    fn test_prime_weights_all_distinct() {
        let mut sorted = PRIME_WEIGHTS;
        sorted.sort();
        for pair in sorted.windows(2) {
            assert_ne!(pair[0], pair[1], "duplicate weight found: {}", pair[0]);
        }
    }

    #[test]
    fn test_prime_weights_are_prime() {
        let mut bad = Vec::new();
        for (i, &w) in PRIME_WEIGHTS.iter().enumerate() {
            if !is_prime(w) {
                // Find next prime.
                let mut candidate = w + 1;
                while !is_prime(candidate) {
                    candidate += 1;
                }
                bad.push((i, w, candidate));
            }
        }
        assert!(
            bad.is_empty(),
            "Non-prime weights found (index, value, next_prime): {bad:?}"
        );
    }

    /// Simple primality test (trial division) — only used in tests.
    fn is_prime(n: i64) -> bool {
        if n < 2 {
            return false;
        }
        let n = n as u64;
        if n % 2 == 0 {
            return n == 2;
        }
        let mut d = 3u64;
        while d * d <= n {
            if n % d == 0 {
                return false;
            }
            d += 2;
        }
        true
    }

    #[test]
    fn test_weight_sum_and_expected_mean() {
        let manual_sum: i64 = PRIME_WEIGHTS.iter().sum();
        assert_eq!(WEIGHT_SUM, manual_sum);
        assert_eq!(EXPECTED_MEAN, manual_sum / 2);
    }

    #[test]
    fn test_theoretical_stdev_reasonable() {
        // Compute manually using f64 for comparison.
        let sum_sq: f64 = PRIME_WEIGHTS.iter().map(|&w| (w as f64) * (w as f64)).sum();
        let expected = (sum_sq.sqrt() / 2.0).round() as i64;
        // Allow ±1 for integer rounding.
        assert!(
            (THEORETICAL_STDEV - expected).abs() <= 1,
            "THEORETICAL_STDEV = {}, expected ~{expected}",
            THEORETICAL_STDEV
        );
    }

    // -- Categorical weight validation --

    #[test]
    fn test_categorical_weights_are_prime() {
        for &w in CATEGORICAL_WEIGHTS_4
            .iter()
            .chain(CATEGORICAL_WEIGHTS_6.iter())
        {
            assert!(is_prime(w), "categorical weight {w} is not prime");
        }
    }

    #[test]
    fn test_categorical_weights_distinct_within_table() {
        for (name, table) in [
            ("4-bit", CATEGORICAL_WEIGHTS_4.as_slice()),
            ("6-bit", CATEGORICAL_WEIGHTS_6.as_slice()),
        ] {
            let mut sorted = table.to_vec();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                table.len(),
                sorted.len(),
                "{name} categorical weights must be distinct"
            );
        }
    }

    // -- Blended categorical expression --

    #[test]
    fn test_blended_adjacent_produces_blend() {
        // 3 categories × 4 bits. Set bits so cat 0 and cat 1 both score high.
        let mut g = Genome::new(12);
        // Cat 0: set bits 0,1,2 (weights 1013+1597+2311 = 4921)
        g.set_bit(0, true);
        g.set_bit(1, true);
        g.set_bit(2, true);
        // Cat 1: set bits 4,5,6 (weights 1013+1597+2311 = 4921)
        g.set_bit(4, true);
        g.set_bit(5, true);
        g.set_bit(6, true);
        // Cat 2: nothing

        let result = express_categorical_blended(&g, 0, 4, 3, 42);
        match result {
            CategoricalResult::Blend {
                primary,
                secondary,
                weight,
            } => {
                // Both should score equally → weight ≈ 127 (50/50).
                assert!(
                    (primary == 0 && secondary == 1) || (primary == 1 && secondary == 0),
                    "should blend cats 0 and 1, got {primary} and {secondary}"
                );
                assert!(
                    (120..=135).contains(&weight),
                    "equal scores should give ~50% blend weight, got {weight}"
                );
            }
            CategoricalResult::Single(_) => {
                panic!("adjacent top-two should blend, not single");
            }
        }
    }

    #[test]
    fn test_blended_non_adjacent_produces_single() {
        // 4 categories × 4 bits. Set bits so cat 0 and cat 2 score high (non-adjacent).
        let mut g = Genome::new(16);
        g.set_bit(0, true);
        g.set_bit(1, true); // cat 0
        g.set_bit(8, true);
        g.set_bit(9, true); // cat 2
        // Cat 1 and 3: nothing

        let result = express_categorical_blended(&g, 0, 4, 4, 42);
        assert!(
            matches!(result, CategoricalResult::Single(_)),
            "non-adjacent top-two should produce Single, got {result:?}"
        );
    }

    #[test]
    fn test_blended_wrap_around_adjacent() {
        // 4 categories × 4 bits. Cat 0 and cat 3 are adjacent (wrap-around).
        let mut g = Genome::new(16);
        g.set_bit(0, true);
        g.set_bit(1, true); // cat 0
        g.set_bit(12, true);
        g.set_bit(13, true); // cat 3
        // Cat 1 and 2: nothing

        let result = express_categorical_blended(&g, 0, 4, 4, 42);
        match result {
            CategoricalResult::Blend {
                primary, secondary, ..
            } => {
                assert!(
                    (primary == 0 && secondary == 3) || (primary == 3 && secondary == 0),
                    "should blend wrap-around adjacent cats 0 and 3"
                );
            }
            CategoricalResult::Single(_) => {
                panic!("wrap-around adjacent should blend");
            }
        }
    }

    #[test]
    fn test_blended_second_score_zero() {
        // 3 categories × 4 bits. Only category 1 has bits set; others are zero.
        // Second-highest score is 0, so result should be Single.
        let mut g = Genome::new(12);
        // Cat 1: set all bits (weights 1013+1597+2311+3109 = 8030)
        for i in 4..8 {
            g.set_bit(i, true);
        }
        // Cat 0 and cat 2: all zero

        let result = express_categorical_blended(&g, 0, 4, 3, 42);
        assert!(
            matches!(result, CategoricalResult::Single(1)),
            "only one category with bits set should produce Single(1), got {result:?}"
        );
    }

    // -- from_parents mutation --

    #[test]
    fn test_from_parents_mutation_flips_bits() {
        let mut rng = GameRng::new(42);
        let parent = Genome::random(&mut rng, 64);
        // No mutation: child should equal parent (both parents identical).
        let mut rng2 = GameRng::new(99);
        let no_mut = Genome::from_parents(&parent, &parent, &mut rng2, 0);
        // High mutation: child should differ from parent.
        let mut rng3 = GameRng::new(99);
        let high_mut = Genome::from_parents(&parent, &parent, &mut rng3, 500); // 50% mutation
        assert_ne!(
            no_mut, high_mut,
            "high mutation rate should produce different genome"
        );
    }

    // -- express_species_genome --

    #[test]
    fn test_express_species_genome_returns_expected_regions() {
        use crate::species::{SnpKind, SnpRegion, SpeciesGenomeConfig};

        let config = SpeciesGenomeConfig {
            stat_bits: 32,
            personality_bits: 8,
            species_snps: vec![
                SnpRegion {
                    name: "brightness".into(),
                    bits: 6,
                    kind: SnpKind::Continuous,
                },
                SnpRegion {
                    name: "size".into(),
                    bits: 6,
                    kind: SnpKind::Continuous,
                },
                // 3 categories × 4 bits for "color" group.
                SnpRegion {
                    name: "color_warm".into(),
                    bits: 4,
                    kind: SnpKind::Categorical {
                        group: "color".into(),
                    },
                },
                SnpRegion {
                    name: "color_cool".into(),
                    bits: 4,
                    kind: SnpKind::Categorical {
                        group: "color".into(),
                    },
                },
                SnpRegion {
                    name: "color_neutral".into(),
                    bits: 4,
                    kind: SnpKind::Categorical {
                        group: "color".into(),
                    },
                },
            ],
        };

        // Total species bits: 6 + 6 + 4*3 = 24.
        let total_bits = crate::genome::GENERIC_GENOME_BITS + 24;
        let mut rng = GameRng::new(42);
        let genome = Genome::random(&mut rng, total_bits);

        let results = express_species_genome(&genome, &config, 123);

        // 2 continuous + 1 categorical group = 3 results.
        assert_eq!(results.len(), 3, "expected 3 results, got {:?}", results);

        // Find the categorical group result.
        let color_result = results.iter().find(|(name, _)| name == "color");
        assert!(color_result.is_some(), "missing 'color' group result");
        let (_, cat_value) = color_result.unwrap();
        assert!(
            *cat_value >= 0 && *cat_value < 3,
            "categorical value should be in 0..3, got {cat_value}"
        );
    }

    // -- express_categorical_blended asymmetric weight --

    #[test]
    fn test_blended_asymmetric_weight() {
        // 2 adjacent categories × 4 bits. Cat 0 has all 4 bits set (high score),
        // cat 1 has only 1 bit set (low score).
        let mut g = Genome::new(8);
        // Cat 0: all bits set.
        for i in 0..4 {
            g.set_bit(i, true);
        }
        // Cat 1: only bit 4 set.
        g.set_bit(4, true);

        let result = express_categorical_blended(&g, 0, 4, 2, 42);
        match result {
            CategoricalResult::Blend {
                primary, weight, ..
            } => {
                assert_eq!(primary, 0, "primary should be cat 0 (highest score)");
                assert!(
                    weight < 80,
                    "weight should be small (heavily toward primary), got {weight}"
                );
            }
            CategoricalResult::Single(cat) => {
                // Also acceptable if second score is too low to blend.
                assert_eq!(cat, 0, "single winner should be cat 0");
            }
        }
    }

    // -- Serde roundtrip --

    #[test]
    fn test_genome_serde_roundtrip() {
        let mut rng = GameRng::new(42);
        let g = Genome::random(&mut rng, GENERIC_GENOME_BITS);
        let json = serde_json::to_string(&g).unwrap();
        let restored: Genome = serde_json::from_str(&json).unwrap();
        assert_eq!(g, restored);
    }
}
