//! Integration tests for compound (multi-column) primary keys.
//!
//! Tests the struct-level `#[primary_key("field1", "field2")]` attribute,
//! which creates a tuple key type `(Type1, Type2)` for the generated table.

use tabulosity::{Bounded, QueryOpts, Table};

// --- Row types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CreatureId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TraitKind(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind")]
struct CreatureTrait {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
}

// --- Helpers ---

fn make_trait(cid: u32, tk: u32, value: i32) -> CreatureTrait {
    CreatureTrait {
        creature_id: CreatureId(cid),
        trait_kind: TraitKind(tk),
        value,
    }
}

// --- Basic CRUD ---

#[test]
fn insert_and_get() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    let row = table.get(&(CreatureId(1), TraitKind(10))).unwrap();
    assert_eq!(row.value, 42);
}

#[test]
fn insert_and_get_ref() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    let row = table.get_ref(&(CreatureId(1), TraitKind(10))).unwrap();
    assert_eq!(row.value, 42);
}

#[test]
fn contains() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    assert!(table.contains(&(CreatureId(1), TraitKind(10))));
    assert!(!table.contains(&(CreatureId(1), TraitKind(20))));
    assert!(!table.contains(&(CreatureId(2), TraitKind(10))));
}

#[test]
fn duplicate_compound_key_fails() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    let err = table.insert_no_fk(make_trait(1, 10, 99)).unwrap_err();
    assert!(
        matches!(err, tabulosity::Error::DuplicateKey { .. }),
        "expected DuplicateKey, got: {err:?}"
    );
}

#[test]
fn different_compound_keys_succeed() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    table.insert_no_fk(make_trait(1, 20, 43)).unwrap(); // same creature, different trait
    table.insert_no_fk(make_trait(2, 10, 44)).unwrap(); // different creature, same trait
    assert_eq!(table.len(), 3);
}

#[test]
fn update() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    table.update_no_fk(make_trait(1, 10, 99)).unwrap();
    assert_eq!(
        table.get(&(CreatureId(1), TraitKind(10))).unwrap().value,
        99
    );
}

#[test]
fn update_nonexistent_fails() {
    let mut table = CreatureTraitTable::new();
    let err = table.update_no_fk(make_trait(1, 10, 42)).unwrap_err();
    assert!(matches!(err, tabulosity::Error::NotFound { .. }));
}

#[test]
fn upsert_insert_path() {
    let mut table = CreatureTraitTable::new();
    table.upsert_no_fk(make_trait(1, 10, 42)).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(
        table.get(&(CreatureId(1), TraitKind(10))).unwrap().value,
        42
    );
}

#[test]
fn upsert_update_path() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    table.upsert_no_fk(make_trait(1, 10, 99)).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(
        table.get(&(CreatureId(1), TraitKind(10))).unwrap().value,
        99
    );
}

#[test]
fn remove() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    let removed = table.remove_no_fk(&(CreatureId(1), TraitKind(10))).unwrap();
    assert_eq!(removed.value, 42);
    assert_eq!(table.len(), 0);
}

#[test]
fn remove_nonexistent_fails() {
    let mut table = CreatureTraitTable::new();
    let err = table
        .remove_no_fk(&(CreatureId(1), TraitKind(10)))
        .unwrap_err();
    assert!(matches!(err, tabulosity::Error::NotFound { .. }));
}

// --- Keys and iteration ---

#[test]
fn keys_returns_tuples() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(2, 20, 1)).unwrap();
    table.insert_no_fk(make_trait(1, 10, 2)).unwrap();
    table.insert_no_fk(make_trait(1, 20, 3)).unwrap();
    let keys = table.keys();
    assert_eq!(
        keys,
        vec![
            (CreatureId(1), TraitKind(10)),
            (CreatureId(1), TraitKind(20)),
            (CreatureId(2), TraitKind(20)),
        ]
    );
}

#[test]
fn iter_all_ordered_by_compound_key() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(2, 10, 1)).unwrap();
    table.insert_no_fk(make_trait(1, 20, 2)).unwrap();
    table.insert_no_fk(make_trait(1, 10, 3)).unwrap();
    let values: Vec<i32> = table.iter_all().map(|r| r.value).collect();
    assert_eq!(values, vec![3, 2, 1]); // (1,10), (1,20), (2,10)
}

// --- pk_val ---

#[test]
fn pk_val_returns_tuple() {
    let row = make_trait(1, 10, 42);
    assert_eq!(row.pk_val(), (CreatureId(1), TraitKind(10)));
}

// --- Secondary index on compound PK table ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind")]
struct IndexedTrait {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    #[indexed]
    pub value: i32,
}

#[test]
fn secondary_index_query() {
    let mut table = IndexedTraitTable::new();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(20),
            value: 99,
        })
        .unwrap();

    assert_eq!(table.count_by_value(&42, QueryOpts::ASC), 2);
    assert_eq!(table.count_by_value(&99, QueryOpts::ASC), 1);

    let rows = table.by_value(&42, QueryOpts::ASC);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].creature_id, CreatureId(1));
    assert_eq!(rows[1].creature_id, CreatureId(2));
}

#[test]
fn secondary_index_maintained_through_update() {
    let mut table = IndexedTraitTable::new();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    assert_eq!(table.count_by_value(&42, QueryOpts::ASC), 1);

    table
        .update_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 99,
        })
        .unwrap();
    assert_eq!(table.count_by_value(&42, QueryOpts::ASC), 0);
    assert_eq!(table.count_by_value(&99, QueryOpts::ASC), 1);
}

#[test]
fn secondary_index_maintained_through_remove() {
    let mut table = IndexedTraitTable::new();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table.remove_no_fk(&(CreatureId(1), TraitKind(10))).unwrap();
    assert_eq!(table.count_by_value(&42, QueryOpts::ASC), 0);
}

// --- modify_unchecked ---

#[test]
fn modify_unchecked_with_compound_key() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    table
        .modify_unchecked(&(CreatureId(1), TraitKind(10)), |row| {
            row.value = 99;
        })
        .unwrap();
    assert_eq!(
        table.get(&(CreatureId(1), TraitKind(10))).unwrap().value,
        99
    );
}

#[test]
fn modify_unchecked_nonexistent_fails() {
    let mut table = CreatureTraitTable::new();
    let err = table
        .modify_unchecked(&(CreatureId(1), TraitKind(10)), |row| {
            row.value = 99;
        })
        .unwrap_err();
    assert!(matches!(err, tabulosity::Error::NotFound { .. }));
}

#[test]
fn modify_unchecked_range_with_compound_key() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 1)).unwrap();
    table.insert_no_fk(make_trait(1, 20, 2)).unwrap();
    table.insert_no_fk(make_trait(2, 10, 3)).unwrap();

    let count = table.modify_unchecked_range(
        (CreatureId(1), TraitKind(10))..=(CreatureId(1), TraitKind(20)),
        |_pk, row| {
            row.value += 100;
        },
    );
    assert_eq!(count, 2);
    assert_eq!(
        table.get(&(CreatureId(1), TraitKind(10))).unwrap().value,
        101
    );
    assert_eq!(
        table.get(&(CreatureId(1), TraitKind(20))).unwrap().value,
        102
    );
    assert_eq!(table.get(&(CreatureId(2), TraitKind(10))).unwrap().value, 3); // unchanged
}

#[test]
fn modify_unchecked_all_with_compound_key() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 1)).unwrap();
    table.insert_no_fk(make_trait(2, 20, 2)).unwrap();
    let count = table.modify_unchecked_all(|_pk, row| {
        row.value += 10;
    });
    assert_eq!(count, 2);
    assert_eq!(
        table.get(&(CreatureId(1), TraitKind(10))).unwrap().value,
        11
    );
    assert_eq!(
        table.get(&(CreatureId(2), TraitKind(20))).unwrap().value,
        12
    );
}

// --- Debug assertions for modify_unchecked ---

#[test]
#[should_panic(expected = "primary key field")]
#[cfg(debug_assertions)]
fn modify_unchecked_panics_on_first_pk_field_change() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    let _ = table.modify_unchecked(&(CreatureId(1), TraitKind(10)), |row| {
        row.creature_id = CreatureId(99);
    });
}

#[test]
#[should_panic(expected = "primary key field")]
#[cfg(debug_assertions)]
fn modify_unchecked_panics_on_second_pk_field_change() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    let _ = table.modify_unchecked(&(CreatureId(1), TraitKind(10)), |row| {
        row.trait_kind = TraitKind(99);
    });
}

#[test]
#[should_panic(expected = "indexed field")]
#[cfg(debug_assertions)]
fn modify_unchecked_panics_on_indexed_field_change() {
    let mut table = IndexedTraitTable::new();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    let _ = table.modify_unchecked(&(CreatureId(1), TraitKind(10)), |row| {
        row.value = 99;
    });
}

#[test]
#[should_panic(expected = "primary key field")]
#[cfg(debug_assertions)]
fn modify_unchecked_all_panics_on_pk_change() {
    let mut table = CreatureTraitTable::new();
    table.insert_no_fk(make_trait(1, 10, 42)).unwrap();
    table.modify_unchecked_all(|_pk, row| {
        row.creature_id = CreatureId(99);
    });
}

// --- TableMeta ---

#[test]
fn table_meta_key_type() {
    use tabulosity::TableMeta;
    fn assert_key_type<T: TableMeta<Key = (CreatureId, TraitKind)>>() {}
    assert_key_type::<CreatureTraitTable>();
}

// --- Unique index on compound PK table ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind")]
struct UniqueNamedTrait {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    #[indexed(unique)]
    pub name: String,
}

#[test]
fn unique_index_insert_conflict_detected() {
    let mut table = UniqueNamedTraitTable::new();
    table
        .insert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            name: "Strength".to_string(),
        })
        .unwrap();
    let err = table
        .insert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            name: "Strength".to_string(), // same name, different PK
        })
        .unwrap_err();
    assert!(
        matches!(err, tabulosity::Error::DuplicateIndex { .. }),
        "expected DuplicateIndex, got: {err:?}"
    );
}

#[test]
fn unique_index_update_conflict_detected() {
    let mut table = UniqueNamedTraitTable::new();
    table
        .insert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            name: "Strength".to_string(),
        })
        .unwrap();
    table
        .insert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            name: "Speed".to_string(),
        })
        .unwrap();
    let err = table
        .update_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            name: "Strength".to_string(), // conflicts with row (1,10)
        })
        .unwrap_err();
    assert!(
        matches!(err, tabulosity::Error::DuplicateIndex { .. }),
        "expected DuplicateIndex, got: {err:?}"
    );
}

#[test]
fn unique_index_same_value_update_succeeds() {
    let mut table = UniqueNamedTraitTable::new();
    table
        .insert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            name: "Strength".to_string(),
        })
        .unwrap();
    // Update same row, same unique value — should succeed.
    table
        .update_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            name: "Strength".to_string(),
        })
        .unwrap();
}

#[test]
fn upsert_update_path_unique_conflict() {
    let mut table = UniqueNamedTraitTable::new();
    table
        .insert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            name: "Strength".to_string(),
        })
        .unwrap();
    table
        .insert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            name: "Speed".to_string(),
        })
        .unwrap();
    // Upsert update path — change name to conflict.
    let err = table
        .upsert_no_fk(UniqueNamedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            name: "Strength".to_string(),
        })
        .unwrap_err();
    assert!(matches!(err, tabulosity::Error::DuplicateIndex { .. }));
    // Original should be unchanged.
    assert_eq!(
        table.get(&(CreatureId(2), TraitKind(20))).unwrap().name,
        "Speed"
    );
}

// --- Same-type compound PK with unique index ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("x", "y")]
struct SameTypePk {
    pub x: u32,
    pub y: u32,
    #[indexed(unique)]
    pub label: String,
}

#[test]
fn same_type_pk_unique_index_conflict() {
    let mut table = SameTypePkTable::new();
    // x values small, y values large — tests bounds sharing correctness.
    table
        .insert_no_fk(SameTypePk {
            x: 1,
            y: 100,
            label: "alpha".to_string(),
        })
        .unwrap();
    table
        .insert_no_fk(SameTypePk {
            x: 2,
            y: 200,
            label: "beta".to_string(),
        })
        .unwrap();
    let err = table
        .insert_no_fk(SameTypePk {
            x: 3,
            y: 300,
            label: "alpha".to_string(), // conflicts
        })
        .unwrap_err();
    assert!(matches!(err, tabulosity::Error::DuplicateIndex { .. }));
}

// --- Compound struct-level index on compound PK table ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind")]
#[index(name = "value_label", fields("value", "label"))]
struct CompoundIndexedTrait {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
    pub label: String,
}

#[test]
fn compound_index_on_compound_pk_table() {
    let mut table = CompoundIndexedTraitTable::new();
    table
        .insert_no_fk(CompoundIndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
            label: "str".to_string(),
        })
        .unwrap();
    table
        .insert_no_fk(CompoundIndexedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(10),
            value: 42,
            label: "dex".to_string(),
        })
        .unwrap();
    table
        .insert_no_fk(CompoundIndexedTrait {
            creature_id: CreatureId(3),
            trait_kind: TraitKind(20),
            value: 99,
            label: "str".to_string(),
        })
        .unwrap();

    // Query by both fields exact.
    let results = table.by_value_label(&42, &"str".to_string(), QueryOpts::ASC);
    assert_eq!(results.len(), 1);

    // Query by first field only.
    let results = table.by_value_label(&42, tabulosity::MatchAll, QueryOpts::ASC);
    assert_eq!(results.len(), 2);
}

// --- modify_each_by on compound PK table ---

#[test]
fn modify_each_by_on_compound_pk() {
    let mut table = IndexedTraitTable::new();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(3),
            trait_kind: TraitKind(30),
            value: 99,
        })
        .unwrap();

    // Use modify_each_by_value to modify all rows with value 42 —
    // but only change non-indexed fields (value is indexed, so we can't
    // change it via modify_each). Since our only non-PK non-indexed field
    // doesn't exist here, we'll just verify it runs and returns correct count.
    // Actually, IndexedTrait has no non-indexed non-PK fields, so we just
    // verify the method exists and can be called.
    let count = table.modify_each_by_value(&42, QueryOpts::ASC, |_pk, _row| {
        // No fields to safely modify (value is indexed, PK fields are PK).
        // Just verify the callback fires.
    });
    assert_eq!(count, 2);
}

// --- DESC query ordering on compound PK table ---

#[test]
fn desc_query_on_compound_pk_secondary_index() {
    let mut table = IndexedTraitTable::new();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(3),
            trait_kind: TraitKind(30),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            value: 42,
        })
        .unwrap();

    // ASC: ordered by compound PK within same index value.
    let asc = table.by_value(&42, QueryOpts::ASC);
    assert_eq!(asc[0].creature_id, CreatureId(1));
    assert_eq!(asc[1].creature_id, CreatureId(2));
    assert_eq!(asc[2].creature_id, CreatureId(3));

    // DESC: reversed.
    let desc = table.by_value(&42, QueryOpts::DESC);
    assert_eq!(desc[0].creature_id, CreatureId(3));
    assert_eq!(desc[1].creature_id, CreatureId(2));
    assert_eq!(desc[2].creature_id, CreatureId(1));
}

#[test]
fn desc_query_with_offset_on_compound_pk() {
    let mut table = IndexedTraitTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(IndexedTrait {
                creature_id: CreatureId(i),
                trait_kind: TraitKind(i * 10),
                value: 1,
            })
            .unwrap();
    }

    let opts = QueryOpts {
        order: tabulosity::QueryOrder::Desc,
        offset: 2,
    };
    let results = table.by_value(&1, opts);
    assert_eq!(results.len(), 3); // 5 total, skip 2
    assert_eq!(results[0].creature_id, CreatureId(3));
    assert_eq!(results[2].creature_id, CreatureId(1));
}

// --- Filtered index on compound PK table ---

fn is_positive(row: &FilteredTrait) -> bool {
    row.value > 0
}

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind")]
#[index(name = "positive_value", fields("value"), filter = "is_positive")]
struct FilteredTrait {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
}

#[test]
fn filtered_index_on_compound_pk_table() {
    let mut table = FilteredTraitTable::new();
    table
        .insert_no_fk(FilteredTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(FilteredTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            value: -5, // excluded by filter
        })
        .unwrap();
    table
        .insert_no_fk(FilteredTrait {
            creature_id: CreatureId(3),
            trait_kind: TraitKind(30),
            value: 42,
        })
        .unwrap();

    // Only positive rows should be in the index.
    assert_eq!(table.count_by_positive_value(&42, QueryOpts::ASC), 2);
    assert_eq!(table.count_by_positive_value(&(-5), QueryOpts::ASC), 0);
}

#[test]
fn filtered_index_maintained_through_update() {
    let mut table = FilteredTraitTable::new();
    table
        .insert_no_fk(FilteredTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    assert_eq!(table.count_by_positive_value(&42, QueryOpts::ASC), 1);

    // Update to negative — should be removed from filtered index.
    table
        .update_no_fk(FilteredTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: -1,
        })
        .unwrap();
    assert_eq!(table.count_by_positive_value(&42, QueryOpts::ASC), 0);
    assert_eq!(table.count_by_positive_value(&(-1), QueryOpts::ASC), 0);

    // Update back to positive — should re-enter filtered index.
    table
        .update_no_fk(FilteredTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 99,
        })
        .unwrap();
    assert_eq!(table.count_by_positive_value(&99, QueryOpts::ASC), 1);
}

// --- Explicit manual_rebuild_all_indexes on compound PK table ---

#[test]
fn rebuild_indexes_compound_pk() {
    let mut table = IndexedTraitTable::new();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(IndexedTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            value: 42,
        })
        .unwrap();

    // Verify index works before rebuild.
    assert_eq!(table.count_by_value(&42, QueryOpts::ASC), 2);

    // Force rebuild and verify indexes still correct.
    table.manual_rebuild_all_indexes();
    assert_eq!(table.count_by_value(&42, QueryOpts::ASC), 2);
    assert_eq!(table.count_by_value(&99, QueryOpts::ASC), 0);
}

// --- Three-column compound PK ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct SlotId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind", "slot")]
struct ThreeColumnPk {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub slot: SlotId,
    pub value: i32,
}

#[test]
fn three_column_pk_crud() {
    let mut table = ThreeColumnPkTable::new();
    table
        .insert_no_fk(ThreeColumnPk {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            slot: SlotId(0),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(ThreeColumnPk {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            slot: SlotId(1),
            value: 43,
        })
        .unwrap();
    assert_eq!(table.len(), 2);
    assert_eq!(
        table
            .get(&(CreatureId(1), TraitKind(10), SlotId(0)))
            .unwrap()
            .value,
        42
    );
    assert_eq!(
        table
            .get(&(CreatureId(1), TraitKind(10), SlotId(1)))
            .unwrap()
            .value,
        43
    );

    // Remove one.
    table
        .remove_no_fk(&(CreatureId(1), TraitKind(10), SlotId(0)))
        .unwrap();
    assert_eq!(table.len(), 1);
}
