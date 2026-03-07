//! Shared traits and helpers used by tabulosity-generated code.
//!
//! - `Bounded` — provides `MIN`/`MAX` constants for primary key types and
//!   indexed field types (legacy trait, no longer required by generated code).
//! - `FkCheck` — uniform FK validation for both `T` and `Option<T>` fields.
//! - `TableMeta` — associates a table companion struct with its key and row types.
//! - `IntoQuery` / `QueryBound` / `MatchAll` — unified query API for compound
//!   and simple indexes.
//! - `QueryOrder` / `QueryOpts` — ordering (asc/desc) and offset for query methods.
//! - `in_bounds` — post-filter helper for range checks in generated query code.

use std::ops::{Bound, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};

/// Types with known minimum and maximum values.
///
/// Legacy trait retained for user convenience. No longer required by generated
/// index code (which uses tracked bounds instead).
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

impl_bounded!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

/// Types that support auto-incrementing primary keys.
///
/// Provides `first()` (the starting value) and `successor()` (the next value
/// after `self`). Used by generated table code when `#[primary_key(auto_increment)]`
/// is present.
///
/// For all integer primitives, `first()` returns 0 — including signed types.
/// This means signed types have fewer usable IDs before overflow than their
/// unsigned counterparts (e.g., `i8` overflows at 127, not 255).
pub trait AutoIncrementable: Clone + Ord {
    fn first() -> Self;
    fn successor(&self) -> Self;
}

macro_rules! impl_auto_incrementable {
    ($($ty:ty),*) => {
        $(
            impl AutoIncrementable for $ty {
                fn first() -> Self { 0 }
                fn successor(&self) -> Self {
                    self.checked_add(1).expect("AutoIncrementable overflow")
                }
            }
        )*
    };
}

impl_auto_incrementable!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

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

// =============================================================================
// Unified query API — IntoQuery, QueryBound, MatchAll
// =============================================================================

/// Unit struct that matches all values for a field in a compound query.
/// Named `MatchAll` (not `Any`) to avoid collision with `std::any::Any`.
pub struct MatchAll;

/// The resolved form of a query parameter for a single field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryBound<T> {
    /// Match exactly this value.
    Exact(T),
    /// Match values in this range.
    Range { start: Bound<T>, end: Bound<T> },
    /// Match all values (no constraint on this field).
    MatchAll,
}

/// Converts a query parameter into a `QueryBound` for use in compound index
/// queries. Implemented for references (exact match), standard range types,
/// `MatchAll`, and arbitrary `(Bound<T>, Bound<T>)` pairs.
pub trait IntoQuery<T> {
    fn into_query(self) -> QueryBound<T>;
}

// Exact match from a reference — the most common case.
impl<T: Clone> IntoQuery<T> for &T {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Exact(self.clone())
    }
}

// Range types -> QueryBound::Range with appropriate bounds.
impl<T: Clone> IntoQuery<T> for Range<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Included(self.start),
            end: Bound::Excluded(self.end),
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeInclusive<T> {
    fn into_query(self) -> QueryBound<T> {
        let (start, end) = self.into_inner();
        QueryBound::Range {
            start: Bound::Included(start),
            end: Bound::Included(end),
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeFrom<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Included(self.start),
            end: Bound::Unbounded,
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeTo<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Unbounded,
            end: Bound::Excluded(self.end),
        }
    }
}

impl<T: Clone> IntoQuery<T> for RangeToInclusive<T> {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: Bound::Unbounded,
            end: Bound::Included(self.end),
        }
    }
}

impl<T> IntoQuery<T> for RangeFull {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::MatchAll
    }
}

// The MatchAll unit struct — ergonomic shorthand for "no constraint".
impl<T> IntoQuery<T> for MatchAll {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::MatchAll
    }
}

// Arbitrary bound combinations — covers cases with no named Rust range type.
impl<T: Clone> IntoQuery<T> for (Bound<T>, Bound<T>) {
    fn into_query(self) -> QueryBound<T> {
        QueryBound::Range {
            start: self.0,
            end: self.1,
        }
    }
}

/// Returns true if `val` falls within the given bounds.
///
/// Used by generated query code for post-filtering fields that cannot be
/// served by the index range scan.
#[doc(hidden)]
pub fn in_bounds<T: Ord>(val: &T, start: &Bound<T>, end: &Bound<T>) -> bool {
    let start_ok = match start {
        Bound::Included(s) => val >= s,
        Bound::Excluded(s) => val > s,
        Bound::Unbounded => true,
    };
    let end_ok = match end {
        Bound::Included(e) => val <= e,
        Bound::Excluded(e) => val < e,
        Bound::Unbounded => true,
    };
    start_ok && end_ok
}

/// Ordering direction for index query results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryOrder {
    #[default]
    Asc,
    Desc,
}

/// Options controlling ordering and offset for index query methods.
///
/// All `by_*`, `iter_by_*`, and `count_by_*` methods accept a `QueryOpts`
/// parameter. `QueryOpts::ASC` preserves the default ascending BTreeSet
/// iteration order with no offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryOpts {
    pub order: QueryOrder,
    pub offset: usize,
}

impl Default for QueryOpts {
    fn default() -> Self {
        Self {
            order: QueryOrder::Asc,
            offset: 0,
        }
    }
}

impl QueryOpts {
    pub const ASC: Self = Self {
        order: QueryOrder::Asc,
        offset: 0,
    };
    pub const DESC: Self = Self {
        order: QueryOrder::Desc,
        offset: 0,
    };

    pub fn desc() -> Self {
        Self::DESC
    }

    pub fn offset(offset: usize) -> Self {
        Self {
            order: QueryOrder::Asc,
            offset,
        }
    }

    pub fn with_offset(self, offset: usize) -> Self {
        Self { offset, ..self }
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
    fn bounded_u128() {
        assert_eq!(u128::MIN, 0);
        assert_eq!(u128::MAX, u128::MAX);
        assert_eq!(i128::MIN, i128::MIN);
        assert_eq!(i128::MAX, i128::MAX);
    }

    #[test]
    fn bounded_usize_isize() {
        assert_eq!(usize::MIN, 0);
        assert_eq!(usize::MAX, usize::MAX);
        assert_eq!(isize::MIN, isize::MIN);
        assert_eq!(isize::MAX, isize::MAX);
    }

    #[test]
    fn bounded_option_ordering() {
        // None < Some(anything) — confirms MIN < MAX
        assert!(<Option<u8> as Bounded>::MIN < <Option<u8> as Bounded>::MAX);
    }

    // --- AutoIncrementable trait tests ---

    #[test]
    fn auto_incrementable_unsigned() {
        assert_eq!(u32::first(), 0);
        assert_eq!(u32::first().successor(), 1);
        assert_eq!(42u32.successor(), 43);
    }

    #[test]
    fn auto_incrementable_signed() {
        assert_eq!(i32::first(), 0);
        assert_eq!(i32::first().successor(), 1);
        assert_eq!((-5i32).successor(), -4);
    }

    #[test]
    fn auto_incrementable_all_primitives() {
        // Just verify they compile and produce sensible results.
        assert_eq!(u8::first(), 0u8);
        assert_eq!(u16::first(), 0u16);
        assert_eq!(u64::first(), 0u64);
        assert_eq!(u128::first(), 0u128);
        assert_eq!(usize::first(), 0usize);
        assert_eq!(i8::first(), 0i8);
        assert_eq!(i16::first(), 0i16);
        assert_eq!(i64::first(), 0i64);
        assert_eq!(i128::first(), 0i128);
        assert_eq!(isize::first(), 0isize);
    }

    #[test]
    #[should_panic(expected = "overflow")]
    fn auto_incrementable_overflow_panics() {
        u8::MAX.successor();
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

    // --- IntoQuery trait tests ---

    #[test]
    fn into_query_exact_from_ref() {
        assert_eq!((&42u32).into_query(), QueryBound::Exact(42u32));
    }

    #[test]
    fn into_query_range() {
        assert_eq!(
            (3u32..7u32).into_query(),
            QueryBound::Range {
                start: Bound::Included(3),
                end: Bound::Excluded(7),
            }
        );
    }

    #[test]
    fn into_query_range_inclusive() {
        assert_eq!(
            (3u32..=7u32).into_query(),
            QueryBound::Range {
                start: Bound::Included(3),
                end: Bound::Included(7),
            }
        );
    }

    #[test]
    fn into_query_range_from() {
        assert_eq!(
            (3u32..).into_query(),
            QueryBound::Range {
                start: Bound::Included(3),
                end: Bound::Unbounded,
            }
        );
    }

    #[test]
    fn into_query_range_to() {
        assert_eq!(
            (..7u32).into_query(),
            QueryBound::Range {
                start: Bound::Unbounded,
                end: Bound::Excluded(7),
            }
        );
    }

    #[test]
    fn into_query_range_to_inclusive() {
        assert_eq!(
            (..=7u32).into_query(),
            QueryBound::Range {
                start: Bound::Unbounded,
                end: Bound::Included(7),
            }
        );
    }

    #[test]
    fn into_query_range_full() {
        assert_eq!((..).into_query(), QueryBound::<u32>::MatchAll);
    }

    #[test]
    fn into_query_match_all() {
        assert_eq!(MatchAll.into_query(), QueryBound::<u32>::MatchAll);
    }

    #[test]
    fn into_query_arbitrary_bounds() {
        assert_eq!(
            (Bound::Excluded(3u32), Bound::Excluded(7u32)).into_query(),
            QueryBound::Range {
                start: Bound::Excluded(3),
                end: Bound::Excluded(7),
            }
        );
    }

    // --- in_bounds tests ---

    #[test]
    fn in_bounds_included_both() {
        assert!(in_bounds(&5, &Bound::Included(3), &Bound::Included(7)));
        assert!(in_bounds(&3, &Bound::Included(3), &Bound::Included(7)));
        assert!(in_bounds(&7, &Bound::Included(3), &Bound::Included(7)));
        assert!(!in_bounds(&2, &Bound::Included(3), &Bound::Included(7)));
        assert!(!in_bounds(&8, &Bound::Included(3), &Bound::Included(7)));
    }

    #[test]
    fn in_bounds_excluded_both() {
        assert!(in_bounds(&5, &Bound::Excluded(3), &Bound::Excluded(7)));
        assert!(!in_bounds(&3, &Bound::Excluded(3), &Bound::Excluded(7)));
        assert!(!in_bounds(&7, &Bound::Excluded(3), &Bound::Excluded(7)));
    }

    #[test]
    fn in_bounds_unbounded() {
        assert!(in_bounds(&5, &Bound::Unbounded, &Bound::Unbounded));
        assert!(in_bounds(&u32::MAX, &Bound::Unbounded, &Bound::Unbounded));
    }

    #[test]
    fn in_bounds_mixed() {
        assert!(in_bounds(&5, &Bound::Excluded(3), &Bound::Included(7)));
        assert!(!in_bounds(&3, &Bound::Excluded(3), &Bound::Included(7)));
        assert!(in_bounds(&7, &Bound::Excluded(3), &Bound::Included(7)));
    }
}
