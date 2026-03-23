// Deterministic fixed-point arithmetic types.
//
// Provides `Fixed64`, a scalar fixed-point number stored as `i64` with
// 2^30 fractional units (~1.07 billion subdivisions per integer unit),
// and `FixedVec3`, a 3D vector using the same representation per axis.
//
// These types guarantee cross-platform determinism: all arithmetic is
// pure integer (no floating-point instructions), so results are identical
// regardless of CPU architecture, compiler version, or optimization level.
//
// ## Precision
//
// With 30 fractional bits the resolution is ~9.3×10⁻¹⁰ per unit, and the
// integer range is ±2^33 (~±8.6 billion). Intermediate products that
// exceed i64 range (e.g., Fixed64 × Fixed64) are computed in i128 before
// shifting back.
//
// ## Usage in Elven Canopy
//
// - `elven_canopy_sim` uses `FixedVec3` as `SubVoxelCoord` for projectile
//   ballistics (2^30 sub-voxel units per voxel).
// - `elven_canopy_music` uses `Fixed64` as `Score` for deterministic music
//   composition scoring.
//
// ## Serde
//
// `Fixed64` serializes as a JSON array `[integer_part, fractional_bits]`
// split at the 2^30 boundary, avoiding JSON's 2^53 precision limit for
// large accumulated values. `FixedVec3` serializes as `{"x": i64, "y":
// i64, "z": i64}` — the raw sub-unit values always fit in JSON numbers
// for practical coordinate ranges.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of fractional bits in the fixed-point representation.
pub const FRAC_SHIFT: u32 = 30;

/// One integer unit in fixed-point representation (2^30 = 1_073_741_824).
pub const FRAC_ONE: i64 = 1i64 << FRAC_SHIFT;

// ---------------------------------------------------------------------------
// Fixed64 — scalar fixed-point
// ---------------------------------------------------------------------------

/// A fixed-point scalar: `i64` with 30 fractional bits.
///
/// The raw value is `integer_value * 2^30 + fractional_bits`. For example,
/// `Fixed64::from_int(3)` stores `3 * 2^30 = 3_221_225_472`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fixed64(pub i64);

impl Fixed64 {
    pub const ZERO: Fixed64 = Fixed64(0);
    pub const ONE: Fixed64 = Fixed64(FRAC_ONE);

    /// Create from a raw internal value (already in fixed-point representation).
    #[inline]
    pub const fn from_raw(raw: i64) -> Self {
        Fixed64(raw)
    }

    /// Create from an integer value (shifts left by FRAC_SHIFT).
    #[inline]
    pub const fn from_int(n: i64) -> Self {
        Fixed64(n << FRAC_SHIFT)
    }

    /// Create from a ratio `numerator / denominator`, computed in i128
    /// to avoid overflow.
    #[inline]
    pub const fn from_ratio(numerator: i64, denominator: i64) -> Self {
        Fixed64(((numerator as i128 * FRAC_ONE as i128) / denominator as i128) as i64)
    }

    /// Get the raw internal value.
    #[inline]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Convert to f64 for display purposes. NOT for deterministic computation.
    #[inline]
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / FRAC_ONE as f64
    }

    /// Multiply two Fixed64 values. Uses i128 intermediate to avoid overflow.
    #[inline]
    pub const fn mul_fixed(self, rhs: Fixed64) -> Fixed64 {
        Fixed64(((self.0 as i128 * rhs.0 as i128) >> FRAC_SHIFT) as i64)
    }

    /// Multiply by a plain integer.
    #[inline]
    pub const fn mul_int(self, n: i64) -> Fixed64 {
        Fixed64(self.0 * n)
    }

    /// Divide by another Fixed64. Uses i128 intermediate.
    #[inline]
    pub const fn div_fixed(self, rhs: Fixed64) -> Fixed64 {
        Fixed64((((self.0 as i128) << FRAC_SHIFT) / rhs.0 as i128) as i64)
    }

    /// Divide by a plain integer.
    #[inline]
    pub const fn div_int(self, n: i64) -> Fixed64 {
        Fixed64(self.0 / n)
    }

    /// Absolute value.
    #[inline]
    pub const fn abs(self) -> Fixed64 {
        if self.0 < 0 { Fixed64(-self.0) } else { self }
    }
}

// --- Arithmetic operator impls ---

impl std::ops::Add for Fixed64 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Fixed64(self.0 + rhs.0)
    }
}

impl std::ops::AddAssign for Fixed64 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl std::ops::Sub for Fixed64 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Fixed64(self.0 - rhs.0)
    }
}

impl std::ops::SubAssign for Fixed64 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl std::ops::Neg for Fixed64 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Fixed64(-self.0)
    }
}

impl std::iter::Sum for Fixed64 {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Fixed64::ZERO, |acc, x| acc + x)
    }
}

// --- Display ---

impl fmt::Debug for Fixed64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fixed64({:.6})", self.to_f64())
    }
}

impl fmt::Display for Fixed64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.4}", self.to_f64())
    }
}

// --- Serde: [integer_part, fractional_bits] ---

impl Serialize for Fixed64 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Split: integer part = raw >> FRAC_SHIFT, frac = raw & (FRAC_ONE - 1)
        // But for negative values we need to preserve the sign correctly.
        // Simplest: serialize the raw i64 as a two-element array [hi, lo]
        // where hi = raw >> 30 (arithmetic shift, signed) and lo = raw & mask.
        let hi = self.0 >> FRAC_SHIFT;
        let lo = self.0 & (FRAC_ONE - 1);
        (hi, lo).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Fixed64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (hi, lo): (i64, i64) = Deserialize::deserialize(deserializer)?;
        Ok(Fixed64((hi << FRAC_SHIFT) | (lo & (FRAC_ONE - 1))))
    }
}

// ---------------------------------------------------------------------------
// FixedVec3 — 3D vector at fixed-point scale
// ---------------------------------------------------------------------------

/// A 3D vector with each axis stored as `i64` in fixed-point representation
/// (2^30 sub-units per integer unit).
///
/// Used for high-precision integer positions and velocities. Serializes as
/// raw `{x, y, z}` i64 values (always fit in JSON number range for practical
/// coordinate values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FixedVec3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

impl FixedVec3 {
    #[inline]
    pub const fn new(x: i64, y: i64, z: i64) -> Self {
        Self { x, y, z }
    }

    /// Squared magnitude using i128 to avoid overflow.
    /// Components are i64; squaring produces up to ~2^60 per axis,
    /// and summing three gives up to ~3×2^60 — fits in i128.
    #[inline]
    pub fn magnitude_sq(self) -> i128 {
        let x = self.x as i128;
        let y = self.y as i128;
        let z = self.z as i128;
        x * x + y * y + z * z
    }

    /// Squared magnitude as `i64`. Panics in debug builds if the value
    /// would overflow `i64`.
    #[inline]
    pub fn magnitude_sq_i64(self) -> i64 {
        let sq = self.magnitude_sq();
        debug_assert!(
            sq <= i64::MAX as i128,
            "magnitude_sq overflows i64: {sq} (components: {}, {}, {})",
            self.x,
            self.y,
            self.z
        );
        sq as i64
    }

    /// Convert to floating-point for rendering. NOT for deterministic computation.
    #[inline]
    pub fn to_render_floats(self) -> (f32, f32, f32) {
        let scale = FRAC_ONE as f64;
        (
            (self.x as f64 / scale) as f32,
            (self.y as f64 / scale) as f32,
            (self.z as f64 / scale) as f32,
        )
    }
}

impl std::ops::Add for FixedVec3 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl std::ops::AddAssign for FixedVec3 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl std::ops::Sub for FixedVec3 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

// ---------------------------------------------------------------------------
// Integer square root
// ---------------------------------------------------------------------------

/// Integer square root of a non-negative i128 using Newton's method.
/// Returns floor(sqrt(n)). Deterministic, no floating-point.
pub fn isqrt_i128(n: i128) -> i128 {
    if n <= 0 {
        return 0;
    }
    if n == 1 {
        return 1;
    }

    // Initial guess: use bit length to get in the right ballpark.
    // sqrt(n) has roughly half the bits of n.
    let bits = 128 - n.leading_zeros();
    let mut x = 1i128 << (bits.div_ceil(2));

    // Newton iterations: x_{n+1} = (x_n + n / x_n) / 2
    loop {
        let next = (x + n / x) / 2;
        if next >= x {
            break;
        }
        x = next;
    }
    x
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Fixed64 basics ---

    #[test]
    fn from_int_roundtrip() {
        for n in [-50, -1, 0, 1, 42, 1000] {
            let f = Fixed64::from_int(n);
            assert_eq!(f.0 >> FRAC_SHIFT, n, "from_int({n}) integer part wrong");
            assert_eq!(f.0 & (FRAC_ONE - 1), 0, "from_int({n}) has fractional bits");
        }
    }

    #[test]
    fn from_ratio_basic() {
        // 1/2 = 0.5 → raw should be FRAC_ONE / 2
        let half = Fixed64::from_ratio(1, 2);
        assert_eq!(half.0, FRAC_ONE / 2);

        // 3/10 = 0.3 → raw ≈ 0.3 * 2^30
        let third = Fixed64::from_ratio(3, 10);
        let expected = (3i128 * FRAC_ONE as i128 / 10) as i64;
        assert_eq!(third.0, expected);
    }

    #[test]
    fn from_ratio_negative() {
        let neg_half = Fixed64::from_ratio(-1, 2);
        assert_eq!(neg_half.0, -(FRAC_ONE / 2));
    }

    #[test]
    fn add_sub() {
        let a = Fixed64::from_int(3);
        let b = Fixed64::from_int(7);
        assert_eq!((a + b).0, Fixed64::from_int(10).0);
        assert_eq!((b - a).0, Fixed64::from_int(4).0);
    }

    #[test]
    fn mul_fixed_basic() {
        let two = Fixed64::from_int(2);
        let three = Fixed64::from_int(3);
        let six = two.mul_fixed(three);
        assert_eq!(six.0, Fixed64::from_int(6).0);

        // 0.5 * 0.5 = 0.25
        let half = Fixed64::from_ratio(1, 2);
        let quarter = half.mul_fixed(half);
        let expected = Fixed64::from_ratio(1, 4);
        // Allow ±1 for rounding
        assert!(
            (quarter.0 - expected.0).abs() <= 1,
            "0.5 * 0.5: got {}, expected {}",
            quarter.0,
            expected.0
        );
    }

    #[test]
    fn mul_fixed_large_values() {
        // -50 * 2 = -100 — tests i128 intermediate
        let a = Fixed64::from_int(-50);
        let b = Fixed64::from_int(2);
        let result = a.mul_fixed(b);
        assert_eq!(result.0, Fixed64::from_int(-100).0);
    }

    #[test]
    fn mul_int_basic() {
        let a = Fixed64::from_ratio(3, 10); // 0.3 (with truncation)
        let result = a.mul_int(10);
        // from_ratio truncates: 3 * 2^30 / 10 = 322_122_547.2 → 322_122_547
        // Multiplying back by 10 gives 3_221_225_470, not 3 * 2^30 = 3_221_225_472.
        // This 2-LSB rounding error is inherent to fixed-point representation.
        let error = (result.0 - Fixed64::from_int(3).0).abs();
        assert!(error <= 10, "mul_int rounding error too large: {error}");
    }

    #[test]
    fn div_fixed_basic() {
        let six = Fixed64::from_int(6);
        let two = Fixed64::from_int(2);
        let three = six.div_fixed(two);
        assert_eq!(three.0, Fixed64::from_int(3).0);
    }

    #[test]
    fn div_int_basic() {
        let six = Fixed64::from_int(6);
        let three = six.div_int(2);
        assert_eq!(three.0, Fixed64::from_int(3).0);
    }

    #[test]
    fn neg_and_abs() {
        let pos = Fixed64::from_int(5);
        let neg = -pos;
        assert_eq!(neg.0, Fixed64::from_int(-5).0);
        assert_eq!(neg.abs().0, pos.0);
        assert_eq!(pos.abs().0, pos.0);
    }

    #[test]
    fn sum_iterator() {
        let values = [
            Fixed64::from_int(1),
            Fixed64::from_int(2),
            Fixed64::from_int(3),
        ];
        let total: Fixed64 = values.into_iter().sum();
        assert_eq!(total.0, Fixed64::from_int(6).0);
    }

    #[test]
    fn ordering() {
        let a = Fixed64::from_int(-1);
        let b = Fixed64::ZERO;
        let c = Fixed64::from_int(1);
        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
    }

    #[test]
    fn to_f64_display() {
        let half = Fixed64::from_ratio(1, 2);
        let val = half.to_f64();
        assert!((val - 0.5).abs() < 1e-9);
    }

    // --- Fixed64 serde ---

    #[test]
    fn serde_roundtrip_positive() {
        let original = Fixed64::from_int(42);
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "[42,0]");
        let decoded: Fixed64 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn serde_roundtrip_negative() {
        let original = Fixed64::from_int(-50);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Fixed64 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn serde_roundtrip_fractional() {
        let original = Fixed64::from_ratio(3, 10);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Fixed64 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn serde_roundtrip_negative_fractional() {
        // -0.7 = from_ratio(-7, 10)
        let original = Fixed64::from_ratio(-7, 10);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Fixed64 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn serde_roundtrip_negative_half() {
        // -0.5: hi = -1, lo = FRAC_ONE / 2. Exercises the arithmetic right-shift
        // path where hi is negative and lo is a large positive number.
        let original = Fixed64::from_ratio(-1, 2);
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(json, "[-1,536870912]");
        let decoded: Fixed64 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn serde_roundtrip_smallest_negative() {
        // from_raw(-1): the smallest possible negative value.
        let original = Fixed64::from_raw(-1);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Fixed64 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    // --- FixedVec3 basics ---

    #[test]
    fn vec3_add_sub() {
        let a = FixedVec3::new(10, 20, 30);
        let b = FixedVec3::new(3, 7, 11);
        assert_eq!(a + b, FixedVec3::new(13, 27, 41));
        assert_eq!(a - b, FixedVec3::new(7, 13, 19));
    }

    #[test]
    fn vec3_add_assign() {
        let mut a = FixedVec3::new(10, 20, 30);
        a += FixedVec3::new(1, 2, 3);
        assert_eq!(a, FixedVec3::new(11, 22, 33));
    }

    #[test]
    fn vec3_magnitude_sq() {
        let v = FixedVec3::new(3, 4, 0);
        assert_eq!(v.magnitude_sq(), 25);

        let v = FixedVec3::new(FRAC_ONE, 0, 0);
        assert_eq!(v.magnitude_sq(), (FRAC_ONE as i128).pow(2));
    }

    #[test]
    fn vec3_magnitude_sq_i64() {
        let v = FixedVec3::new(1000, 2000, 3000);
        let sq_i128 = v.magnitude_sq();
        let sq_i64 = v.magnitude_sq_i64();
        assert_eq!(sq_i128, sq_i64 as i128);
    }

    #[test]
    fn vec3_to_render_floats() {
        let pos = FixedVec3::new(FRAC_ONE * 3 / 2, FRAC_ONE * 5, 0);
        let (rx, ry, rz) = pos.to_render_floats();
        assert!((rx - 1.5).abs() < 0.001);
        assert!((ry - 5.0).abs() < 0.001);
        assert!(rz.abs() < 0.001);
    }

    // --- FixedVec3 serde ---

    #[test]
    fn vec3_serde_roundtrip() {
        let original = FixedVec3::new(-12345, FRAC_ONE * 50, 999999);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: FixedVec3 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    // --- isqrt_i128 ---

    #[test]
    fn isqrt_exact() {
        assert_eq!(isqrt_i128(0), 0);
        assert_eq!(isqrt_i128(1), 1);
        assert_eq!(isqrt_i128(4), 2);
        assert_eq!(isqrt_i128(9), 3);
        assert_eq!(isqrt_i128(100), 10);
        assert_eq!(isqrt_i128(1_000_000), 1000);
    }

    #[test]
    fn isqrt_non_exact() {
        assert_eq!(isqrt_i128(2), 1);
        assert_eq!(isqrt_i128(8), 2);
        assert_eq!(isqrt_i128(99), 9);
    }

    #[test]
    fn isqrt_large() {
        let n = 1i128 << 60;
        assert_eq!(isqrt_i128(n), 1i128 << 30);

        let n = (1i128 << 60) + 1;
        let s = isqrt_i128(n);
        assert!(s * s <= n);
        assert!((s + 1) * (s + 1) > n);
    }

    #[test]
    fn isqrt_very_large() {
        let n = 1i128 << 120;
        assert_eq!(isqrt_i128(n), 1i128 << 60);

        let n = (1i128 << 120) + 42;
        let s = isqrt_i128(n);
        assert!(s * s <= n);
        assert!((s + 1) * (s + 1) > n);
    }

    #[test]
    fn isqrt_negative() {
        assert_eq!(isqrt_i128(-1), 0);
        assert_eq!(isqrt_i128(-100), 0);
    }
}
