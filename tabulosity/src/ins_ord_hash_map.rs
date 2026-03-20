//! `InsOrdHashMap` — an insertion-ordered hash map with deterministic iteration.
//!
//! Provides O(1) lookup via an internal `HashMap<K, usize>` and deterministic
//! iteration via a `Vec<Entry<K, V>>` that preserves insertion order. Removed
//! entries become tombstones with a skip structure that allows iteration to
//! jump over contiguous dead spans in O(1). Periodic compaction reclaims
//! tombstone space while preserving insertion order.
//!
//! This module exists to give Tabulosity hash-based indexes that also support
//! deterministic iteration — critical for `elven_canopy_sim`'s lockstep
//! multiplayer and replay verification. The `HashMap` is internal and never
//! iterated; all iteration goes through the ordered vec.
//!
//! **Interior tombstone fields are garbage.** Only span-boundary tombstones
//! have valid `span_start` / `after_span` values. Interior tombstones exist
//! solely to drop the `(K, V)` that occupied that slot. This is intentional
//! and must not be "fixed."
//!
//! See also: `lib.rs` (crate root, re-exports `InsOrdHashMap`), `table.rs`
//! (query infrastructure), `error.rs` (write errors).

use std::collections::HashMap;

/// A single slot in the ordered entry vector.
///
/// `Live` holds an active key-value pair. `Tombstone` marks a removed entry;
/// only the first and last tombstones in a contiguous span have meaningful
/// field values (see module docs). Interior tombstone fields are garbage and
/// must never be read.
#[derive(Debug, Clone)]
enum Entry<K, V> {
    Live(K, V),
    Tombstone {
        /// Vec index of the first tombstone in this contiguous span.
        /// Valid only on the first and last tombstones of a span.
        span_start: usize,
        /// Vec index of the first live entry after this span (or `vec.len()`).
        /// Valid only on the first and last tombstones of a span.
        after_span: usize,
    },
}

/// A hash map that iterates in deterministic insertion order.
///
/// Lookup is O(1) via hashing. Iteration walks an internal vec in the order
/// entries were first inserted, skipping tombstoned (removed) entries via an
/// O(1) span-jump structure. Compaction reclaims tombstone space when the
/// dead-entry ratio exceeds ~50%.
///
/// # Key duplication
///
/// Keys are stored twice: once in the `HashMap` (for O(1) lookup) and once
/// in the vec (for ordered iteration without hash probes). This is a deliberate
/// trade-off — see the `F-tab-indexmap-fork` tracker item for a potential
/// zero-duplication alternative.
#[derive(Debug, Clone)]
pub struct InsOrdHashMap<K, V> {
    /// Key → vec index. Only Live entries have map entries.
    map: HashMap<K, usize>,
    /// Ordered entries. Iteration walks this, skipping tombstones.
    entries: Vec<Entry<K, V>>,
}

impl<K, V> InsOrdHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    /// Creates an empty `InsOrdHashMap`.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            entries: Vec::new(),
        }
    }

    /// Creates an empty `InsOrdHashMap` with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            entries: Vec::with_capacity(capacity),
        }
    }

    /// Returns the number of live entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the map contains no live entries.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns `true` if the map contains the given key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Returns a reference to the value for `key`, or `None`.
    pub fn get(&self, key: &K) -> Option<&V> {
        let &idx = self.map.get(key)?;
        match &self.entries[idx] {
            Entry::Live(_, v) => Some(v),
            Entry::Tombstone { .. } => {
                // HashMap says it's here but it's a tombstone — this is a bug.
                debug_assert!(
                    false,
                    "InsOrdHashMap: HashMap points to tombstone at index {idx}"
                );
                None
            }
        }
    }

    /// Returns a mutable reference to the value for `key`, or `None`.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let &idx = self.map.get(key)?;
        match &mut self.entries[idx] {
            Entry::Live(_, v) => Some(v),
            Entry::Tombstone { .. } => {
                debug_assert!(
                    false,
                    "InsOrdHashMap: HashMap points to tombstone at index {idx}"
                );
                None
            }
        }
    }

    /// Inserts a key-value pair. If the key already exists, updates the value
    /// in place (preserving iteration position) and returns the old value.
    /// New keys are appended to the end of the iteration order.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(&idx) = self.map.get(&key) {
            // Key exists — update value in place, preserving position.
            let entry = &mut self.entries[idx];
            match entry {
                Entry::Live(_, v) => {
                    let old = std::mem::replace(v, value);
                    Some(old)
                }
                Entry::Tombstone { .. } => {
                    debug_assert!(
                        false,
                        "InsOrdHashMap: HashMap points to tombstone at index {idx}"
                    );
                    None
                }
            }
        } else {
            // New key — append to vec.
            let idx = self.entries.len();
            self.entries.push(Entry::Live(key.clone(), value));
            self.map.insert(key, idx);
            None
        }
    }

    /// Removes the entry for `key` and returns its value, or `None` if absent.
    ///
    /// The vec slot becomes a tombstone. Adjacent tombstone spans are merged
    /// in O(1). Compaction is triggered if tombstones exceed ~50% of the vec.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let idx = self.map.remove(key)?;

        // Extract the live entry's value before tombstoning.
        let old_entry = std::mem::replace(
            &mut self.entries[idx],
            Entry::Tombstone {
                span_start: idx,
                after_span: idx + 1,
            },
        );
        let value = match old_entry {
            Entry::Live(_, v) => v,
            Entry::Tombstone { .. } => {
                debug_assert!(
                    false,
                    "InsOrdHashMap: removed entry was already a tombstone"
                );
                return None;
            }
        };

        // Merge with neighboring tombstone spans.
        self.merge_tombstone_spans(idx);

        // Compact if tombstones exceed ~50% of vec length.
        // Use "tombstones > live entries" as the threshold, which is equivalent
        // to tombstones > 50% of vec.len() (since tombstones + live = vec.len()).
        let tombstone_count = self.entries.len() - self.map.len();
        if tombstone_count > self.map.len() && !self.entries.is_empty() {
            self.compact();
        }

        Some(value)
    }

    /// Merges the tombstone at `idx` with any adjacent tombstone spans.
    ///
    /// After this call, the first tombstone in the merged span has a valid
    /// `after_span` and the last has a valid `span_start`. Interior tombstone
    /// fields are garbage.
    fn merge_tombstone_spans(&mut self, idx: usize) {
        // Find the leftmost tombstone of the span containing idx.
        let left = if idx > 0 {
            match &self.entries[idx - 1] {
                Entry::Tombstone { span_start, .. } => *span_start,
                Entry::Live(..) => idx,
            }
        } else {
            idx
        };

        // Find the rightmost tombstone of the span containing idx.
        let right_end = if idx + 1 < self.entries.len() {
            match &self.entries[idx + 1] {
                Entry::Tombstone { after_span, .. } => {
                    // The right neighbor's after_span points past that span.
                    // The last tombstone in that span is at after_span - 1.
                    *after_span
                }
                Entry::Live(..) => idx + 1,
            }
        } else {
            idx + 1
        };

        // Update span boundaries.
        // First tombstone in span: set after_span.
        if let Entry::Tombstone { after_span, .. } = &mut self.entries[left] {
            *after_span = right_end;
        }
        // Last tombstone in span: set span_start (skip if span is length 1,
        // since left already has both fields set correctly).
        let last = right_end - 1;
        if last != left
            && let Entry::Tombstone { span_start, .. } = &mut self.entries[last]
        {
            *span_start = left;
        }
    }

    /// Rebuilds the vec, removing all tombstones and updating HashMap indices.
    /// Preserves the original insertion order of surviving entries.
    fn compact(&mut self) {
        let mut new_entries = Vec::with_capacity(self.map.len());
        for entry in self.entries.drain(..) {
            if let Entry::Live(k, v) = entry {
                let new_idx = new_entries.len();
                self.map.insert(k.clone(), new_idx);
                new_entries.push(Entry::Live(k, v));
            }
        }
        self.entries = new_entries;
    }

    /// Forces compaction regardless of tombstone ratio.
    pub fn compact_now(&mut self) {
        self.compact();
    }

    /// Returns the number of tombstones (dead slots) in the internal vec.
    pub fn tombstone_count(&self) -> usize {
        self.entries.len() - self.map.len()
    }

    /// Returns an iterator over `(&K, &V)` pairs in insertion order.
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            entries: &self.entries,
            pos: 0,
        }
    }

    /// Returns an iterator over `(&K, &mut V)` pairs in insertion order.
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut {
            entries: self.entries.as_mut_slice(),
            pos: 0,
        }
    }

    /// Returns an iterator over keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(k, _)| k)
    }

    /// Returns an iterator over values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }

    /// Returns an iterator over mutable values in insertion order.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.iter_mut().map(|(_, v)| v)
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        self.map.clear();
        self.entries.clear();
    }
}

impl<K, V> Default for InsOrdHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

// --- Iterators ---

/// Immutable iterator over `(&K, &V)` pairs in insertion order.
pub struct Iter<'a, K, V> {
    entries: &'a [Entry<K, V>],
    pos: usize,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.pos >= self.entries.len() {
                return None;
            }
            match &self.entries[self.pos] {
                Entry::Live(k, v) => {
                    self.pos += 1;
                    return Some((k, v));
                }
                Entry::Tombstone { after_span, .. } => {
                    self.pos = *after_span;
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // We know there are at most entries.len() - pos items left,
        // but can't cheaply compute the exact count without walking.
        (0, Some(self.entries.len() - self.pos))
    }
}

/// Mutable iterator over `(&K, &mut V)` pairs in insertion order.
pub struct IterMut<'a, K, V> {
    entries: &'a mut [Entry<K, V>],
    pos: usize,
}

impl<'a, K, V> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.pos >= self.entries.len() {
                return None;
            }
            match &self.entries[self.pos] {
                Entry::Tombstone { after_span, .. } => {
                    self.pos = *after_span;
                }
                Entry::Live(..) => {
                    // Advance pos first, then borrow the entry.
                    let idx = self.pos;
                    self.pos += 1;
                    // SAFETY: We only hand out one mutable reference per index,
                    // and we advance past it before the next call. The reborrow
                    // through a raw pointer is needed because the borrow checker
                    // can't prove the non-overlapping access pattern.
                    let entry = unsafe { &mut *self.entries.as_mut_ptr().add(idx) };
                    match entry {
                        Entry::Live(k, v) => return Some((k, v)),
                        Entry::Tombstone { .. } => unreachable!(),
                    }
                }
            }
        }
    }
}

// --- FromIterator ---

impl<K, V> FromIterator<(K, V)> for InsOrdHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    /// Builds an `InsOrdHashMap` from an iterator of `(K, V)` pairs.
    ///
    /// Insertion order matches the iterator's yield order. Duplicate keys
    /// behave like repeated `insert()`: the later value wins, but the
    /// position of the first insertion is preserved.
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let (lo, _) = iter.size_hint();
        let mut map = Self::with_capacity(lo);
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

// --- IntoIterator ---

/// Owning iterator over `(K, V)` pairs in insertion order.
///
/// Unlike `Iter`/`IterMut`, this does not use the `after_span` skip structure
/// because `std::vec::IntoIter` doesn't support index-based jumps. Tombstones
/// are skipped one at a time, but each one is being dropped anyway, so the
/// per-element cost is unavoidable regardless.
pub struct IntoIter<K, V> {
    entries: std::vec::IntoIter<Entry<K, V>>,
}

impl<K, V> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.entries.next()? {
                Entry::Live(k, v) => return Some((k, v)),
                Entry::Tombstone { .. } => continue,
            }
        }
    }
}

impl<K, V> IntoIterator for InsOrdHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    type Item = (K, V);
    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            entries: self.entries.into_iter(),
        }
    }
}

impl<'a, K, V> IntoIterator for &'a InsOrdHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
{
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// --- Serde ---

#[cfg(feature = "serde")]
mod serde_impl {
    use super::*;
    use serde::de::{SeqAccess, Visitor};
    use serde::ser::SerializeSeq;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes as an ordered JSON array of `[key, value]` pairs, preserving
    /// insertion order. Tombstones are skipped.
    impl<K, V> Serialize for InsOrdHashMap<K, V>
    where
        K: Serialize + Eq + std::hash::Hash + Clone,
        V: Serialize,
    {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut seq = serializer.serialize_seq(Some(self.len()))?;
            for (k, v) in self.iter() {
                seq.serialize_element(&(k, v))?;
            }
            seq.end()
        }
    }

    /// Deserializes from an ordered array of `[key, value]` pairs, rebuilding
    /// the map in the serialized order.
    impl<'de, K, V> Deserialize<'de> for InsOrdHashMap<K, V>
    where
        K: Deserialize<'de> + Eq + std::hash::Hash + Clone,
        V: Deserialize<'de>,
    {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct InsOrdHashMapVisitor<K, V>(std::marker::PhantomData<(K, V)>);

            impl<'de, K, V> Visitor<'de> for InsOrdHashMapVisitor<K, V>
            where
                K: Deserialize<'de> + Eq + std::hash::Hash + Clone,
                V: Deserialize<'de>,
            {
                type Value = InsOrdHashMap<K, V>;

                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a sequence of [key, value] pairs")
                }

                fn visit_seq<A: SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> Result<InsOrdHashMap<K, V>, A::Error> {
                    let capacity = seq.size_hint().unwrap_or(0);
                    let mut map = InsOrdHashMap::with_capacity(capacity);
                    while let Some((k, v)) = seq.next_element::<(K, V)>()? {
                        map.insert(k, v);
                    }
                    Ok(map)
                }
            }

            deserializer.deserialize_seq(InsOrdHashMapVisitor(std::marker::PhantomData))
        }
    }
}

// --- PartialEq ---

impl<K, V> PartialEq for InsOrdHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: PartialEq,
{
    /// Two `InsOrdHashMap`s are equal if they have the same entries in the same
    /// insertion order. Tombstone layout doesn't matter — only live entries.
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        self.iter()
            .zip(other.iter())
            .all(|((k1, v1), (k2, v2))| k1 == k2 && v1 == v2)
    }
}

impl<K, V> Eq for InsOrdHashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Eq,
{
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Basic CRUD
    // -----------------------------------------------------------------------

    #[test]
    fn new_map_is_empty() {
        let map: InsOrdHashMap<String, i32> = InsOrdHashMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.tombstone_count(), 0);
    }

    #[test]
    fn insert_and_get() {
        let mut map = InsOrdHashMap::new();
        assert_eq!(map.insert("a", 1), None);
        assert_eq!(map.get(&"a"), Some(&1));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn insert_returns_old_value_on_update() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        assert_eq!(map.insert("a", 2), Some(1));
        assert_eq!(map.get(&"a"), Some(&2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let map: InsOrdHashMap<&str, i32> = InsOrdHashMap::new();
        assert_eq!(map.get(&"nope"), None);
    }

    #[test]
    fn contains_key() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        assert!(map.contains_key(&"a"));
        assert!(!map.contains_key(&"b"));
    }

    #[test]
    fn remove_existing() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        assert_eq!(map.remove(&"a"), Some(1));
        assert_eq!(map.len(), 0);
        assert!(!map.contains_key(&"a"));
        assert_eq!(map.get(&"a"), None);
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut map: InsOrdHashMap<&str, i32> = InsOrdHashMap::new();
        assert_eq!(map.remove(&"nope"), None);
    }

    #[test]
    fn get_mut_modifies_value() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        *map.get_mut(&"a").unwrap() = 42;
        assert_eq!(map.get(&"a"), Some(&42));
    }

    #[test]
    fn clear_empties_everything() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.clear();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.tombstone_count(), 0);
        assert_eq!(map.iter().count(), 0);
    }

    // -----------------------------------------------------------------------
    // Insertion-order iteration
    // -----------------------------------------------------------------------

    #[test]
    fn iter_preserves_insertion_order() {
        let mut map = InsOrdHashMap::new();
        map.insert("c", 3);
        map.insert("a", 1);
        map.insert("b", 2);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["c", "a", "b"]);
    }

    #[test]
    fn keys_values_match() {
        let mut map = InsOrdHashMap::new();
        map.insert("x", 10);
        map.insert("y", 20);
        let keys: Vec<_> = map.keys().copied().collect();
        let values: Vec<_> = map.values().copied().collect();
        assert_eq!(keys, vec!["x", "y"]);
        assert_eq!(values, vec![10, 20]);
    }

    #[test]
    fn iter_empty_map() {
        let map: InsOrdHashMap<&str, i32> = InsOrdHashMap::new();
        assert_eq!(map.iter().count(), 0);
    }

    #[test]
    fn into_iter_preserves_order() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        let pairs: Vec<_> = map.into_iter().collect();
        assert_eq!(pairs, vec![("a", 1), ("b", 2), ("c", 3)]);
    }

    // -----------------------------------------------------------------------
    // Update preserves position
    // -----------------------------------------------------------------------

    #[test]
    fn update_preserves_iteration_position() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        // Update "a" — it should stay in first position.
        map.insert("a", 100);
        let pairs: Vec<_> = map.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(pairs, vec![("a", 100), ("b", 2), ("c", 3)]);
    }

    // -----------------------------------------------------------------------
    // Remove + re-insert same key goes to end
    // -----------------------------------------------------------------------

    #[test]
    fn remove_and_reinsert_goes_to_end() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.remove(&"a");
        map.insert("a", 100);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["b", "c", "a"]);
    }

    // -----------------------------------------------------------------------
    // Tombstone creation and skip
    // -----------------------------------------------------------------------

    #[test]
    fn iteration_skips_single_tombstone() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        map.remove(&"c");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "b", "d", "e"]);
    }

    #[test]
    fn iteration_skips_multiple_scattered_tombstones() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i * 10);
        }
        map.remove(&2);
        map.remove(&5);
        map.remove(&8);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![0, 1, 3, 4, 6, 7, 9]);
    }

    #[test]
    fn remove_first_element() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.remove(&"a");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["b", "c"]);
    }

    #[test]
    fn remove_last_element() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.remove(&"c");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn remove_only_element() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.remove(&"a");
        assert!(map.is_empty());
        assert_eq!(map.iter().count(), 0);
    }

    // -----------------------------------------------------------------------
    // Tombstone span merging
    // -----------------------------------------------------------------------

    #[test]
    fn adjacent_removals_merge_spans_left_then_right() {
        // Remove B then C — B becomes tombstone, then C merges right into B's span.
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.remove(&"b");
        map.remove(&"c");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "d"]);
        // Verify the span is merged: first tombstone's after_span should jump past both.
        match &map.entries[1] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 1);
                assert_eq!(*after_span, 3);
            }
            _ => panic!("expected tombstone at index 1"),
        }
    }

    #[test]
    fn adjacent_removals_merge_spans_right_then_left() {
        // Remove C then B — C becomes tombstone, then B merges left into C's span.
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.remove(&"c");
        map.remove(&"b");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "d"]);
        // First tombstone at index 1.
        match &map.entries[1] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 1);
                assert_eq!(*after_span, 3);
            }
            _ => panic!("expected tombstone at index 1"),
        }
    }

    #[test]
    fn three_adjacent_removals_merge_into_one_span() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        map.insert("f", 6);
        // Remove B, C, D in order — should form one span [1..4).
        map.remove(&"b");
        map.remove(&"c");
        map.remove(&"d");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "e", "f"]);
        // First tombstone should span from 1 to 4.
        match &map.entries[1] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 1);
                assert_eq!(*after_span, 4);
            }
            _ => panic!("expected tombstone at index 1"),
        }
        // Last tombstone should also have span_start = 1.
        match &map.entries[3] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 1);
                assert_eq!(*after_span, 4);
            }
            _ => panic!("expected tombstone at index 3"),
        }
    }

    #[test]
    fn merge_both_sides_middle_removal_bridges_two_spans() {
        let mut map = InsOrdHashMap::new();
        // Use 8 entries so 3 removals (37.5%) won't trigger compaction (need >50%).
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3); // will be removed last, bridging B and D
        map.insert("d", 4);
        map.insert("e", 5);
        map.insert("f", 6);
        map.insert("g", 7);
        map.insert("h", 8);
        // Remove B and D first (separated by C).
        map.remove(&"b");
        map.remove(&"d");
        // Now remove C — it should merge the B span and D span into one.
        map.remove(&"c");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "e", "f", "g", "h"]);
        // Span should be [1..4).
        match &map.entries[1] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 1);
                assert_eq!(*after_span, 4);
            }
            _ => panic!("expected tombstone at index 1"),
        }
        match &map.entries[3] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 1);
                assert_eq!(*after_span, 4);
            }
            _ => panic!("expected tombstone at index 3"),
        }
    }

    #[test]
    fn remove_all_elements_creates_one_big_span() {
        let mut map = InsOrdHashMap::new();
        // Insert enough that compaction won't trigger mid-way through
        // (we'll check spans before compaction by using a specific removal order).
        // Actually, compaction will trigger. Let's just verify final state.
        for i in 0..4 {
            map.insert(i, i * 10);
        }
        map.remove(&0);
        map.remove(&1);
        map.remove(&2);
        map.remove(&3);
        assert!(map.is_empty());
        assert_eq!(map.iter().count(), 0);
    }

    // -----------------------------------------------------------------------
    // Compaction
    // -----------------------------------------------------------------------

    #[test]
    fn compaction_preserves_insertion_order() {
        let mut map = InsOrdHashMap::new();
        // Insert 10 items, remove 6 of them to trigger compaction.
        for i in 0..10 {
            map.insert(i, i * 10);
        }
        // Remove 0,1,2,3,4,5 — 6 tombstones, 4 live. Tombstones > live → compact.
        // Remove them one at a time; compaction triggers when ratio crosses threshold.
        map.remove(&0);
        map.remove(&1);
        map.remove(&2);
        map.remove(&3);
        map.remove(&4);
        map.remove(&5);
        // After compaction, should have 6,7,8,9 in order.
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![6, 7, 8, 9]);
        // And no tombstones.
        assert_eq!(map.tombstone_count(), 0);
    }

    #[test]
    fn compaction_preserves_interleaved_order() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i);
        }
        // Remove even numbers: 0, 2, 4, 6, 8.
        map.remove(&0);
        map.remove(&2);
        map.remove(&4);
        map.remove(&6);
        map.remove(&8);
        // 5 tombstones, 5 live. Next removal triggers compaction.
        // But we already crossed the threshold on the 5th removal
        // (5 tombstones > 5 live is false, equal not greater).
        // Let's remove one more to be sure.
        map.remove(&1);
        // Now: 6 tombstones, 4 live → compact.
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![3, 5, 7, 9]);
        assert_eq!(map.tombstone_count(), 0);
    }

    #[test]
    fn compact_now_forces_compaction() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.remove(&"b"); // 1 tombstone, 2 live — no auto-compact.
        assert_eq!(map.tombstone_count(), 1);
        map.compact_now();
        assert_eq!(map.tombstone_count(), 0);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "c"]);
    }

    #[test]
    fn lookup_works_after_compaction() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i * 100);
        }
        // Remove enough to trigger compaction.
        for i in 0..6 {
            map.remove(&i);
        }
        // Remaining entries should still be accessible.
        for i in 6..10 {
            assert_eq!(map.get(&i), Some(&(i * 100)));
        }
        // Removed entries should be gone.
        for i in 0..6 {
            assert_eq!(map.get(&i), None);
        }
    }

    #[test]
    fn insert_after_compaction_goes_to_end() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i);
        }
        for i in 0..6 {
            map.remove(&i);
        }
        // Compaction happened. Now insert new items.
        map.insert(100, 100);
        map.insert(101, 101);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![6, 7, 8, 9, 100, 101]);
    }

    // -----------------------------------------------------------------------
    // iter_mut
    // -----------------------------------------------------------------------

    #[test]
    fn iter_mut_modifies_values() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        for (_, v) in map.iter_mut() {
            *v *= 10;
        }
        let pairs: Vec<_> = map.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(pairs, vec![("a", 10), ("b", 20), ("c", 30)]);
    }

    #[test]
    fn iter_mut_skips_tombstones() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        map.remove(&"b");
        map.remove(&"d");
        let keys: Vec<_> = map.iter_mut().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["a", "c", "e"]);
    }

    #[test]
    fn values_mut_modifies_values() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        for v in map.values_mut() {
            *v += 1;
        }
        assert_eq!(map.get(&1), Some(&11));
        assert_eq!(map.get(&2), Some(&21));
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn with_capacity_works() {
        let map: InsOrdHashMap<i32, i32> = InsOrdHashMap::with_capacity(100);
        assert!(map.is_empty());
    }

    #[test]
    fn default_creates_empty() {
        let map: InsOrdHashMap<i32, i32> = InsOrdHashMap::default();
        assert!(map.is_empty());
    }

    #[test]
    fn remove_first_and_last_leaves_middle() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.remove(&"a");
        map.remove(&"c");
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["b"]);
    }

    #[test]
    fn alternating_insert_remove() {
        let mut map = InsOrdHashMap::new();
        for i in 0..20 {
            map.insert(i, i);
            if i > 0 && i % 2 == 0 {
                map.remove(&(i - 1));
            }
        }
        // Should have: 0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 19
        // (removed 1, 3, 5, 7, 9, 11, 13, 15, 17)
        for i in (1..18).step_by(2) {
            assert!(!map.contains_key(&i));
        }
        assert!(map.contains_key(&0));
        assert!(map.contains_key(&18));
        assert!(map.contains_key(&19));
    }

    // -----------------------------------------------------------------------
    // Tombstone span boundary correctness — detailed checks
    // -----------------------------------------------------------------------

    #[test]
    fn single_tombstone_has_self_referencing_span() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        map.remove(&"c"); // Index 2 becomes a lone tombstone.
        match &map.entries[2] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 2, "lone tombstone span_start should be self");
                assert_eq!(*after_span, 3, "lone tombstone after_span should be idx+1");
            }
            _ => panic!("expected tombstone at index 2"),
        }
    }

    #[test]
    fn span_at_vec_start() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        map.insert("f", 6);
        map.remove(&"a");
        map.remove(&"b");
        // Span [0..2), after_span = 2.
        match &map.entries[0] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 0);
                assert_eq!(*after_span, 2);
            }
            _ => panic!("expected tombstone at index 0"),
        }
        match &map.entries[1] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 0);
                assert_eq!(*after_span, 2);
            }
            _ => panic!("expected tombstone at index 1"),
        }
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["c", "d", "e", "f"]);
    }

    #[test]
    fn span_at_vec_end() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        map.insert("f", 6);
        map.remove(&"e");
        map.remove(&"f");
        // Span [4..6), after_span = 6 = vec.len().
        match &map.entries[4] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 4);
                assert_eq!(*after_span, 6);
            }
            _ => panic!("expected tombstone at index 4"),
        }
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["a", "b", "c", "d"]);
    }

    // -----------------------------------------------------------------------
    // Large-scale randomized consistency
    // -----------------------------------------------------------------------

    #[test]
    fn randomized_operations_stay_consistent() {
        // Deterministic "random" sequence using a simple LCG.
        let mut rng = 12345u64;
        let mut next = || -> u64 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            rng >> 33
        };

        let mut map = InsOrdHashMap::new();
        let mut reference: Vec<(u64, u64)> = Vec::new(); // ground truth

        for _ in 0..1000 {
            let op = next() % 3;
            match op {
                0 => {
                    // Insert
                    let k = next() % 100;
                    let v = next();
                    let existed = reference.iter().any(|(rk, _)| *rk == k);
                    if existed {
                        // Update existing — find and update in reference.
                        for entry in &mut reference {
                            if entry.0 == k {
                                entry.1 = v;
                                break;
                            }
                        }
                    } else {
                        reference.push((k, v));
                    }
                    map.insert(k, v);
                }
                1 => {
                    // Remove
                    let k = next() % 100;
                    let ref_idx = reference.iter().position(|(rk, _)| *rk == k);
                    if let Some(idx) = ref_idx {
                        reference.remove(idx);
                    }
                    map.remove(&k);
                }
                _ => {
                    // Get
                    let k = next() % 100;
                    let ref_val = reference.iter().find(|(rk, _)| *rk == k).map(|(_, v)| v);
                    assert_eq!(map.get(&k), ref_val);
                }
            }
        }

        // Final consistency check: iteration order matches reference.
        let map_pairs: Vec<_> = map.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(map_pairs, reference);
        assert_eq!(map.len(), reference.len());
    }

    #[test]
    fn heavy_removal_triggers_compaction_and_stays_consistent() {
        let mut map = InsOrdHashMap::new();
        // Insert 100 items.
        for i in 0u32..100 {
            map.insert(i, i * 10);
        }
        // Remove 70 items (enough to trigger compaction multiple times).
        for i in 0u32..70 {
            map.remove(&i);
        }
        // Remaining: 70..100 in order.
        let keys: Vec<_> = map.keys().copied().collect();
        let expected: Vec<u32> = (70..100).collect();
        assert_eq!(keys, expected);
        assert_eq!(map.len(), 30);
        // Compaction fires whenever tombstones > live count, so after many
        // removals the tombstone count should be well below live count.
        assert!(
            map.tombstone_count() <= map.len(),
            "tombstones {} should be <= live {}",
            map.tombstone_count(),
            map.len()
        );
    }

    #[test]
    fn repeated_insert_remove_same_key() {
        let mut map = InsOrdHashMap::new();
        map.insert("anchor", 0);
        for i in 0..50 {
            map.insert("key", i);
            map.remove(&"key");
        }
        // Only "anchor" should remain.
        assert_eq!(map.len(), 1);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["anchor"]);
    }

    // -----------------------------------------------------------------------
    // PartialEq
    // -----------------------------------------------------------------------

    #[test]
    fn equality_same_order() {
        let mut a = InsOrdHashMap::new();
        a.insert(1, "a");
        a.insert(2, "b");

        let mut b = InsOrdHashMap::new();
        b.insert(1, "a");
        b.insert(2, "b");

        assert_eq!(a, b);
    }

    #[test]
    fn equality_different_order() {
        let mut a = InsOrdHashMap::new();
        a.insert(1, "a");
        a.insert(2, "b");

        let mut b = InsOrdHashMap::new();
        b.insert(2, "b");
        b.insert(1, "a");

        assert_ne!(a, b);
    }

    #[test]
    fn equality_different_values() {
        let mut a = InsOrdHashMap::new();
        a.insert(1, "a");

        let mut b = InsOrdHashMap::new();
        b.insert(1, "b");

        assert_ne!(a, b);
    }

    #[test]
    fn equality_different_lengths() {
        let mut a = InsOrdHashMap::new();
        a.insert(1, "a");

        let mut b = InsOrdHashMap::new();
        b.insert(1, "a");
        b.insert(2, "b");

        assert_ne!(a, b);
    }

    #[test]
    fn equality_after_compaction() {
        let mut a = InsOrdHashMap::new();
        a.insert(1, "a");
        a.insert(2, "b");
        a.insert(3, "c");

        let mut b = InsOrdHashMap::new();
        b.insert(0, "x");
        b.insert(1, "a");
        b.insert(2, "b");
        b.insert(3, "c");
        b.remove(&0);
        b.compact_now();

        assert_eq!(a, b);
    }

    // -----------------------------------------------------------------------
    // Tombstone count
    // -----------------------------------------------------------------------

    #[test]
    fn tombstone_count_tracks_correctly() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        assert_eq!(map.tombstone_count(), 0);

        map.remove(&"b");
        assert_eq!(map.tombstone_count(), 1);

        map.remove(&"d");
        assert_eq!(map.tombstone_count(), 2);

        map.compact_now();
        assert_eq!(map.tombstone_count(), 0);
    }

    // -----------------------------------------------------------------------
    // Multiple spans with live entries between them
    // -----------------------------------------------------------------------

    #[test]
    fn two_separate_spans_both_skip_correctly() {
        let mut map = InsOrdHashMap::new();
        // a b c d e f g h
        for c in b'a'..=b'h' {
            map.insert(c as char, c as i32);
        }
        // Remove b,c and f,g — two separate spans.
        map.remove(&'b');
        map.remove(&'c');
        map.remove(&'f');
        map.remove(&'g');
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!['a', 'd', 'e', 'h']);
    }

    #[test]
    fn three_separate_spans() {
        let mut map = InsOrdHashMap::new();
        for i in 0..15 {
            map.insert(i, i);
        }
        // Remove 1-2, 6-8, 12-13 — three separate spans.
        map.remove(&1);
        map.remove(&2);
        map.remove(&6);
        map.remove(&7);
        map.remove(&8);
        map.remove(&12);
        map.remove(&13);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![0, 3, 4, 5, 9, 10, 11, 14]);
    }

    // -----------------------------------------------------------------------
    // Span merging order independence
    // -----------------------------------------------------------------------

    #[test]
    fn removal_order_bcd_produces_same_iteration_as_dcb() {
        let items = vec![("a", 1), ("b", 2), ("c", 3), ("d", 4), ("e", 5), ("f", 6)];

        let mut map1 = InsOrdHashMap::new();
        for &(k, v) in &items {
            map1.insert(k, v);
        }
        map1.remove(&"b");
        map1.remove(&"c");
        map1.remove(&"d");

        let mut map2 = InsOrdHashMap::new();
        for &(k, v) in &items {
            map2.insert(k, v);
        }
        map2.remove(&"d");
        map2.remove(&"c");
        map2.remove(&"b");

        let keys1: Vec<_> = map1.keys().copied().collect();
        let keys2: Vec<_> = map2.keys().copied().collect();
        assert_eq!(keys1, keys2);
        assert_eq!(keys1, vec!["a", "e", "f"]);
    }

    #[test]
    fn removal_order_outside_in_vs_inside_out() {
        let items: Vec<_> = (0..8).map(|i| (i, i * 10)).collect();

        // Remove 2,5 (outside) then 3,4 (inside).
        let mut map1 = InsOrdHashMap::new();
        for &(k, v) in &items {
            map1.insert(k, v);
        }
        map1.remove(&2);
        map1.remove(&5);
        map1.remove(&3);
        map1.remove(&4);

        // Remove 3,4 (inside) then 2,5 (outside).
        let mut map2 = InsOrdHashMap::new();
        for &(k, v) in &items {
            map2.insert(k, v);
        }
        map2.remove(&3);
        map2.remove(&4);
        map2.remove(&2);
        map2.remove(&5);

        let keys1: Vec<_> = map1.keys().copied().collect();
        let keys2: Vec<_> = map2.keys().copied().collect();
        assert_eq!(keys1, keys2);
        assert_eq!(keys1, vec![0, 1, 6, 7]);
    }

    // -----------------------------------------------------------------------
    // Stress: insert many, remove many, verify
    // -----------------------------------------------------------------------

    #[test]
    fn stress_insert_then_remove_half() {
        let n = 200;
        let mut map = InsOrdHashMap::new();
        for i in 0..n {
            map.insert(i, i);
        }
        // Remove all even numbers.
        for i in (0..n).step_by(2) {
            map.remove(&i);
        }
        let keys: Vec<_> = map.keys().copied().collect();
        let expected: Vec<i32> = (0..n).filter(|i| i % 2 != 0).collect();
        assert_eq!(keys, expected);

        // All remaining should be gettable.
        for &k in &expected {
            assert_eq!(map.get(&k), Some(&k));
        }
    }

    // -----------------------------------------------------------------------
    // "Get me 1 thing" pattern — iter().next() with leading tombstones
    // -----------------------------------------------------------------------

    #[test]
    fn iter_next_with_no_tombstones() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        assert_eq!(map.iter().next(), Some((&"a", &1)));
    }

    #[test]
    fn iter_next_with_leading_tombstone() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.remove(&"a");
        // First call to next() should skip the tombstone and return "b".
        assert_eq!(map.iter().next(), Some((&"b", &2)));
    }

    #[test]
    fn iter_next_with_many_leading_tombstones() {
        let mut map = InsOrdHashMap::new();
        // Insert 10, remove first 8 — iter().next() should jump the span.
        for i in 0..10 {
            map.insert(i, i * 10);
        }
        for i in 0..8 {
            map.remove(&i);
        }
        assert_eq!(map.iter().next(), Some((&8, &80)));
    }

    #[test]
    fn iter_next_all_tombstones() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.remove(&"a");
        // Empty after removal — but vec still has a tombstone (or was compacted).
        assert_eq!(map.iter().next(), None);
    }

    // -----------------------------------------------------------------------
    // Compaction during span merge: remove triggers compaction mid-merge
    // -----------------------------------------------------------------------

    #[test]
    fn compaction_during_heavy_span_building() {
        // Build a scenario where we create progressively larger spans
        // and compaction fires mid-process.
        let mut map = InsOrdHashMap::new();
        for i in 0..20 {
            map.insert(i, i);
        }
        // Remove 0..5 (span at front), then 10..15 (span in middle),
        // then 5..10 (bridges the two spans, triggers compaction).
        for i in 0..5 {
            map.remove(&i);
        }
        for i in 10..15 {
            map.remove(&i);
        }
        for i in 5..10 {
            map.remove(&i);
        }
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![15, 16, 17, 18, 19]);
        // All remaining entries are gettable.
        for &k in &keys {
            assert_eq!(map.get(&k), Some(&k));
        }
    }

    // -----------------------------------------------------------------------
    // Remove middle of an existing span (cannot happen — middle is already
    // tombstoned). But test that removing the entry just before or after
    // an existing span correctly extends it.
    // -----------------------------------------------------------------------

    #[test]
    fn extend_span_from_left() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i);
        }
        // Create span at [3..5).
        map.remove(&3);
        map.remove(&4);
        // Now remove 2 — should extend span to [2..5).
        map.remove(&2);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![0, 1, 5, 6, 7, 8, 9]);
        // Check span boundaries.
        match &map.entries[2] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 2);
                assert_eq!(*after_span, 5);
            }
            _ => panic!("expected tombstone at index 2"),
        }
        match &map.entries[4] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 2);
                assert_eq!(*after_span, 5);
            }
            _ => panic!("expected tombstone at index 4"),
        }
    }

    #[test]
    fn extend_span_from_right() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i);
        }
        // Create span at [3..5).
        map.remove(&3);
        map.remove(&4);
        // Now remove 5 — should extend span to [3..6).
        map.remove(&5);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![0, 1, 2, 6, 7, 8, 9]);
        match &map.entries[3] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 3);
                assert_eq!(*after_span, 6);
            }
            _ => panic!("expected tombstone at index 3"),
        }
        match &map.entries[5] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 3);
                assert_eq!(*after_span, 6);
            }
            _ => panic!("expected tombstone at index 5"),
        }
    }

    // -----------------------------------------------------------------------
    // Clone
    // -----------------------------------------------------------------------

    #[test]
    fn clone_produces_independent_copy() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, "a");
        map.insert(2, "b");
        map.insert(3, "c");
        map.remove(&2);

        let mut clone = map.clone();
        clone.insert(4, "d");
        clone.remove(&1);

        // Original unchanged.
        let orig_keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(orig_keys, vec![1, 3]);

        // Clone has different state.
        let clone_keys: Vec<_> = clone.keys().copied().collect();
        assert_eq!(clone_keys, vec![3, 4]);
    }

    // -----------------------------------------------------------------------
    // Multiple iterators (non-overlapping borrows)
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_immutable_iterators() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);

        let keys: Vec<_> = map.keys().copied().collect();
        let values: Vec<_> = map.values().copied().collect();
        assert_eq!(keys, vec![1, 2, 3]);
        assert_eq!(values, vec![10, 20, 30]);
    }

    // -----------------------------------------------------------------------
    // Regression: compaction then remove should still work
    // -----------------------------------------------------------------------

    #[test]
    fn remove_after_compaction() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i);
        }
        // Trigger compaction.
        for i in 0..6 {
            map.remove(&i);
        }
        // Now remove more from the compacted map.
        map.remove(&7);
        map.remove(&8);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![6, 9]);
        assert_eq!(map.get(&6), Some(&6));
        assert_eq!(map.get(&9), Some(&9));
    }

    // -----------------------------------------------------------------------
    // Ensure all 6 removal orderings of BCD produce same result
    // -----------------------------------------------------------------------

    #[test]
    fn all_removal_permutations_of_three_adjacent() {
        let permutations: &[&[&str]] = &[
            &["b", "c", "d"],
            &["b", "d", "c"],
            &["c", "b", "d"],
            &["c", "d", "b"],
            &["d", "b", "c"],
            &["d", "c", "b"],
        ];

        for perm in permutations {
            let mut map = InsOrdHashMap::new();
            // Use enough entries to avoid compaction (3 out of 8 = 37.5%).
            for &k in &["a", "b", "c", "d", "e", "f", "g", "h"] {
                map.insert(k, 0);
            }
            for &k in *perm {
                map.remove(&k);
            }
            let keys: Vec<_> = map.keys().copied().collect();
            assert_eq!(
                keys,
                vec!["a", "e", "f", "g", "h"],
                "failed for removal order {:?}",
                perm
            );
        }
    }

    // -----------------------------------------------------------------------
    // Large randomized test with verification at each step
    // -----------------------------------------------------------------------

    #[test]
    fn randomized_step_by_step_verification() {
        let mut rng = 99u64;
        let mut next = || -> u64 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            rng >> 33
        };

        let mut map = InsOrdHashMap::new();
        let mut reference: Vec<(u64, u64)> = Vec::new();

        for step in 0..500 {
            let op = next() % 3;
            match op {
                0 => {
                    let k = next() % 50;
                    let v = next();
                    if let Some(entry) = reference.iter_mut().find(|(rk, _)| *rk == k) {
                        entry.1 = v;
                    } else {
                        reference.push((k, v));
                    }
                    map.insert(k, v);
                }
                1 => {
                    let k = next() % 50;
                    if let Some(idx) = reference.iter().position(|(rk, _)| *rk == k) {
                        reference.remove(idx);
                    }
                    map.remove(&k);
                }
                _ => {
                    // Verify full state.
                    let map_pairs: Vec<_> = map.iter().map(|(&k, &v)| (k, v)).collect();
                    assert_eq!(map_pairs, reference, "mismatch at step {step}, op=verify");
                }
            }

            // Always verify len matches.
            assert_eq!(map.len(), reference.len(), "len mismatch at step {step}");
        }
    }

    // -----------------------------------------------------------------------
    // Extend long span by removing adjacent live entry
    // -----------------------------------------------------------------------

    #[test]
    fn extend_long_span_left() {
        // Span of 5 tombstones, then remove the entry just before the span.
        let mut map = InsOrdHashMap::new();
        for i in 0..20 {
            map.insert(i, i);
        }
        // Create span [5..10).
        for i in 5..10 {
            map.remove(&i);
        }
        // Remove 4 — should extend span to [4..10).
        map.remove(&4);
        let keys: Vec<_> = map.keys().copied().collect();
        let expected: Vec<i32> = (0..4).chain(10..20).collect();
        assert_eq!(keys, expected);
        // Check first tombstone.
        match &map.entries[4] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 4);
                assert_eq!(*after_span, 10);
            }
            _ => panic!("expected tombstone at index 4"),
        }
    }

    #[test]
    fn extend_long_span_right() {
        let mut map = InsOrdHashMap::new();
        for i in 0..20 {
            map.insert(i, i);
        }
        // Create span [5..10).
        for i in 5..10 {
            map.remove(&i);
        }
        // Remove 10 — should extend span to [5..11).
        map.remove(&10);
        let keys: Vec<_> = map.keys().copied().collect();
        let expected: Vec<i32> = (0..5).chain(11..20).collect();
        assert_eq!(keys, expected);
        match &map.entries[5] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 5);
                assert_eq!(*after_span, 11);
            }
            _ => panic!("expected tombstone at index 5"),
        }
        match &map.entries[10] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 5);
                assert_eq!(*after_span, 11);
            }
            _ => panic!("expected tombstone at index 10"),
        }
    }

    // -----------------------------------------------------------------------
    // Bridge two long spans
    // -----------------------------------------------------------------------

    #[test]
    fn bridge_two_long_spans() {
        let mut map = InsOrdHashMap::new();
        for i in 0..30 {
            map.insert(i, i);
        }
        // Span A: [2..6), Span B: [8..12). Live entry at 6 and 7.
        for i in 2..6 {
            map.remove(&i);
        }
        for i in 8..12 {
            map.remove(&i);
        }
        // Remove 6 and 7 to bridge A and B into one span [2..12).
        map.remove(&6);
        map.remove(&7);
        let keys: Vec<_> = map.keys().copied().collect();
        let expected: Vec<i32> = (0..2).chain(12..30).collect();
        assert_eq!(keys, expected);
        // Check span boundaries.
        match &map.entries[2] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 2);
                assert_eq!(*after_span, 12);
            }
            _ => panic!("expected tombstone at index 2"),
        }
        match &map.entries[11] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 2);
                assert_eq!(*after_span, 12);
            }
            _ => panic!("expected tombstone at index 11"),
        }
    }

    // -----------------------------------------------------------------------
    // Verify compaction count: multiple compactions during sequential removal
    // -----------------------------------------------------------------------

    #[test]
    fn multiple_compactions_during_sequential_removal() {
        let mut map = InsOrdHashMap::new();
        for i in 0..100 {
            map.insert(i, i);
        }
        // Remove items one at a time. After each removal, the invariant
        // should hold: tombstones <= live entries (compaction fires otherwise).
        for i in 0..90 {
            map.remove(&i);
            assert!(
                map.tombstone_count() <= map.len() || map.is_empty(),
                "invariant violated at removal {i}: {} tombstones, {} live",
                map.tombstone_count(),
                map.len()
            );
        }
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, (90..100).collect::<Vec<_>>());
    }

    // -----------------------------------------------------------------------
    // get_mut doesn't invalidate iteration
    // -----------------------------------------------------------------------

    #[test]
    fn get_mut_then_iter() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);
        *map.get_mut(&2).unwrap() = 200;
        let pairs: Vec<_> = map.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(pairs, vec![(1, 10), (2, 200), (3, 30)]);
    }

    // -----------------------------------------------------------------------
    // Capacity hint doesn't affect behavior
    // -----------------------------------------------------------------------

    #[test]
    fn with_capacity_same_behavior_as_new() {
        let mut a = InsOrdHashMap::new();
        let mut b = InsOrdHashMap::with_capacity(100);
        for i in 0..10 {
            a.insert(i, i);
            b.insert(i, i);
        }
        for i in 0..5 {
            a.remove(&i);
            b.remove(&i);
        }
        assert_eq!(a, b);
    }

    // -----------------------------------------------------------------------
    // Once-over: missing coverage identified by review
    // -----------------------------------------------------------------------

    #[test]
    fn into_iter_skips_tombstones() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.insert("c", 3);
        map.insert("d", 4);
        map.insert("e", 5);
        map.remove(&"b");
        map.remove(&"d");
        let pairs: Vec<_> = map.into_iter().collect();
        assert_eq!(pairs, vec![("a", 1), ("c", 3), ("e", 5)]);
    }

    #[test]
    fn get_mut_nonexistent_returns_none() {
        let mut map: InsOrdHashMap<&str, i32> = InsOrdHashMap::new();
        assert_eq!(map.get_mut(&"nope"), None);
        map.insert("a", 1);
        assert_eq!(map.get_mut(&"b"), None);
    }

    #[test]
    fn iter_size_hint() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);

        let mut it = map.iter();
        let (lo, hi) = it.size_hint();
        assert_eq!(lo, 0);
        assert!(hi.unwrap() >= 3);

        it.next();
        let (lo2, hi2) = it.size_hint();
        assert_eq!(lo2, 0);
        assert!(hi2.unwrap() < hi.unwrap());

        // Exhaust.
        it.next();
        it.next();
        assert_eq!(it.next(), None);
        let (lo3, hi3) = it.size_hint();
        assert_eq!(lo3, 0);
        assert_eq!(hi3, Some(0));
    }

    #[test]
    fn iter_mut_empty_map() {
        let mut map: InsOrdHashMap<i32, i32> = InsOrdHashMap::new();
        assert_eq!(map.iter_mut().next(), None);
    }

    #[test]
    fn equality_both_empty() {
        let a: InsOrdHashMap<i32, i32> = InsOrdHashMap::new();
        let b: InsOrdHashMap<i32, i32> = InsOrdHashMap::new();
        assert_eq!(a, b);
    }

    #[test]
    fn compact_now_empty_map() {
        let mut map: InsOrdHashMap<i32, i32> = InsOrdHashMap::new();
        map.compact_now(); // should not panic
        assert!(map.is_empty());

        // Also test on a map that had entries then was fully cleared.
        map.insert(1, 1);
        map.remove(&1);
        map.compact_now();
        assert!(map.is_empty());
        assert_eq!(map.tombstone_count(), 0);
    }

    #[test]
    fn values_mut_skips_tombstones() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);
        map.insert(4, 40);
        map.insert(5, 50);
        map.remove(&2);
        map.remove(&4);
        for v in map.values_mut() {
            *v += 1;
        }
        assert_eq!(map.get(&1), Some(&11));
        assert_eq!(map.get(&3), Some(&31));
        assert_eq!(map.get(&5), Some(&51));
    }

    #[test]
    fn remove_all_then_reinsert() {
        let mut map = InsOrdHashMap::new();
        for i in 0..5 {
            map.insert(i, i * 10);
        }
        for i in 0..5 {
            map.remove(&i);
        }
        assert!(map.is_empty());
        assert_eq!(map.tombstone_count(), 0); // compaction should have fired

        // Reinsert — indices should start fresh.
        map.insert(100, 1000);
        map.insert(200, 2000);
        assert_eq!(map.len(), 2);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![100, 200]);
        assert_eq!(map.get(&100), Some(&1000));
    }

    #[test]
    fn zst_values() {
        let mut map: InsOrdHashMap<String, ()> = InsOrdHashMap::new();
        map.insert("a".into(), ());
        map.insert("b".into(), ());
        map.insert("c".into(), ());
        map.remove(&"b".into());
        let keys: Vec<_> = map.keys().cloned().collect();
        assert_eq!(keys, vec!["a", "c"]);
        map.compact_now();
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn remove_at_fifty_percent_does_not_compact() {
        // Compaction fires when tombstones > live (strictly greater).
        // At exactly 50%, no compaction.
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.remove(&"a"); // 1 tombstone, 1 live — equal, not greater.
        assert_eq!(map.tombstone_count(), 1, "should NOT compact at 50%");
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn iter_trailing_tombstone_span() {
        // Trailing tombstone span: after_span == entries.len().
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i);
        }
        // Remove last 3 entries.
        map.remove(&7);
        map.remove(&8);
        map.remove(&9);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![0, 1, 2, 3, 4, 5, 6]);
        // iter().next() from the end should terminate.
        let mut it = map.iter();
        let count = it.by_ref().count();
        assert_eq!(count, 7);
        assert_eq!(it.next(), None);
    }

    #[test]
    fn equality_different_tombstone_layouts() {
        // Same live entries, same order, but one has tombstones and one doesn't.
        let mut a = InsOrdHashMap::new();
        a.insert(1, "x");
        a.insert(2, "y");
        a.insert(3, "z");

        let mut b = InsOrdHashMap::new();
        b.insert(0, "removed");
        b.insert(1, "x");
        b.insert(2, "y");
        b.insert(3, "z");
        b.remove(&0); // tombstone at index 0, no compaction (1 < 3)
        assert!(b.tombstone_count() > 0);

        // They should still be equal since live entries match.
        assert_eq!(a, b);
    }

    #[test]
    fn rapid_insert_remove_same_key_vec_bounded() {
        // Verify that repeated insert/remove of the same key doesn't grow
        // the entries vec unboundedly — compaction should keep it small.
        let mut map = InsOrdHashMap::new();
        map.insert("anchor", 0);
        for i in 0..1000 {
            map.insert("ephemeral", i);
            map.remove(&"ephemeral");
        }
        assert_eq!(map.len(), 1);
        // entries vec should be small (anchor + at most 1 tombstone, or
        // just anchor after compaction).
        assert!(
            map.entries.len() <= 2,
            "entries vec should be small after compaction, got {}",
            map.entries.len()
        );
    }

    // -----------------------------------------------------------------------
    // Second once-over: remaining coverage gaps
    // -----------------------------------------------------------------------

    #[test]
    fn into_iter_empty_map() {
        let map: InsOrdHashMap<i32, i32> = InsOrdHashMap::new();
        let pairs: Vec<_> = map.into_iter().collect();
        assert!(pairs.is_empty());
    }

    #[test]
    fn into_iter_all_tombstones() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, 10);
        map.insert(2, 20);
        // Remove both — compaction fires, leaving an empty vec.
        map.remove(&1);
        map.remove(&2);
        let pairs: Vec<_> = map.into_iter().collect();
        assert!(pairs.is_empty());
    }

    #[test]
    fn clear_then_reuse() {
        let mut map = InsOrdHashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        map.clear();

        // Full lifecycle after clear.
        map.insert("x", 10);
        map.insert("y", 20);
        assert_eq!(map.get(&"x"), Some(&10));
        assert_eq!(map.len(), 2);
        map.remove(&"x");
        assert_eq!(map.len(), 1);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec!["y"]);
    }

    #[test]
    fn get_mut_after_compaction() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i * 100);
        }
        for i in 0..6 {
            map.remove(&i);
        }
        // Compaction remapped indices. Verify get_mut still works.
        *map.get_mut(&7).unwrap() = 999;
        assert_eq!(map.get(&7), Some(&999));
    }

    // -----------------------------------------------------------------------
    // FromIterator
    // -----------------------------------------------------------------------

    #[test]
    fn from_iter_preserves_order() {
        let pairs = vec![(3, "c"), (1, "a"), (2, "b")];
        let map: InsOrdHashMap<i32, &str> = pairs.into_iter().collect();
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![3, 1, 2]);
        assert_eq!(map.get(&1), Some(&"a"));
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn from_iter_empty() {
        let map: InsOrdHashMap<i32, i32> = std::iter::empty().collect();
        assert!(map.is_empty());
    }

    #[test]
    fn from_iter_duplicate_keys_last_wins() {
        let pairs = vec![(1, "first"), (2, "two"), (1, "second")];
        let map: InsOrdHashMap<i32, &str> = pairs.into_iter().collect();
        assert_eq!(map.len(), 2);
        // Key 1 keeps its original position but has updated value.
        assert_eq!(map.get(&1), Some(&"second"));
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![1, 2]);
    }

    #[test]
    fn merge_spans_at_idx_zero_with_right_neighbor() {
        // Remove index 1, then index 0. Merging at idx=0 takes the
        // left=idx path (idx > 0 is false), then merges right.
        let mut map = InsOrdHashMap::new();
        for i in 0..8 {
            map.insert(i, i);
        }
        map.remove(&1);
        map.remove(&0);
        // Span [0..2) should exist.
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![2, 3, 4, 5, 6, 7]);
        match &map.entries[0] {
            Entry::Tombstone {
                span_start,
                after_span,
            } => {
                assert_eq!(*span_start, 0);
                assert_eq!(*after_span, 2);
            }
            _ => panic!("expected tombstone at index 0"),
        }
    }
}
