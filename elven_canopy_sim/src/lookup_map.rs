// Non-iterable hash map for deterministic simulation code.
//
// `LookupMap<K, V>` wraps `HashMap` but exposes only point operations:
// `get`, `insert`, `remove`, `contains_key`, `len`. No `iter()`, `keys()`,
// `values()`, or `IntoIterator` — so there is no way to introduce
// iteration-order-dependent nondeterminism. For the rare cases where you
// need all keys or entries, `keys_sorted()` and `entries_sorted()` collect
// into a `Vec` and sort by key, guaranteeing deterministic order (requires
// `K: Ord + Clone`).
//
// Use this instead of `BTreeMap` when you only need point queries and want
// O(1) average-case performance instead of O(log n). Use `BTreeMap` when
// you genuinely need ordered iteration.
//
// **Serde:** Not derived. A naive `#[derive(Serialize, Deserialize)]` would
// work correctly but serialize in HashMap iteration order, so two identical
// maps could produce different bytes. If serde is needed and byte-identical
// output matters (e.g., for checksum comparison), write a custom impl that
// sorts keys before serializing.
//
// See also: `nav.rs` (spatial index), `types.rs` (VoxelCoord keys).

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

/// A hash map that only supports point operations (get, insert, remove,
/// contains_key). Iteration is intentionally unavailable, preventing
/// nondeterministic iteration-order bugs in deterministic simulation code.
///
/// Thin wrapper around `HashMap` — same O(1) average-case performance for
/// lookups and mutations, but without the footgun.
///
/// `Debug` intentionally prints only the length, not the contents, because
/// `HashMap` iteration order is nondeterministic.
#[derive(Clone)]
pub struct LookupMap<K, V> {
    inner: HashMap<K, V>,
}

impl<K, V> fmt::Debug for LookupMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LookupMap")
            .field("len", &self.inner.len())
            .finish()
    }
}

impl<K, V> LookupMap<K, V>
where
    K: Eq + Hash,
{
    /// Create an empty `LookupMap`.
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Create an empty `LookupMap` with at least the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: HashMap::with_capacity(capacity),
        }
    }

    /// Reserve capacity for at least `additional` more entries.
    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    /// Returns a reference to the value for `key`, or `None`.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.inner.get(key)
    }

    /// Returns a mutable reference to the value for `key`, or `None`.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.inner.get_mut(key)
    }

    /// Insert a key-value pair. Returns the previous value if the key was
    /// already present.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    /// Remove a key. Returns the value if it was present.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.inner.remove(key)
    }

    /// Returns `true` if the map contains the key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

// ---------------------------------------------------------------------------
// Sorted bulk access (requires K: Ord + Clone)
// ---------------------------------------------------------------------------

impl<K, V> LookupMap<K, V>
where
    K: Eq + Hash + Ord + Clone,
{
    /// Return all keys, sorted. O(n log n).
    pub fn keys_sorted(&self) -> Vec<K> {
        let mut keys: Vec<K> = self.inner.keys().cloned().collect();
        keys.sort();
        keys
    }
}

impl<K, V> LookupMap<K, V>
where
    K: Eq + Hash + Ord + Clone,
    V: Clone,
{
    /// Return all (key, value) pairs, sorted by key. O(n log n).
    pub fn entries_sorted(&self) -> Vec<(K, V)> {
        let mut entries: Vec<(K, V)> = self
            .inner
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        entries
    }
}

impl<K, V> PartialEq for LookupMap<K, V>
where
    K: Eq + Hash,
    V: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<K, V> Eq for LookupMap<K, V>
where
    K: Eq + Hash,
    V: Eq,
{
}

impl<K: Eq + Hash, V> Default for LookupMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut map = LookupMap::new();
        assert_eq!(map.insert(1, "one"), None);
        assert_eq!(map.get(&1), Some(&"one"));
        assert_eq!(map.get(&2), None);
    }

    #[test]
    fn insert_overwrites() {
        let mut map = LookupMap::new();
        map.insert(1, "one");
        assert_eq!(map.insert(1, "uno"), Some("one"));
        assert_eq!(map.get(&1), Some(&"uno"));
    }

    #[test]
    fn get_mut() {
        let mut map = LookupMap::new();
        map.insert(1, 10);
        *map.get_mut(&1).unwrap() += 5;
        assert_eq!(map.get(&1), Some(&15));
    }

    #[test]
    fn remove() {
        let mut map = LookupMap::new();
        map.insert(1, "one");
        assert_eq!(map.remove(&1), Some("one"));
        assert_eq!(map.remove(&1), None);
        assert!(!map.contains_key(&1));
    }

    #[test]
    fn contains_key() {
        let mut map = LookupMap::new();
        assert!(!map.contains_key(&1));
        map.insert(1, "one");
        assert!(map.contains_key(&1));
    }

    #[test]
    fn len_and_is_empty() {
        let mut map: LookupMap<i32, &str> = LookupMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        map.insert(1, "one");
        map.insert(2, "two");
        assert_eq!(map.len(), 2);
        assert!(!map.is_empty());
    }

    #[test]
    fn clear() {
        let mut map = LookupMap::new();
        map.insert(1, "one");
        map.insert(2, "two");
        map.clear();
        assert!(map.is_empty());
        assert_eq!(map.get(&1), None);
    }

    #[test]
    fn with_capacity() {
        let map: LookupMap<i32, i32> = LookupMap::with_capacity(100);
        assert!(map.is_empty());
    }

    #[test]
    fn reserve_does_not_lose_data() {
        let mut map = LookupMap::new();
        map.insert(1, "one");
        map.insert(2, "two");
        map.reserve(1000);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&1), Some(&"one"));
        assert_eq!(map.get(&2), Some(&"two"));
    }

    #[test]
    fn default_is_empty() {
        let map: LookupMap<String, i32> = LookupMap::default();
        assert!(map.is_empty());
    }

    #[test]
    fn keys_sorted_returns_deterministic_order() {
        let mut map = LookupMap::new();
        map.insert(30, "thirty");
        map.insert(10, "ten");
        map.insert(20, "twenty");
        assert_eq!(map.keys_sorted(), vec![10, 20, 30]);
    }

    #[test]
    fn keys_sorted_empty() {
        let map: LookupMap<i32, i32> = LookupMap::new();
        assert!(map.keys_sorted().is_empty());
    }

    #[test]
    fn entries_sorted_returns_deterministic_order() {
        let mut map = LookupMap::new();
        map.insert(3, "c");
        map.insert(1, "a");
        map.insert(2, "b");
        assert_eq!(map.entries_sorted(), vec![(1, "a"), (2, "b"), (3, "c")]);
    }

    #[test]
    fn entries_sorted_empty() {
        let map: LookupMap<i32, i32> = LookupMap::new();
        assert!(map.entries_sorted().is_empty());
    }

    #[test]
    fn keys_sorted_is_deterministic_across_calls() {
        let mut map = LookupMap::new();
        for i in (0..100).rev() {
            map.insert(i, i * 10);
        }
        let first = map.keys_sorted();
        let second = map.keys_sorted();
        assert_eq!(first, second);
        assert_eq!(first, (0..100).collect::<Vec<_>>());
    }
}
