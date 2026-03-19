//! Integration tests for `InsOrdHashMap` — insertion-ordered hash map.
//!
//! These tests complement the unit tests in `ins_ord_hash_map.rs` with
//! cross-module and serde scenarios.

use tabulosity::InsOrdHashMap;

// ---------------------------------------------------------------------------
// Basic integration: use via the public API only
// ---------------------------------------------------------------------------

#[test]
fn public_api_insert_get_remove_iter() {
    let mut map = InsOrdHashMap::new();
    map.insert("alpha", 1);
    map.insert("beta", 2);
    map.insert("gamma", 3);

    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&"alpha"), Some(&1));
    assert_eq!(map.get(&"beta"), Some(&2));
    assert_eq!(map.get(&"gamma"), Some(&3));

    map.remove(&"beta");
    assert_eq!(map.len(), 2);
    assert_eq!(map.get(&"beta"), None);

    let keys: Vec<_> = map.keys().copied().collect();
    assert_eq!(keys, vec!["alpha", "gamma"]);
}

#[test]
fn iteration_determinism_across_identical_builds() {
    // Two maps built identically must iterate in the same order.
    let build = || {
        let mut map = InsOrdHashMap::new();
        for i in 0..50 {
            map.insert(i, i * 7);
        }
        for i in (0..50).step_by(3) {
            map.remove(&i);
        }
        map
    };

    let a = build();
    let b = build();

    let pairs_a: Vec<_> = a.iter().map(|(&k, &v)| (k, v)).collect();
    let pairs_b: Vec<_> = b.iter().map(|(&k, &v)| (k, v)).collect();
    assert_eq!(pairs_a, pairs_b);
}

#[test]
fn for_loop_iteration() {
    let mut map = InsOrdHashMap::new();
    map.insert(1, "one");
    map.insert(2, "two");
    map.insert(3, "three");

    let mut collected = Vec::new();
    for (&k, &v) in &map {
        collected.push((k, v));
    }
    assert_eq!(collected, vec![(1, "one"), (2, "two"), (3, "three")]);
}

#[test]
fn into_iter_consumes_map() {
    let mut map = InsOrdHashMap::new();
    map.insert(String::from("x"), 10);
    map.insert(String::from("y"), 20);

    let pairs: Vec<_> = map.into_iter().collect();
    assert_eq!(
        pairs,
        vec![(String::from("x"), 10), (String::from("y"), 20)]
    );
}

// ---------------------------------------------------------------------------
// Compaction observable behavior
// ---------------------------------------------------------------------------

#[test]
fn compaction_is_transparent_to_api_user() {
    let mut map = InsOrdHashMap::new();
    for i in 0..20 {
        map.insert(i, i);
    }
    // Remove enough to trigger compaction (>50%).
    for i in 0..15 {
        map.remove(&i);
    }
    // The user shouldn't notice compaction happened — just that the
    // right entries remain in the right order.
    let keys: Vec<_> = map.keys().copied().collect();
    assert_eq!(keys, vec![15, 16, 17, 18, 19]);

    // Can still insert and remove normally.
    map.insert(100, 100);
    assert_eq!(map.get(&100), Some(&100));
    let keys: Vec<_> = map.keys().copied().collect();
    assert_eq!(keys, vec![15, 16, 17, 18, 19, 100]);
}

#[test]
fn compact_now_is_idempotent() {
    let mut map = InsOrdHashMap::new();
    map.insert(1, 1);
    map.insert(2, 2);
    map.compact_now();
    map.compact_now();
    map.compact_now();
    assert_eq!(map.len(), 2);
    assert_eq!(map.tombstone_count(), 0);
    let keys: Vec<_> = map.keys().copied().collect();
    assert_eq!(keys, vec![1, 2]);
}

// ---------------------------------------------------------------------------
// String keys (non-Copy, heap-allocated)
// ---------------------------------------------------------------------------

#[test]
fn string_keys_work() {
    let mut map = InsOrdHashMap::new();
    map.insert(String::from("hello"), 1);
    map.insert(String::from("world"), 2);
    map.insert(String::from("foo"), 3);

    assert_eq!(map.get(&String::from("hello")), Some(&1));
    map.remove(&String::from("world"));

    let keys: Vec<_> = map.keys().cloned().collect();
    assert_eq!(keys, vec!["hello", "foo"]);
}

// ---------------------------------------------------------------------------
// Large values (ensure no accidental copies)
// ---------------------------------------------------------------------------

#[test]
fn large_value_types() {
    #[derive(Debug, Clone, PartialEq)]
    struct BigVal {
        data: [u8; 256],
    }

    let mut map = InsOrdHashMap::new();
    let val = BigVal { data: [42; 256] };
    map.insert(1, val.clone());
    assert_eq!(map.get(&1), Some(&val));
    map.remove(&1);
    assert_eq!(map.get(&1), None);
}

// ---------------------------------------------------------------------------
// Edge: zero-capacity, single element
// ---------------------------------------------------------------------------

#[test]
fn zero_capacity() {
    let mut map: InsOrdHashMap<i32, i32> = InsOrdHashMap::with_capacity(0);
    map.insert(1, 1);
    assert_eq!(map.get(&1), Some(&1));
}

#[test]
fn single_element_all_operations() {
    let mut map = InsOrdHashMap::new();
    map.insert(42, "answer");
    assert_eq!(map.len(), 1);
    assert!(!map.is_empty());
    assert!(map.contains_key(&42));
    assert_eq!(map.get(&42), Some(&"answer"));

    *map.get_mut(&42).unwrap() = "updated";
    assert_eq!(map.get(&42), Some(&"updated"));

    let keys: Vec<_> = map.keys().copied().collect();
    assert_eq!(keys, vec![42]);

    assert_eq!(map.remove(&42), Some("updated"));
    assert!(map.is_empty());
    assert_eq!(map.iter().count(), 0);
}

// ---------------------------------------------------------------------------
// PartialEq
// ---------------------------------------------------------------------------

#[test]
fn equality_ignores_internal_tombstone_state() {
    // Build two maps with the same final contents but different removal histories.
    let mut a = InsOrdHashMap::new();
    a.insert(1, "a");
    a.insert(2, "b");
    a.insert(3, "c");

    let mut b = InsOrdHashMap::new();
    b.insert(0, "x"); // extra entry
    b.insert(1, "a");
    b.insert(2, "b");
    b.insert(3, "c");
    b.remove(&0);
    b.compact_now();

    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// Randomized stress with different operation patterns
// ---------------------------------------------------------------------------

#[test]
fn stress_interleaved_insert_remove_get() {
    let mut rng = 42u64;
    let mut next = || -> u64 {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        rng >> 33
    };

    let mut map = InsOrdHashMap::new();
    let mut reference: Vec<(u64, u64)> = Vec::new();

    for _ in 0..2000 {
        match next() % 4 {
            0 | 1 => {
                // Insert (biased toward inserts to grow the map).
                let k = next() % 200;
                let v = next();
                if let Some(entry) = reference.iter_mut().find(|(rk, _)| *rk == k) {
                    entry.1 = v;
                } else {
                    reference.push((k, v));
                }
                map.insert(k, v);
            }
            2 => {
                // Remove.
                let k = next() % 200;
                if let Some(idx) = reference.iter().position(|(rk, _)| *rk == k) {
                    reference.remove(idx);
                }
                map.remove(&k);
            }
            _ => {
                // Get.
                let k = next() % 200;
                let expected = reference.iter().find(|(rk, _)| *rk == k).map(|(_, v)| v);
                assert_eq!(map.get(&k), expected);
            }
        }
    }

    let map_pairs: Vec<_> = map.iter().map(|(&k, &v)| (k, v)).collect();
    assert_eq!(map_pairs, reference);
}

// ---------------------------------------------------------------------------
// Serde roundtrip (requires serde feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
mod serde_tests {
    use tabulosity::InsOrdHashMap;

    #[test]
    fn serde_roundtrip_basic() {
        let mut map = InsOrdHashMap::new();
        map.insert("alpha".to_string(), 1);
        map.insert("beta".to_string(), 2);
        map.insert("gamma".to_string(), 3);

        let json = serde_json::to_string(&map).unwrap();
        let deserialized: InsOrdHashMap<String, i32> = serde_json::from_str(&json).unwrap();

        assert_eq!(map, deserialized);
    }

    #[test]
    fn serde_roundtrip_preserves_insertion_order() {
        let mut map = InsOrdHashMap::new();
        map.insert(3, "c");
        map.insert(1, "a");
        map.insert(2, "b");

        let json = serde_json::to_string(&map).unwrap();
        let deserialized: InsOrdHashMap<i32, &str> = serde_json::from_str(&json).unwrap();

        let keys: Vec<_> = deserialized.keys().copied().collect();
        assert_eq!(
            keys,
            vec![3, 1, 2],
            "insertion order must survive roundtrip"
        );
    }

    #[test]
    fn serde_roundtrip_after_removals() {
        let mut map = InsOrdHashMap::new();
        for i in 0..10 {
            map.insert(i, i * 100);
        }
        map.remove(&3);
        map.remove(&7);

        let json = serde_json::to_string(&map).unwrap();
        let deserialized: InsOrdHashMap<i32, i32> = serde_json::from_str(&json).unwrap();

        // Deserialized map should have no tombstones (they're skipped during serialization).
        assert_eq!(deserialized.tombstone_count(), 0);
        assert_eq!(deserialized.len(), map.len());

        // Same iteration order.
        let orig: Vec<_> = map.iter().map(|(&k, &v)| (k, v)).collect();
        let deser: Vec<_> = deserialized.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(orig, deser);
    }

    #[test]
    fn serde_roundtrip_after_compaction() {
        let mut map = InsOrdHashMap::new();
        for i in 0..20 {
            map.insert(i, i);
        }
        for i in 0..15 {
            map.remove(&i);
        }
        // Compaction should have occurred. Roundtrip should still work.
        let json = serde_json::to_string(&map).unwrap();
        let deserialized: InsOrdHashMap<i32, i32> = serde_json::from_str(&json).unwrap();

        assert_eq!(map, deserialized);
    }

    #[test]
    fn serde_empty_map() {
        let map: InsOrdHashMap<String, i32> = InsOrdHashMap::new();
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(json, "[]");
        let deserialized: InsOrdHashMap<String, i32> = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_empty());
    }

    #[test]
    fn serde_serialized_format_is_array_of_pairs() {
        let mut map = InsOrdHashMap::new();
        map.insert(1, "one");
        map.insert(2, "two");

        let json = serde_json::to_string(&map).unwrap();
        // Should be [[1,"one"],[2,"two"]]
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], serde_json::json!([1, "one"]));
        assert_eq!(arr[1], serde_json::json!([2, "two"]));
    }

    #[test]
    fn serde_duplicate_keys_in_input_last_wins() {
        // If the serialized data has duplicate keys, insert semantics apply:
        // the last occurrence's value wins, and iteration order follows first insert.
        let json = r#"[[1, "first"], [2, "second"], [1, "updated"]]"#;
        let map: InsOrdHashMap<i32, String> = serde_json::from_str(json).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&1), Some(&"updated".to_string()));
        // Key 1 was first inserted, so it's first in iteration order.
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, vec![1, 2]);
    }
}
