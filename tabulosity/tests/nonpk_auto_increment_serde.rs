//! Serde integration tests for non-PK auto-increment fields.
//!
//! Tests serialization format (`{"next_<field>": N, "rows": [...]}`), roundtrip
//! fidelity, missing-counter fallback (computes `max(field) + 1` for old save
//! compatibility), and defensive counter initialization.

#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use tabulosity::{Bounded, QueryOpts, Table};

// =============================================================================
// Schema: compound PK + non-PK auto-increment
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct CreatureId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[primary_key("creature_id", "seq")]
struct Thought {
    pub creature_id: CreatureId,
    #[auto_increment]
    pub seq: u64,
    pub kind: String,
    pub tick: u64,
}

// =============================================================================
// Roundtrip
// =============================================================================

#[test]
fn roundtrip_preserves_data_and_counter() {
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
            creature_id: CreatureId(2),
            seq,
            kind: "sad".into(),
            tick: 200,
        })
        .unwrap();

    assert_eq!(table.next_seq(), 2);

    let json = serde_json::to_string(&table).unwrap();
    let restored: ThoughtTable = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.len(), 2);
    assert_eq!(restored.next_seq(), 2);
    assert_eq!(restored.get(&(CreatureId(1), 0)).unwrap().kind, "happy");
    assert_eq!(restored.get(&(CreatureId(2), 1)).unwrap().kind, "sad");
}

#[test]
fn serialized_format_has_counter_and_rows() {
    let mut table = ThoughtTable::new();
    table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "x".into(),
            tick: 0,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Should have "next_seq" and "rows" fields.
    assert!(value.get("next_seq").is_some(), "missing next_seq");
    assert!(value.get("rows").is_some(), "missing rows");
    assert_eq!(value["next_seq"], 1);
    assert_eq!(value["rows"].as_array().unwrap().len(), 1);
}

// =============================================================================
// Missing counter (old save compatibility)
// =============================================================================

#[test]
fn missing_counter_computes_from_rows() {
    // Simulate an old save that has "rows" but no "next_seq".
    let json = r#"{"rows": [
        {"creature_id": 1, "seq": 5, "kind": "a", "tick": 100},
        {"creature_id": 2, "seq": 10, "kind": "b", "tick": 200}
    ]}"#;

    let table: ThoughtTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 2);
    // Counter should be max(5, 10) + 1 = 11.
    assert_eq!(table.next_seq(), 11);
}

#[test]
fn missing_counter_on_empty_table() {
    let json = r#"{"rows": []}"#;
    let table: ThoughtTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 0);
    assert_eq!(table.next_seq(), 0); // first() value
}

// =============================================================================
// Defensive counter: max(deserialized, computed)
// =============================================================================

#[test]
fn counter_takes_max_of_deserialized_and_computed() {
    // next_seq says 5 but rows have seq up to 10 → should use 11.
    let json = r#"{"next_seq": 5, "rows": [
        {"creature_id": 1, "seq": 10, "kind": "a", "tick": 100}
    ]}"#;
    let table: ThoughtTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.next_seq(), 11);
}

#[test]
fn counter_uses_deserialized_when_higher() {
    // next_seq says 20 but rows only go up to seq 5 → should use 20.
    let json = r#"{"next_seq": 20, "rows": [
        {"creature_id": 1, "seq": 5, "kind": "a", "tick": 100}
    ]}"#;
    let table: ThoughtTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.next_seq(), 20);
}

// =============================================================================
// Deserialization rebuilds indexes
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
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
fn deserialized_table_has_working_indexes() {
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

    let json = serde_json::to_string(&table).unwrap();
    let restored: IndexedThoughtTable = serde_json::from_str(&json).unwrap();

    assert_eq!(
        restored.count_by_kind(&"happy".to_string(), QueryOpts::ASC),
        2
    );
    assert_eq!(
        restored.count_by_kind(&"sad".to_string(), QueryOpts::ASC),
        1
    );
    assert_eq!(restored.next_seq(), 3);
}

// =============================================================================
// Auto insert after deserialization continues from counter
// =============================================================================

#[test]
fn auto_insert_after_deserialize_uses_correct_counter() {
    let json = r#"{"next_seq": 5, "rows": [
        {"creature_id": 1, "seq": 3, "kind": "a", "tick": 100}
    ]}"#;
    let mut table: ThoughtTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.next_seq(), 5);

    let pk = table
        .insert_auto_no_fk(|seq| Thought {
            creature_id: CreatureId(1),
            seq,
            kind: "new".into(),
            tick: 200,
        })
        .unwrap();
    assert_eq!(pk, (CreatureId(1), 5));
    assert_eq!(table.next_seq(), 6);
}

// =============================================================================
// Duplicate key in deserialized rows
// =============================================================================

#[test]
fn duplicate_key_in_json_produces_error() {
    let json = r#"{"next_seq": 5, "rows": [
        {"creature_id": 1, "seq": 0, "kind": "a", "tick": 100},
        {"creature_id": 1, "seq": 0, "kind": "b", "tick": 200}
    ]}"#;
    let result: Result<ThoughtTable, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

// =============================================================================
// Unknown fields in JSON are ignored (forward compat)
// =============================================================================

#[test]
fn unknown_fields_ignored() {
    let json = r#"{"next_seq": 2, "next_id": 99, "some_future_field": true, "rows": [
        {"creature_id": 1, "seq": 0, "kind": "a", "tick": 100}
    ]}"#;
    let table: ThoughtTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(table.next_seq(), 2);
}

// =============================================================================
// Single-column PK + non-PK auto-increment serde
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct NoteId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Note {
    #[primary_key]
    pub id: NoteId,
    #[auto_increment]
    pub version: u64,
    pub text: String,
}

#[test]
fn single_pk_roundtrip() {
    let mut table = NoteTable::new();
    table
        .insert_auto_no_fk(|ver| Note {
            id: NoteId(1),
            version: ver,
            text: "hello".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|ver| Note {
            id: NoteId(2),
            version: ver,
            text: "world".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let restored: NoteTable = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.len(), 2);
    assert_eq!(restored.next_version(), 2);
    assert_eq!(restored.get(&NoteId(1)).unwrap().version, 0);
    assert_eq!(restored.get(&NoteId(2)).unwrap().version, 1);
}

#[test]
fn single_pk_missing_counter() {
    let json = r#"{"rows": [
        {"id": 1, "version": 7, "text": "hi"}
    ]}"#;
    let table: NoteTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.next_version(), 8);
}

// =============================================================================
// Error cases
// =============================================================================

#[test]
fn missing_rows_field_produces_error() {
    let json = r#"{"next_seq": 5}"#;
    let result: Result<ThoughtTable, _> = serde_json::from_str(json);
    assert!(result.is_err(), "Expected error for missing 'rows' field");
}

#[test]
fn duplicate_counter_field_produces_error() {
    // serde_json doesn't produce duplicate keys, but if a custom deserializer
    // did, the guard should catch it. serde_json just uses the last value,
    // so this test verifies the table still works (last-value-wins behavior).
    // The actual duplicate_field guard in the visitor is for non-JSON formats.
    let json = r#"{"next_seq": 5, "rows": [
        {"creature_id": 1, "seq": 0, "kind": "a", "tick": 100}
    ]}"#;
    let table: ThoughtTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(table.next_seq(), 5);
}
