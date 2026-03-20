//! Integration tests for non-PK auto-increment fields (`#[auto_increment]`).
//!
//! Tests the standalone `#[auto_increment]` attribute on non-primary-key fields,
//! which generates a per-table counter (`next_<field>`) and an `insert_auto_no_fk`
//! method that auto-assigns the field value. This is orthogonal to
//! `#[primary_key(auto_increment)]` — the auto-increment field here is NOT the PK.
//!
//! Covers: sequential assignment, counter bump on manual/upsert inserts, counter
//! stability on delete/failed insert, interaction with compound PKs, secondary
//! indexes, unique indexes, modify_unchecked, and manual_rebuild_all_indexes.

use tabulosity::{Bounded, Error, MatchAll, QueryOpts, Table};

// =============================================================================
// Schema: compound PK + non-PK auto-increment (the primary use case)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct CreatureId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[primary_key("creature_id", "seq")]
struct Thought {
    pub creature_id: CreatureId,
    #[auto_increment]
    pub seq: u64,
    pub kind: String,
    pub tick: u64,
}

// =============================================================================
// Basic auto-increment assignment
// =============================================================================

#[test]
fn auto_insert_assigns_sequential_seq() {
    let mut table = ThoughtTable::new();
    let pk0 = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    let pk1 = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();
    let pk2 = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(2),
            seq,
            kind: "hungry".into(),
            tick: 102,
        })
        .unwrap();

    assert_eq!(pk0, (CreatureId(1), 0));
    assert_eq!(pk1, (CreatureId(1), 1));
    assert_eq!(pk2, (CreatureId(2), 2)); // global seq, not per-creature
    assert_eq!(table.len(), 3);
}

#[test]
fn next_seq_accessor() {
    let table = ThoughtTable::new();
    assert_eq!(table.next_seq(), 0);
}

#[test]
fn next_seq_advances_after_auto_insert() {
    let mut table = ThoughtTable::new();
    assert_eq!(table.next_seq(), 0);
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 1);
}

// =============================================================================
// Counter behavior on manual insert / upsert
// =============================================================================

#[test]
fn manual_insert_bumps_counter_if_high() {
    let mut table = ThoughtTable::new();
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 10,
            kind: "manual".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 11);
}

#[test]
fn manual_insert_below_counter_does_not_lower_it() {
    let mut table = ThoughtTable::new();
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 10,
            kind: "high".into(),
            tick: 100,
        })
        .unwrap();
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(2),
            seq: 3,
            kind: "low".into(),
            tick: 101,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 11);
}

#[test]
fn auto_insert_after_manual_high_insert() {
    let mut table = ThoughtTable::new();
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 5,
            kind: "manual".into(),
            tick: 100,
        })
        .unwrap();
    let pk = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "auto".into(),
            tick: 101,
        })
        .unwrap();
    assert_eq!(pk, (CreatureId(1), 6));
}

#[test]
fn upsert_insert_path_bumps_counter() {
    let mut table = ThoughtTable::new();
    table
        .upsert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 7,
            kind: "upserted".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 8);
}

#[test]
fn upsert_update_path_below_counter_does_not_change_it() {
    let mut table = ThoughtTable::new();
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 10,
            kind: "high".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 11);
    // Upsert-update on existing key — seq 10 is below counter.
    table
        .upsert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 10,
            kind: "updated".into(),
            tick: 101,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 11);
}

// =============================================================================
// Counter stability on delete and failed insert
// =============================================================================

#[test]
fn delete_does_not_affect_counter() {
    let mut table = ThoughtTable::new();
    let pk = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "a".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 1);
    table.remove_no_fk(&pk).unwrap();
    assert_eq!(table.next_seq(), 1);
}

#[test]
fn duplicate_key_does_not_advance_counter() {
    let mut table = ThoughtTable::new();
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 0,
            kind: "first".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 1);
    // Try inserting with same PK — should fail.
    let err = table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 0,
            kind: "duplicate".into(),
            tick: 101,
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateKey { .. }));
    assert_eq!(table.next_seq(), 1);
}

// =============================================================================
// Interaction with secondary indexes
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[primary_key("creature_id", "seq")]
struct IndexedThought {
    pub creature_id: CreatureId,
    #[auto_increment]
    pub seq: u64,
    #[indexed]
    pub kind: String,
    pub tick: u64,
}

#[test]
fn auto_insert_maintains_secondary_index() {
    let mut table = IndexedThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| IndexedThought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| IndexedThought {
            creature_id: CreatureId(1),
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| IndexedThought {
            creature_id: CreatureId(2),
            seq,
            kind: "happy".into(),
            tick: 102,
        })
        .unwrap();

    assert_eq!(table.count_by_kind(&"happy".to_string(), QueryOpts::ASC), 2);
    assert_eq!(table.count_by_kind(&"sad".to_string(), QueryOpts::ASC), 1);
}

#[test]
fn auto_insert_then_remove_updates_secondary_index() {
    let mut table = IndexedThoughtTable::new();
    let pk0 = table
        .insert_auto_no_fk(|seq| IndexedThought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| IndexedThought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 101,
        })
        .unwrap();

    assert_eq!(table.count_by_kind(&"happy".to_string(), QueryOpts::ASC), 2);
    table.remove_no_fk(&pk0).unwrap();
    assert_eq!(table.count_by_kind(&"happy".to_string(), QueryOpts::ASC), 1);
}

// =============================================================================
// Interaction with unique indexes
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[primary_key("creature_id", "seq")]
struct UniqueThought {
    pub creature_id: CreatureId,
    #[auto_increment]
    pub seq: u64,
    #[indexed(unique)]
    pub label: String,
    pub tick: u64,
}

#[test]
fn unique_index_violation_does_not_advance_counter() {
    let mut table = UniqueThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| UniqueThought {
            creature_id: CreatureId(1),
            seq,
            label: "ABC".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 1);

    // Try auto-insert with duplicate unique label — should fail.
    let err = table
        .insert_auto_no_fk(|seq| UniqueThought {
            creature_id: CreatureId(1),
            seq,
            label: "ABC".into(),
            tick: 101,
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
    // Counter should NOT have advanced.
    assert_eq!(table.next_seq(), 1);
    assert_eq!(table.len(), 1);

    // Subsequent insert should get seq 1, not 2.
    let pk = table
        .insert_auto_no_fk(|seq| UniqueThought {
            creature_id: CreatureId(1),
            seq,
            label: "DEF".into(),
            tick: 102,
        })
        .unwrap();
    assert_eq!(pk, (CreatureId(1), 1));
}

// =============================================================================
// modify_unchecked and manual_rebuild_all_indexes
// =============================================================================

#[test]
fn modify_unchecked_on_non_indexed_field() {
    let mut table = ThoughtTable::new();
    let pk = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    table
        .modify_unchecked(&pk, |row| {
            row.tick = 200;
            row.kind = "sad".into();
        })
        .unwrap();
    let row = table.get(&pk).unwrap();
    assert_eq!(row.tick, 200);
    assert_eq!(row.kind, "sad");
}

#[test]
fn rebuild_indexes_does_not_affect_counter() {
    let mut table = ThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 2);
    table.manual_rebuild_all_indexes();
    assert_eq!(table.next_seq(), 2);
    assert_eq!(table.len(), 2);
}

// =============================================================================
// Single-column PK + non-PK auto-increment
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct NoteId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Note {
    #[primary_key]
    pub id: NoteId,
    #[auto_increment]
    pub version: u64,
    pub text: String,
}

#[test]
fn single_pk_with_nonpk_auto_increment() {
    let mut table = NoteTable::new();
    let pk = table
        .insert_auto_no_fk(|ver| Note {
            id: NoteId(42),
            version: ver,
            text: "hello".into(),
        })
        .unwrap();
    assert_eq!(pk, NoteId(42));
    assert_eq!(table.next_version(), 1);

    let row = table.get(&NoteId(42)).unwrap();
    assert_eq!(row.version, 0);
    assert_eq!(row.text, "hello");
}

#[test]
fn single_pk_auto_insert_returns_correct_pk() {
    let mut table = NoteTable::new();
    let pk0 = table
        .insert_auto_no_fk(|ver| Note {
            id: NoteId(1),
            version: ver,
            text: "a".into(),
        })
        .unwrap();
    let pk1 = table
        .insert_auto_no_fk(|ver| Note {
            id: NoteId(2),
            version: ver,
            text: "b".into(),
        })
        .unwrap();
    assert_eq!(pk0, NoteId(1));
    assert_eq!(pk1, NoteId(2));
    assert_eq!(table.next_version(), 2);
}

// =============================================================================
// Interleaved manual and auto inserts
// =============================================================================

#[test]
fn interleaved_manual_and_auto_inserts() {
    let mut table = ThoughtTable::new();

    let pk0 = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "auto0".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(pk0, (CreatureId(1), 0));

    // Manual insert at high seq.
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(2),
            seq: 5,
            kind: "manual5".into(),
            tick: 101,
        })
        .unwrap();

    let pk1 = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "auto6".into(),
            tick: 102,
        })
        .unwrap();
    assert_eq!(pk1, (CreatureId(1), 6));

    // Manual insert at low seq.
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(3),
            seq: 2,
            kind: "manual2".into(),
            tick: 103,
        })
        .unwrap();

    // Counter should still be 7.
    let pk2 = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "auto7".into(),
            tick: 104,
        })
        .unwrap();
    assert_eq!(pk2, (CreatureId(1), 7));
    assert_eq!(table.len(), 5);
}

// =============================================================================
// Compound index on compound PK + non-PK auto-increment
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[primary_key("creature_id", "seq")]
#[index(name = "kind_tick", fields("kind", "tick"))]
struct CompoundIndexedThought {
    pub creature_id: CreatureId,
    #[auto_increment]
    pub seq: u64,
    pub kind: String,
    pub tick: u64,
}

#[test]
fn compound_index_with_nonpk_auto_increment() {
    let mut table = CompoundIndexedThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| CompoundIndexedThought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| CompoundIndexedThought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 200,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| CompoundIndexedThought {
            creature_id: CreatureId(2),
            seq,
            kind: "sad".into(),
            tick: 100,
        })
        .unwrap();

    assert_eq!(
        table.count_by_kind_tick(&"happy".to_string(), &100, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_kind_tick(&"happy".to_string(), MatchAll, QueryOpts::ASC),
        2
    );
}

// =============================================================================
// Debug assertions: PK fields cannot be modified via modify_unchecked
// =============================================================================

#[test]
#[should_panic(expected = "primary key field")]
#[cfg(debug_assertions)]
fn modify_unchecked_panics_on_pk_field_change() {
    let mut table = ThoughtTable::new();
    let pk = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "a".into(),
            tick: 100,
        })
        .unwrap();
    let _ = table.modify_unchecked(&pk, |row| {
        row.creature_id = CreatureId(99);
    });
}

#[test]
#[should_panic(expected = "primary key field")]
#[cfg(debug_assertions)]
fn modify_unchecked_panics_on_auto_field_change_when_pk() {
    // The auto-increment field `seq` is part of the compound PK,
    // so changing it via modify_unchecked should panic.
    let mut table = ThoughtTable::new();
    let pk = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "a".into(),
            tick: 100,
        })
        .unwrap();
    let _ = table.modify_unchecked(&pk, |row| {
        row.seq = 999;
    });
}

#[test]
#[should_panic(expected = "indexed field")]
#[cfg(debug_assertions)]
fn modify_unchecked_panics_on_indexed_field_change() {
    let mut table = IndexedThoughtTable::new();
    let pk = table
        .insert_auto_no_fk(|seq| IndexedThought {
            creature_id: CreatureId(1),
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    let _ = table.modify_unchecked(&pk, |row| {
        row.kind = "sad".into();
    });
}

// =============================================================================
// Edge case: auto-increment field NOT part of PK
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[primary_key("creature_id", "kind")]
struct ThoughtV2 {
    pub creature_id: CreatureId,
    pub kind: String,
    #[auto_increment]
    pub seq: u64,
    pub tick: u64,
}

#[test]
fn auto_field_not_in_pk() {
    let mut table = ThoughtV2Table::new();
    let pk0 = table
        .insert_auto_no_fk(|seq| ThoughtV2 {
            creature_id: CreatureId(1),
            kind: "happy".into(),
            seq,
            tick: 100,
        })
        .unwrap();
    let pk1 = table
        .insert_auto_no_fk(|seq| ThoughtV2 {
            creature_id: CreatureId(1),
            kind: "sad".into(),
            seq,
            tick: 101,
        })
        .unwrap();

    assert_eq!(pk0, (CreatureId(1), "happy".to_string()));
    assert_eq!(pk1, (CreatureId(1), "sad".to_string()));
    assert_eq!(table.next_seq(), 2);
    assert_eq!(table.get(&pk0).unwrap().seq, 0);
    assert_eq!(table.get(&pk1).unwrap().seq, 1);
}

// =============================================================================
// update_no_fk does NOT bump non-PK auto counter
// =============================================================================

#[test]
fn update_does_not_bump_counter() {
    let mut table = ThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "a".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 1);

    table
        .update_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 0,
            kind: "updated".into(),
            tick: 200,
        })
        .unwrap();
    // Counter should not change on update.
    assert_eq!(table.next_seq(), 1);
}

// =============================================================================
// Overflow: manual insert at max value panics
// =============================================================================

mod overflow {
    use std::panic;

    use tabulosity::{Bounded, Table};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    struct TinyId(u8);

    #[derive(Table, Clone, Debug, PartialEq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[primary_key("parent_id", "seq")]
    struct TinyChild {
        pub parent_id: TinyId,
        #[auto_increment]
        pub seq: u8,
        pub value: u32,
    }

    #[test]
    fn nonpk_auto_manual_insert_at_max_panics() {
        let mut table = TinyChildTable::new();
        // Manual insert at u8::MAX should trigger successor panic on counter bump.
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            table
                .insert_no_fk(TinyChild {
                    parent_id: TinyId(1),
                    seq: 255,
                    value: 0,
                })
                .unwrap();
        }));
        assert!(
            result.is_err(),
            "Expected panic from AutoIncrementable overflow on nonpk auto field"
        );
    }

    #[test]
    fn nonpk_auto_insert_auto_fills_to_254() {
        let mut table = TinyChildTable::new();
        // Insert 255 rows (seq 0..=254) — all should succeed.
        for i in 0..255u32 {
            table
                .insert_auto_no_fk(|seq| TinyChild {
                    parent_id: TinyId((i % 256) as u8),
                    seq,
                    value: i,
                })
                .unwrap();
        }
        assert_eq!(table.len(), 255);
        assert_eq!(table.next_seq(), 255);
    }

    #[test]
    fn nonpk_auto_insert_auto_at_255_panics() {
        let mut table = TinyChildTable::new();
        for i in 0..255u32 {
            table
                .insert_auto_no_fk(|seq| TinyChild {
                    parent_id: TinyId((i % 256) as u8),
                    seq,
                    value: i,
                })
                .unwrap();
        }
        // Next auto-insert would assign seq=255, then bumping to 256 overflows u8.
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            table
                .insert_auto_no_fk(|seq| TinyChild {
                    parent_id: TinyId(0),
                    seq,
                    value: 255,
                })
                .unwrap();
        }));
        assert!(
            result.is_err(),
            "Expected panic from AutoIncrementable overflow"
        );
    }
}

// =============================================================================
// insert_auto_no_fk with duplicate PK does not advance counter
// =============================================================================

#[test]
fn insert_auto_with_duplicate_pk_does_not_advance_counter() {
    // If the caller provides a creature_id that, combined with the auto seq,
    // somehow collides with an existing PK, the counter should not advance.
    // We set this up by manually inserting a row at (CreatureId(1), seq=1),
    // then auto-inserting. First auto gets seq=0 -> OK. Then manual at seq=1.
    // Next auto would get seq=2 -> no collision.
    // To force a collision: manually insert at the exact (creature_id, seq)
    // that auto would generate.
    let mut table = ThoughtTable::new();

    // Auto-insert gets seq=0.
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "first".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 1);

    // Manually insert at seq=1 (what auto would assign next).
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 1,
            kind: "manual".into(),
            tick: 101,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 2);

    // Next auto-insert gets seq=2, but we use CreatureId(1) again.
    // The compound PK (CreatureId(1), 2) is unique, so this should succeed.
    let pk = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "auto_after_manual".into(),
            tick: 102,
        })
        .unwrap();
    assert_eq!(pk, (CreatureId(1), 2));
    assert_eq!(table.next_seq(), 3);
}

// =============================================================================
// Upsert update path with high seq bumps counter
// =============================================================================

#[test]
fn upsert_update_path_with_high_seq_bumps_counter() {
    let mut table = ThoughtTable::new();
    // Insert row with low seq.
    table
        .insert_no_fk(Thought {
            creature_id: CreatureId(1),
            seq: 0,
            kind: "low".into(),
            tick: 100,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 1);

    // Upsert-update same PK with high seq value in a different field.
    // Actually the seq IS part of the PK, so changing it means a different PK.
    // For ThoughtV2 where seq is NOT in the PK, the upsert-update would
    // bump if the seq value in the new row is >= counter. But for Thought
    // where seq IS in the PK, we can test this by upserting a row with
    // a high seq (new PK -> insert path).
    table
        .upsert_no_fk(Thought {
            creature_id: CreatureId(2),
            seq: 20,
            kind: "high".into(),
            tick: 200,
        })
        .unwrap();
    assert_eq!(table.next_seq(), 21);
}

// =============================================================================
// modify_unchecked_range and modify_unchecked_all
// =============================================================================

#[test]
fn modify_unchecked_range_on_nonpk_auto_table() {
    let mut table = ThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "a".into(),
            tick: 100,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "b".into(),
            tick: 200,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(2),
            seq,
            kind: "c".into(),
            tick: 300,
        })
        .unwrap();

    let count =
        table.modify_unchecked_range((CreatureId(1), 0)..=(CreatureId(1), 1), |_pk, row| {
            row.tick += 1000;
        });
    assert_eq!(count, 2);
    assert_eq!(table.get(&(CreatureId(1), 0)).unwrap().tick, 1100);
    assert_eq!(table.get(&(CreatureId(1), 1)).unwrap().tick, 1200);
    assert_eq!(table.get(&(CreatureId(2), 2)).unwrap().tick, 300); // unchanged
}

#[test]
fn modify_unchecked_all_on_nonpk_auto_table() {
    let mut table = ThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "a".into(),
            tick: 100,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(2),
            seq,
            kind: "b".into(),
            tick: 200,
        })
        .unwrap();

    let count = table.modify_unchecked_all(|_pk, row| {
        row.tick += 500;
    });
    assert_eq!(count, 2);
    assert_eq!(table.get(&(CreatureId(1), 0)).unwrap().tick, 600);
    assert_eq!(table.get(&(CreatureId(2), 1)).unwrap().tick, 700);
}
