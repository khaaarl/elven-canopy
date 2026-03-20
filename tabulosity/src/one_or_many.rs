//! `OneOrMany<V, Many>` — an enum optimizing the common case of single-entry
//! index groups in Tabulosity hash indexes.
//!
//! Non-unique hash indexes map each indexed field value to a set of primary
//! keys. When most field values map to a single row (near-unique indexes),
//! allocating a full `BTreeSet` or `InsOrdHashMap` per entry wastes memory.
//! `OneOrMany` stores a single PK inline and only promotes to a collection
//! when a second PK arrives.
//!
//! Two concrete instantiations are used by generated code:
//! - `OneOrMany<PK, BTreeSet<PK>>` — for tables with BTree primary storage
//! - `OneOrMany<PK, InsOrdHashMap<PK, ()>>` — for tables with hash primary storage
//!
//! See also: `ins_ord_hash_map.rs` (the hash map used as inner collection),
//! `lib.rs` (re-exports `OneOrMany` and `RemoveResult`).

use std::collections::BTreeSet;

use crate::InsOrdHashMap;

/// Result of removing a value from a `OneOrMany`.
///
/// The caller (generated index maintenance code) uses this to decide whether
/// to remove the entire entry from the outer `InsOrdHashMap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveResult {
    /// The value was found and removed; the `OneOrMany` is now empty.
    /// Caller should remove the entry from the outer map.
    Empty,
    /// The value was found and removed; the `OneOrMany` still has entries.
    Removed,
    /// The value was not found (no-op). Should not happen in correct
    /// index maintenance.
    NotFound,
}

/// An enum that holds either a single value or a collection of values.
///
/// Optimizes the common case where a non-unique hash index key maps to
/// exactly one primary key. `One(pk)` is just the PK inline (typically
/// 4-8 bytes), avoiding the heap allocation of a full collection.
///
/// Promotion: `One` → `Many` when a second distinct value is inserted.
/// Demotion: `Many` → `One` when removal leaves exactly one element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneOrMany<V, Many> {
    One(V),
    Many(Many),
}

// --- OneOrMany<V, BTreeSet<V>> ---

impl<V: Ord + Clone> OneOrMany<V, BTreeSet<V>> {
    /// Inserts a value. If `One(existing)` and `val != existing`, promotes
    /// to `Many`. If `val == existing`, no-op (idempotent).
    pub fn insert_btree(&mut self, val: V) {
        match self {
            OneOrMany::One(existing) => {
                if *existing != val {
                    let mut set = BTreeSet::new();
                    set.insert(existing.clone());
                    set.insert(val);
                    *self = OneOrMany::Many(set);
                }
            }
            OneOrMany::Many(set) => {
                set.insert(val);
            }
        }
    }

    /// Removes a value. Returns a `RemoveResult` indicating what happened.
    pub fn remove_btree(&mut self, val: &V) -> RemoveResult {
        match self {
            OneOrMany::One(existing) => {
                if existing == val {
                    RemoveResult::Empty
                } else {
                    RemoveResult::NotFound
                }
            }
            OneOrMany::Many(set) => {
                if !set.remove(val) {
                    return RemoveResult::NotFound;
                }
                if set.len() == 1 {
                    let remaining = set.iter().next().unwrap().clone();
                    *self = OneOrMany::One(remaining);
                }
                RemoveResult::Removed
            }
        }
    }

    /// Returns `true` if the value is present.
    pub fn contains_btree(&self, val: &V) -> bool {
        match self {
            OneOrMany::One(existing) => existing == val,
            OneOrMany::Many(set) => set.contains(val),
        }
    }
}

// --- OneOrMany<V, InsOrdHashMap<V, ()>> ---

impl<V: Eq + std::hash::Hash + Clone> OneOrMany<V, InsOrdHashMap<V, ()>> {
    /// Inserts a value. If `One(existing)` and `val != existing`, promotes
    /// to `Many`. If `val == existing`, no-op (idempotent).
    pub fn insert_hash(&mut self, val: V) {
        match self {
            OneOrMany::One(existing) => {
                if *existing != val {
                    let mut map = InsOrdHashMap::with_capacity(2);
                    map.insert(existing.clone(), ());
                    map.insert(val, ());
                    *self = OneOrMany::Many(map);
                }
            }
            OneOrMany::Many(map) => {
                map.insert(val, ());
            }
        }
    }

    /// Removes a value. Returns a `RemoveResult` indicating what happened.
    pub fn remove_hash(&mut self, val: &V) -> RemoveResult {
        match self {
            OneOrMany::One(existing) => {
                if existing == val {
                    RemoveResult::Empty
                } else {
                    RemoveResult::NotFound
                }
            }
            OneOrMany::Many(map) => {
                if map.remove(val).is_none() {
                    return RemoveResult::NotFound;
                }
                if map.len() == 1 {
                    let remaining = map.keys().next().unwrap().clone();
                    *self = OneOrMany::One(remaining);
                }
                RemoveResult::Removed
            }
        }
    }

    /// Returns `true` if the value is present.
    pub fn contains_hash(&self, val: &V) -> bool {
        match self {
            OneOrMany::One(existing) => existing == val,
            OneOrMany::Many(map) => map.contains_key(val),
        }
    }
}

// --- Shared methods ---

impl<V, Many> OneOrMany<V, Many> {
    /// Returns the number of values stored.
    /// `OneOrMany` is never empty (the `One` variant always has 1 entry),
    /// so `is_empty()` is not provided.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize
    where
        Many: HasLen,
    {
        match self {
            OneOrMany::One(_) => 1,
            OneOrMany::Many(many) => many.len(),
        }
    }
}

/// Trait for collections that expose `len()`. Implemented for `BTreeSet`
/// and `InsOrdHashMap` to support `OneOrMany::len()`.
#[allow(clippy::len_without_is_empty)]
pub trait HasLen {
    fn len(&self) -> usize;
}

impl<V: Ord> HasLen for BTreeSet<V> {
    fn len(&self) -> usize {
        BTreeSet::len(self)
    }
}

impl<K: Eq + std::hash::Hash + Clone, V> HasLen for InsOrdHashMap<K, V> {
    fn len(&self) -> usize {
        InsOrdHashMap::len(self)
    }
}

// --- Iteration ---

/// Iterator over values in a `OneOrMany`.
///
/// For `One`, yields the single value. For `Many(BTreeSet)`, yields in
/// BTree order. For `Many(InsOrdHashMap)`, yields in insertion order.
pub enum OneOrManyIter<'a, V, ManyIter> {
    One(std::iter::Once<&'a V>),
    Many(ManyIter),
}

impl<'a, V, ManyIter> Iterator for OneOrManyIter<'a, V, ManyIter>
where
    ManyIter: Iterator<Item = &'a V>,
{
    type Item = &'a V;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            OneOrManyIter::One(once) => once.next(),
            OneOrManyIter::Many(iter) => iter.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            OneOrManyIter::One(once) => once.size_hint(),
            OneOrManyIter::Many(iter) => iter.size_hint(),
        }
    }
}

impl<V: Ord> OneOrMany<V, BTreeSet<V>> {
    /// Iterates values in BTree order (for `Many`) or yields the single value.
    pub fn iter_btree(&self) -> OneOrManyIter<'_, V, std::collections::btree_set::Iter<'_, V>> {
        match self {
            OneOrMany::One(v) => OneOrManyIter::One(std::iter::once(v)),
            OneOrMany::Many(set) => OneOrManyIter::Many(set.iter()),
        }
    }
}

/// Iterator adapter that extracts keys from `InsOrdHashMap` iteration.
pub struct InsOrdHashMapKeyIter<'a, K, V> {
    inner: crate::ins_ord_hash_map::Iter<'a, K, V>,
}

impl<'a, K, V> Iterator for InsOrdHashMapKeyIter<'a, K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    type Item = &'a K;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, _)| k)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<V: Eq + std::hash::Hash + Clone> OneOrMany<V, InsOrdHashMap<V, ()>> {
    /// Iterates values in insertion order (for `Many`) or yields the single value.
    pub fn iter_hash(&self) -> OneOrManyIter<'_, V, InsOrdHashMapKeyIter<'_, V, ()>> {
        match self {
            OneOrMany::One(v) => OneOrManyIter::One(std::iter::once(v)),
            OneOrMany::Many(map) => OneOrManyIter::Many(InsOrdHashMapKeyIter { inner: map.iter() }),
        }
    }
}

// --- Serde ---

#[cfg(feature = "serde")]
mod serde_impl {
    use super::*;
    use serde::de::{SeqAccess, Visitor};
    use serde::ser::SerializeSeq;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // --- OneOrMany<V, BTreeSet<V>> ---

    impl<V> Serialize for OneOrMany<V, BTreeSet<V>>
    where
        V: Serialize + Ord,
    {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                OneOrMany::One(v) => {
                    let mut seq = serializer.serialize_seq(Some(1))?;
                    seq.serialize_element(v)?;
                    seq.end()
                }
                OneOrMany::Many(set) => {
                    let mut seq = serializer.serialize_seq(Some(set.len()))?;
                    for v in set {
                        seq.serialize_element(v)?;
                    }
                    seq.end()
                }
            }
        }
    }

    impl<'de, V> Deserialize<'de> for OneOrMany<V, BTreeSet<V>>
    where
        V: Deserialize<'de> + Ord + Clone,
    {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct OneOrManyVisitor<V>(std::marker::PhantomData<V>);

            impl<'de, V> Visitor<'de> for OneOrManyVisitor<V>
            where
                V: Deserialize<'de> + Ord + Clone,
            {
                type Value = OneOrMany<V, BTreeSet<V>>;

                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a non-empty sequence of values")
                }

                fn visit_seq<A: SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> Result<OneOrMany<V, BTreeSet<V>>, A::Error> {
                    let first: V = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::custom("OneOrMany cannot be empty"))?;

                    match seq.next_element::<V>()? {
                        None => Ok(OneOrMany::One(first)),
                        Some(second) => {
                            let mut set = BTreeSet::new();
                            set.insert(first);
                            set.insert(second);
                            while let Some(v) = seq.next_element::<V>()? {
                                set.insert(v);
                            }
                            Ok(OneOrMany::Many(set))
                        }
                    }
                }
            }

            deserializer.deserialize_seq(OneOrManyVisitor(std::marker::PhantomData))
        }
    }

    // --- OneOrMany<V, InsOrdHashMap<V, ()>> ---

    impl<V> Serialize for OneOrMany<V, InsOrdHashMap<V, ()>>
    where
        V: Serialize + Eq + std::hash::Hash + Clone,
    {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                OneOrMany::One(v) => {
                    let mut seq = serializer.serialize_seq(Some(1))?;
                    seq.serialize_element(v)?;
                    seq.end()
                }
                OneOrMany::Many(map) => {
                    let mut seq = serializer.serialize_seq(Some(map.len()))?;
                    for (k, _) in map.iter() {
                        seq.serialize_element(k)?;
                    }
                    seq.end()
                }
            }
        }
    }

    impl<'de, V> Deserialize<'de> for OneOrMany<V, InsOrdHashMap<V, ()>>
    where
        V: Deserialize<'de> + Eq + std::hash::Hash + Clone,
    {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct OneOrManyVisitor<V>(std::marker::PhantomData<V>);

            impl<'de, V> Visitor<'de> for OneOrManyVisitor<V>
            where
                V: Deserialize<'de> + Eq + std::hash::Hash + Clone,
            {
                type Value = OneOrMany<V, InsOrdHashMap<V, ()>>;

                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a non-empty sequence of values")
                }

                fn visit_seq<A: SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> Result<OneOrMany<V, InsOrdHashMap<V, ()>>, A::Error> {
                    let first: V = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::custom("OneOrMany cannot be empty"))?;

                    match seq.next_element::<V>()? {
                        None => Ok(OneOrMany::One(first)),
                        Some(second) => {
                            let mut map =
                                InsOrdHashMap::with_capacity(seq.size_hint().unwrap_or(0) + 2);
                            map.insert(first, ());
                            map.insert(second, ());
                            while let Some(v) = seq.next_element::<V>()? {
                                map.insert(v, ());
                            }
                            Ok(OneOrMany::Many(map))
                        }
                    }
                }
            }

            deserializer.deserialize_seq(OneOrManyVisitor(std::marker::PhantomData))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // BTreeSet variant
    // -----------------------------------------------------------------------

    #[test]
    fn btree_insert_single_remains_one() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        om.insert_btree(1); // same value, idempotent
        assert_eq!(om, OneOrMany::One(1));
    }

    #[test]
    fn btree_insert_different_promotes_to_many() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        om.insert_btree(2);
        match &om {
            OneOrMany::Many(set) => {
                assert!(set.contains(&1));
                assert!(set.contains(&2));
                assert_eq!(set.len(), 2);
            }
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn btree_insert_into_many() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        om.insert_btree(2);
        om.insert_btree(3);
        assert_eq!(om.len(), 3);
    }

    #[test]
    fn btree_remove_from_one_returns_empty() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(5);
        assert_eq!(om.remove_btree(&5), RemoveResult::Empty);
    }

    #[test]
    fn btree_remove_wrong_value_from_one_returns_not_found() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(5);
        assert_eq!(om.remove_btree(&99), RemoveResult::NotFound);
    }

    #[test]
    fn btree_remove_demotes_many_to_one() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        om.insert_btree(2);
        assert_eq!(om.remove_btree(&1), RemoveResult::Removed);
        assert_eq!(om, OneOrMany::One(2));
    }

    #[test]
    fn btree_remove_from_many_stays_many() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        om.insert_btree(2);
        om.insert_btree(3);
        assert_eq!(om.remove_btree(&2), RemoveResult::Removed);
        assert_eq!(om.len(), 2);
        match &om {
            OneOrMany::Many(set) => {
                assert!(set.contains(&1));
                assert!(set.contains(&3));
            }
            _ => panic!("expected Many with 2 elements"),
        }
    }

    #[test]
    fn btree_remove_not_found_in_many() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        om.insert_btree(2);
        assert_eq!(om.remove_btree(&99), RemoveResult::NotFound);
    }

    #[test]
    fn btree_contains() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        assert!(om.contains_btree(&1));
        assert!(!om.contains_btree(&2));
        om.insert_btree(2);
        assert!(om.contains_btree(&1));
        assert!(om.contains_btree(&2));
        assert!(!om.contains_btree(&3));
    }

    #[test]
    fn btree_len() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        assert_eq!(om.len(), 1);
        om.insert_btree(2);
        assert_eq!(om.len(), 2);
        om.insert_btree(3);
        assert_eq!(om.len(), 3);
    }

    #[test]
    fn btree_iter_one() {
        let om = OneOrMany::<i32, BTreeSet<i32>>::One(42);
        let vals: Vec<_> = om.iter_btree().copied().collect();
        assert_eq!(vals, vec![42]);
    }

    #[test]
    fn btree_iter_many() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(3);
        om.insert_btree(1);
        om.insert_btree(2);
        let vals: Vec<_> = om.iter_btree().copied().collect();
        // BTreeSet iterates in sorted order.
        assert_eq!(vals, vec![1, 2, 3]);
    }

    // -----------------------------------------------------------------------
    // InsOrdHashMap variant
    // -----------------------------------------------------------------------

    #[test]
    fn hash_insert_single_remains_one() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        om.insert_hash(1); // same value, idempotent
        assert_eq!(om, OneOrMany::One(1));
    }

    #[test]
    fn hash_insert_different_promotes_to_many() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        om.insert_hash(2);
        match &om {
            OneOrMany::Many(map) => {
                assert!(map.contains_key(&1));
                assert!(map.contains_key(&2));
                assert_eq!(map.len(), 2);
            }
            _ => panic!("expected Many"),
        }
    }

    #[test]
    fn hash_insert_into_many() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        om.insert_hash(2);
        om.insert_hash(3);
        assert_eq!(om.len(), 3);
    }

    #[test]
    fn hash_remove_from_one_returns_empty() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(5);
        assert_eq!(om.remove_hash(&5), RemoveResult::Empty);
    }

    #[test]
    fn hash_remove_wrong_value_from_one_returns_not_found() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(5);
        assert_eq!(om.remove_hash(&99), RemoveResult::NotFound);
    }

    #[test]
    fn hash_remove_demotes_many_to_one() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        om.insert_hash(2);
        assert_eq!(om.remove_hash(&1), RemoveResult::Removed);
        assert_eq!(om, OneOrMany::One(2));
    }

    #[test]
    fn hash_remove_from_many_stays_many() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        om.insert_hash(2);
        om.insert_hash(3);
        assert_eq!(om.remove_hash(&2), RemoveResult::Removed);
        assert_eq!(om.len(), 2);
    }

    #[test]
    fn hash_remove_not_found_in_many() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        om.insert_hash(2);
        assert_eq!(om.remove_hash(&99), RemoveResult::NotFound);
    }

    #[test]
    fn hash_contains() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        assert!(om.contains_hash(&1));
        assert!(!om.contains_hash(&2));
        om.insert_hash(2);
        assert!(om.contains_hash(&1));
        assert!(om.contains_hash(&2));
        assert!(!om.contains_hash(&3));
    }

    #[test]
    fn hash_iter_one() {
        let om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(42);
        let vals: Vec<_> = om.iter_hash().copied().collect();
        assert_eq!(vals, vec![42]);
    }

    #[test]
    fn hash_iter_many_preserves_insertion_order() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(3);
        om.insert_hash(1);
        om.insert_hash(2);
        let vals: Vec<_> = om.iter_hash().copied().collect();
        // InsOrdHashMap iterates in insertion order: 3 first (from One), then 1, then 2.
        assert_eq!(vals, vec![3, 1, 2]);
    }

    // -----------------------------------------------------------------------
    // Promotion/demotion cycle
    // -----------------------------------------------------------------------

    #[test]
    fn btree_full_cycle_one_many_one() {
        let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
        om.insert_btree(2);
        om.insert_btree(3);
        assert_eq!(om.len(), 3);
        assert_eq!(om.remove_btree(&1), RemoveResult::Removed);
        assert_eq!(om.remove_btree(&2), RemoveResult::Removed);
        assert_eq!(om, OneOrMany::One(3));
        assert_eq!(om.remove_btree(&3), RemoveResult::Empty);
    }

    #[test]
    fn hash_full_cycle_one_many_one() {
        let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(1);
        om.insert_hash(2);
        om.insert_hash(3);
        assert_eq!(om.len(), 3);
        assert_eq!(om.remove_hash(&1), RemoveResult::Removed);
        assert_eq!(om.remove_hash(&2), RemoveResult::Removed);
        assert_eq!(om, OneOrMany::One(3));
        assert_eq!(om.remove_hash(&3), RemoveResult::Empty);
    }

    // -----------------------------------------------------------------------
    // Serde roundtrip
    // -----------------------------------------------------------------------

    #[cfg(feature = "serde")]
    mod serde_tests {
        use super::*;

        #[test]
        fn btree_serde_one_roundtrip() {
            let om = OneOrMany::<i32, BTreeSet<i32>>::One(42);
            let json = serde_json::to_string(&om).unwrap();
            assert_eq!(json, "[42]");
            let restored: OneOrMany<i32, BTreeSet<i32>> = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, OneOrMany::One(42));
        }

        #[test]
        fn btree_serde_many_roundtrip() {
            let mut om = OneOrMany::<i32, BTreeSet<i32>>::One(1);
            om.insert_btree(2);
            om.insert_btree(3);
            let json = serde_json::to_string(&om).unwrap();
            let restored: OneOrMany<i32, BTreeSet<i32>> = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.len(), 3);
            assert!(restored.contains_btree(&1));
            assert!(restored.contains_btree(&2));
            assert!(restored.contains_btree(&3));
        }

        #[test]
        fn hash_serde_one_roundtrip() {
            let om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(42);
            let json = serde_json::to_string(&om).unwrap();
            assert_eq!(json, "[42]");
            let restored: OneOrMany<i32, InsOrdHashMap<i32, ()>> =
                serde_json::from_str(&json).unwrap();
            assert_eq!(restored, OneOrMany::One(42));
        }

        #[test]
        fn hash_serde_many_preserves_order() {
            let mut om = OneOrMany::<i32, InsOrdHashMap<i32, ()>>::One(3);
            om.insert_hash(1);
            om.insert_hash(2);
            let json = serde_json::to_string(&om).unwrap();
            assert_eq!(json, "[3,1,2]");
            let restored: OneOrMany<i32, InsOrdHashMap<i32, ()>> =
                serde_json::from_str(&json).unwrap();
            let vals: Vec<i32> = restored.iter_hash().copied().collect();
            assert_eq!(vals, vec![3, 1, 2]);
        }

        #[test]
        fn serde_empty_sequence_rejected() {
            let result: Result<OneOrMany<i32, BTreeSet<i32>>, _> = serde_json::from_str("[]");
            assert!(result.is_err());
        }
    }
}
