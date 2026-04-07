//! CRC32 checksumming for per-row integrity verification.
//!
//! Provides `Crc32State` (running CRC32 hasher) and `CrcFeed` (trait for
//! feeding a value's bytes into the hasher in deterministic, non-allocating
//! order). Used by checksummed tabulosity tables for per-row CRC computation
//! and XOR-aggregated table-level checksums.
//!
//! Backed by `crc32fast`, which uses hardware-accelerated SIMD (`pclmulqdq`
//! on x86_64, CRC32 instructions on ARM) when available, falling back to a
//! software lookup table. All code paths produce identical IEEE CRC32 results,
//! so checksums are deterministic across platforms.
//!
//! `CrcFeed` is implemented for common stdlib types (primitives, `String`,
//! `Option<T>`, `Vec<T>`, `bool`, tuples, arrays). User structs on
//! checksummed tables derive `CrcFeed` via `#[derive(CrcFeed)]` from the
//! `tabulosity_derive` crate.

/// Running CRC32 state. Feed bytes via `update`, finalize with `finalize`.
///
/// Thin wrapper around `crc32fast::Hasher` for hardware-accelerated CRC32
/// (IEEE polynomial).
#[derive(Clone)]
pub struct Crc32State {
    inner: crc32fast::Hasher,
}

impl Crc32State {
    /// Creates a new CRC32 state initialized to the standard starting value.
    pub fn new() -> Self {
        Self {
            inner: crc32fast::Hasher::new(),
        }
    }

    /// Feeds a slice of bytes into the CRC32 computation.
    #[inline]
    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    /// Finalizes and returns the CRC32 value.
    #[inline]
    pub fn finalize(self) -> u32 {
        self.inner.finalize()
    }
}

impl Default for Crc32State {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for feeding a value's bytes into a `Crc32State` in a deterministic,
/// non-allocating way.
///
/// Each field is fed in declaration order. Primitives use `to_le_bytes()`.
/// Strings use length-prefix + UTF-8 bytes. `Option<T>` uses a tag byte +
/// inner. `Vec<T>` uses length-prefix + elements. Newtypes recurse into
/// the inner type. Enums use discriminant + variant fields.
///
/// Derive with `#[derive(CrcFeed)]` from the `tabulosity_derive` crate.
pub trait CrcFeed {
    fn crc_feed(&self, state: &mut Crc32State);
}

/// Convenience: compute the CRC32 of a single value.
pub fn crc32_of<T: CrcFeed>(value: &T) -> u32 {
    let mut state = Crc32State::new();
    value.crc_feed(&mut state);
    state.finalize()
}

// =============================================================================
// Blanket impls for primitives
// =============================================================================

macro_rules! impl_crc_feed_int {
    ($($ty:ty),*) => {
        $(
            impl CrcFeed for $ty {
                #[inline]
                fn crc_feed(&self, state: &mut Crc32State) {
                    state.update(&self.to_le_bytes());
                }
            }
        )*
    };
}

impl_crc_feed_int!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128);

// `usize`/`isize` are platform-dependent (4 bytes on 32-bit, 8 bytes on
// 64-bit). Normalize to u64/i64 LE for cross-platform determinism.
impl CrcFeed for usize {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        state.update(&(*self as u64).to_le_bytes());
    }
}

impl CrcFeed for isize {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        state.update(&(*self as i64).to_le_bytes());
    }
}

impl CrcFeed for bool {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        state.update(&[*self as u8]);
    }
}

impl CrcFeed for String {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        // Length prefix (as u64 LE) + UTF-8 bytes.
        state.update(&(self.len() as u64).to_le_bytes());
        state.update(self.as_bytes());
    }
}

impl CrcFeed for &str {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        state.update(&(self.len() as u64).to_le_bytes());
        state.update(self.as_bytes());
    }
}

impl<T: CrcFeed> CrcFeed for Option<T> {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        match self {
            None => state.update(&[0]),
            Some(v) => {
                state.update(&[1]);
                v.crc_feed(state);
            }
        }
    }
}

impl<T: CrcFeed> CrcFeed for Vec<T> {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        // Length prefix (as u64 LE) + elements.
        state.update(&(self.len() as u64).to_le_bytes());
        for item in self {
            item.crc_feed(state);
        }
    }
}

// Tuples up to 6 elements.
macro_rules! impl_crc_feed_tuple {
    ($($idx:tt : $T:ident),+) => {
        impl<$($T: CrcFeed),+> CrcFeed for ($($T,)+) {
            #[inline]
            fn crc_feed(&self, state: &mut Crc32State) {
                $(self.$idx.crc_feed(state);)+
            }
        }
    };
}

impl_crc_feed_tuple!(0: A);
impl_crc_feed_tuple!(0: A, 1: B);
impl_crc_feed_tuple!(0: A, 1: B, 2: C);
impl_crc_feed_tuple!(0: A, 1: B, 2: C, 3: D);
impl_crc_feed_tuple!(0: A, 1: B, 2: C, 3: D, 4: E);
impl_crc_feed_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F);

// Fixed-size arrays.
impl<T: CrcFeed, const N: usize> CrcFeed for [T; N] {
    #[inline]
    fn crc_feed(&self, state: &mut Crc32State) {
        for item in self {
            item.crc_feed(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_empty() {
        let state = Crc32State::new();
        // CRC32 of empty input is 0x00000000.
        assert_eq!(state.finalize(), 0x0000_0000);
    }

    #[test]
    fn crc32_known_value() {
        // CRC32 of "123456789" is 0xCBF43926 (IEEE standard check value).
        let mut state = Crc32State::new();
        state.update(b"123456789");
        assert_eq!(state.finalize(), 0xCBF4_3926);
    }

    #[test]
    fn crc_feed_u32() {
        let val: u32 = 42;
        let crc = crc32_of(&val);
        // Should be deterministic.
        assert_eq!(crc, crc32_of(&42u32));
        // Different value should differ.
        assert_ne!(crc, crc32_of(&43u32));
    }

    #[test]
    fn crc_feed_string() {
        let s = String::from("hello");
        let crc = crc32_of(&s);
        assert_eq!(crc, crc32_of(&String::from("hello")));
        assert_ne!(crc, crc32_of(&String::from("world")));
    }

    #[test]
    fn crc_feed_option() {
        let none: Option<u32> = None;
        let some = Some(42u32);
        assert_ne!(crc32_of(&none), crc32_of(&some));
        assert_eq!(crc32_of(&some), crc32_of(&Some(42u32)));
    }

    #[test]
    fn crc_feed_vec() {
        let v1 = vec![1u32, 2, 3];
        let v2 = vec![1u32, 2, 3];
        let v3 = vec![1u32, 2, 4];
        assert_eq!(crc32_of(&v1), crc32_of(&v2));
        assert_ne!(crc32_of(&v1), crc32_of(&v3));
    }

    #[test]
    fn crc_feed_tuple() {
        let t1 = (1u32, 2u64);
        let t2 = (1u32, 2u64);
        let t3 = (1u32, 3u64);
        assert_eq!(crc32_of(&t1), crc32_of(&t2));
        assert_ne!(crc32_of(&t1), crc32_of(&t3));
    }

    #[test]
    fn crc_feed_bool() {
        assert_ne!(crc32_of(&true), crc32_of(&false));
    }

    #[test]
    fn crc_feed_array() {
        let a1 = [1u32, 2, 3];
        let a2 = [1u32, 2, 3];
        let a3 = [1u32, 2, 4];
        assert_eq!(crc32_of(&a1), crc32_of(&a2));
        assert_ne!(crc32_of(&a1), crc32_of(&a3));
    }

    #[test]
    fn crc_feed_str_ref_matches_string() {
        let s = String::from("test");
        let sr: &str = "test";
        assert_eq!(crc32_of(&s), crc32_of(&sr));
    }

    #[test]
    fn crc_feed_string_length_prefix_prevents_collision() {
        // "ab" + "cd" as two separate strings should differ from "abcd" as one.
        let mut s1 = Crc32State::new();
        "ab".crc_feed(&mut s1);
        "cd".crc_feed(&mut s1);

        let mut s2 = Crc32State::new();
        "abcd".crc_feed(&mut s2);

        assert_ne!(s1.finalize(), s2.finalize());
    }

    #[test]
    fn crc_feed_vec_length_prefix_prevents_collision() {
        // vec![1, 2] should differ from vec![1, 2, ...more] even if bytes align.
        let v1 = vec![1u8, 2];
        let v2 = vec![1u8, 2, 0];
        assert_ne!(crc32_of(&v1), crc32_of(&v2));
    }

    #[test]
    fn crc_feed_usize_normalized_to_u64() {
        // usize feeds as u64 LE for cross-platform determinism.
        assert_eq!(crc32_of(&42usize), crc32_of(&42u64));
        assert_eq!(crc32_of(&0usize), crc32_of(&0u64));
        assert_eq!(crc32_of(&usize::MAX), crc32_of(&(usize::MAX as u64)));
    }

    #[test]
    fn crc_feed_isize_normalized_to_i64() {
        // isize feeds as i64 LE for cross-platform determinism.
        assert_eq!(crc32_of(&42isize), crc32_of(&42i64));
        assert_eq!(crc32_of(&(-1isize)), crc32_of(&(-1i64)));
        assert_eq!(crc32_of(&0isize), crc32_of(&0i64));
    }
}
