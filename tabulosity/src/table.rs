//! Shared traits and helpers used by tabulosity-generated code.
//!
//! - `Bounded` — provides `MIN`/`MAX` constants for primary key types and
//!   indexed field types that support range queries.
//! - `FkCheck` — uniform FK validation for both `T` and `Option<T>` fields.
//! - `TableMeta` — associates a table companion struct with its key and row types.
//! - `map_start_bound` / `map_end_bound` — translate `RangeBounds<Field>`
//!   into composite `(Field, PK)` bounds for secondary index range scans.

use std::ops::Bound;

/// Types with known minimum and maximum values.
///
/// Used by generated secondary index code to construct composite range bounds
/// for equality and range queries. Primary key types must always implement
/// this trait. Indexed field types need it only if range queries are desired.
pub trait Bounded {
    const MIN: Self;
    const MAX: Self;
}

// --- Blanket impls for primitive types ---

macro_rules! impl_bounded {
    ($($ty:ty),*) => {
        $(
            impl Bounded for $ty {
                const MIN: Self = <$ty>::MIN;
                const MAX: Self = <$ty>::MAX;
            }
        )*
    };
}

impl_bounded!(u8, u16, u32, u64, i8, i16, i32, i64);

impl Bounded for bool {
    const MIN: Self = false;
    const MAX: Self = true;
}

impl<T: Bounded> Bounded for Option<T> {
    const MIN: Self = None;
    const MAX: Self = Some(T::MAX);
}

/// Helper trait for FK validation. Allows the Database derive to generate
/// the same code regardless of whether an FK field is `T` or `Option<T>`.
///
/// For bare `T`, `check_fk` calls the closure with `self`.
/// For `Option<T>`, `check_fk` returns `true` for `None` (no FK to validate)
/// and calls the closure with the inner value for `Some`.
pub trait FkCheck<K> {
    fn check_fk<F: Fn(&K) -> bool>(&self, f: F) -> bool;
}

impl<K> FkCheck<K> for K {
    fn check_fk<F: Fn(&K) -> bool>(&self, f: F) -> bool {
        f(self)
    }
}

impl<K> FkCheck<K> for Option<K> {
    fn check_fk<F: Fn(&K) -> bool>(&self, f: F) -> bool {
        match self {
            None => true,
            Some(k) => f(k),
        }
    }
}

/// Associates a generated table companion struct with its primary key and
/// row types. Implemented automatically by `#[derive(Table)]`.
///
/// Used by `#[derive(Database)]` to resolve the PK type in generated
/// `remove_*` method signatures.
#[doc(hidden)]
pub trait TableMeta {
    type Key;
    type Row;
}

// --- Range bound helpers (used by generated range query methods) ---

/// Translates the start bound of a `RangeBounds<Field>` into a composite
/// `Bound<(Field, PK)>` for secondary index range scans.
#[doc(hidden)]
pub fn map_start_bound<F: Clone, PK: Bounded>(bound: Bound<&F>) -> Bound<(F, PK)> {
    match bound {
        Bound::Included(v) => Bound::Included((v.clone(), PK::MIN)),
        Bound::Excluded(v) => Bound::Excluded((v.clone(), PK::MAX)),
        Bound::Unbounded => Bound::Unbounded,
    }
}

/// Translates the end bound of a `RangeBounds<Field>` into a composite
/// `Bound<(Field, PK)>` for secondary index range scans.
#[doc(hidden)]
pub fn map_end_bound<F: Clone, PK: Bounded>(bound: Bound<&F>) -> Bound<(F, PK)> {
    match bound {
        Bound::Included(v) => Bound::Included((v.clone(), PK::MAX)),
        Bound::Excluded(v) => Bound::Excluded((v.clone(), PK::MIN)),
        Bound::Unbounded => Bound::Unbounded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Bounded trait tests ---

    #[test]
    fn bounded_unsigned() {
        assert_eq!(u8::MIN, 0);
        assert_eq!(u8::MAX, 255);
        assert_eq!(u64::MIN, 0);
        assert_eq!(u64::MAX, u64::MAX);
    }

    #[test]
    fn bounded_signed() {
        assert_eq!(i8::MIN, -128);
        assert_eq!(i8::MAX, 127);
        assert_eq!(i64::MIN, i64::MIN);
        assert_eq!(i64::MAX, i64::MAX);
    }

    #[test]
    fn bounded_bool() {
        assert_eq!(<bool as Bounded>::MIN, false);
        assert_eq!(<bool as Bounded>::MAX, true);
    }

    #[test]
    fn bounded_option() {
        assert_eq!(<Option<u32> as Bounded>::MIN, None);
        assert_eq!(<Option<u32> as Bounded>::MAX, Some(u32::MAX));
    }

    #[test]
    fn bounded_option_ordering() {
        // None < Some(anything) — confirms MIN < MAX
        assert!(<Option<u8> as Bounded>::MIN < <Option<u8> as Bounded>::MAX);
    }

    // --- FkCheck trait tests ---

    #[test]
    fn fk_check_bare_true() {
        let val: u32 = 5;
        assert!(val.check_fk(|k| *k == 5));
    }

    #[test]
    fn fk_check_bare_false() {
        let val: u32 = 5;
        assert!(!val.check_fk(|k| *k == 10));
    }

    #[test]
    fn fk_check_option_none() {
        let val: Option<u32> = None;
        assert!(FkCheck::<u32>::check_fk(&val, |_k| false)); // None always passes
    }

    #[test]
    fn fk_check_option_some_true() {
        let val: Option<u32> = Some(5);
        assert!(FkCheck::<u32>::check_fk(&val, |k| *k == 5));
    }

    #[test]
    fn fk_check_option_some_false() {
        let val: Option<u32> = Some(5);
        assert!(!FkCheck::<u32>::check_fk(&val, |k| *k == 10));
    }

    // --- Range bound helper tests ---

    #[test]
    fn map_start_included() {
        let bound = map_start_bound::<u32, u8>(Bound::Included(&50));
        assert_eq!(bound, Bound::Included((50, u8::MIN)));
    }

    #[test]
    fn map_start_excluded() {
        let bound = map_start_bound::<u32, u8>(Bound::Excluded(&50));
        assert_eq!(bound, Bound::Excluded((50, u8::MAX)));
    }

    #[test]
    fn map_start_unbounded() {
        let bound = map_start_bound::<u32, u8>(Bound::Unbounded);
        assert_eq!(bound, Bound::Unbounded);
    }

    #[test]
    fn map_end_included() {
        let bound = map_end_bound::<u32, u8>(Bound::Included(&50));
        assert_eq!(bound, Bound::Included((50, u8::MAX)));
    }

    #[test]
    fn map_end_excluded() {
        let bound = map_end_bound::<u32, u8>(Bound::Excluded(&50));
        assert_eq!(bound, Bound::Excluded((50, u8::MIN)));
    }

    #[test]
    fn map_end_unbounded() {
        let bound = map_end_bound::<u32, u8>(Bound::Unbounded);
        assert_eq!(bound, Bound::Unbounded);
    }
}
