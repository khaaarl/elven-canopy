//! Database-level integration tests for non-PK auto-increment tables.
//!
//! Tests that tables with `#[auto_increment]` on a non-PK field work correctly
//! within a `#[derive(Database)]` schema: FK validation, cascade delete,
//! and serde roundtrip.

use tabulosity::{Bounded, Database, Error, QueryOpts, Table};

// =============================================================================
// Schema
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct CreatureId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Creature {
    #[primary_key(auto_increment)]
    pub id: CreatureId,
    pub name: String,
}

#[derive(Table, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[primary_key("creature_id", "seq")]
struct Thought {
    #[indexed]
    pub creature_id: CreatureId,
    #[auto_increment]
    pub seq: u64,
    pub kind: String,
    pub tick: u64,
}

#[derive(Database)]
struct TestDb {
    #[table(singular = "creature", auto)]
    pub creatures: CreatureTable,

    #[table(singular = "thought", nonpk_auto, fks(creature_id = "creatures" on_delete cascade))]
    pub thoughts: ThoughtTable,
}

// =============================================================================
// Basic operations
// =============================================================================

#[test]
fn insert_thought_via_table_level_auto() {
    let mut db = TestDb::new();
    let cid = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf".into(),
        })
        .unwrap();

    // Use table-level insert_auto_no_fk (no DB-level wrapper for nonpk_auto).
    let pk = db
        .thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: cid,
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();

    assert_eq!(pk, (cid, 0));
    assert_eq!(db.thoughts.len(), 1);
    assert_eq!(db.thoughts.next_seq(), 1);
}

#[test]
fn multiple_thoughts_same_creature() {
    let mut db = TestDb::new();
    let cid = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf".into(),
        })
        .unwrap();

    let pk0 = db
        .thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: cid,
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    let pk1 = db
        .thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: cid,
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();

    assert_eq!(pk0, (cid, 0));
    assert_eq!(pk1, (cid, 1));
    assert_eq!(db.thoughts.len(), 2);
}

// =============================================================================
// Cascade delete
// =============================================================================

#[test]
fn cascade_delete_removes_thoughts() {
    let mut db = TestDb::new();
    let cid = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf".into(),
        })
        .unwrap();

    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: cid,
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: cid,
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();

    assert_eq!(db.thoughts.len(), 2);

    db.remove_creature(&cid).unwrap();
    assert!(db.creatures.is_empty());
    assert!(db.thoughts.is_empty());

    // Counter should not have reset.
    assert_eq!(db.thoughts.next_seq(), 2);
}

#[test]
fn cascade_delete_only_removes_correct_creature_thoughts() {
    let mut db = TestDb::new();
    let c1 = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf A".into(),
        })
        .unwrap();
    let c2 = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf B".into(),
        })
        .unwrap();

    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: c1,
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: c2,
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();

    assert_eq!(db.thoughts.len(), 2);

    db.remove_creature(&c1).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.thoughts.len(), 1);
    assert_eq!(db.thoughts.get(&(c2, 1)).unwrap().kind, "sad");
}

// =============================================================================
// DB-level insert with FK validation (manual path)
// =============================================================================

#[test]
fn insert_thought_with_db_level_fk_check() {
    let mut db = TestDb::new();
    let cid = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf".into(),
        })
        .unwrap();

    // Use DB-level insert_thought (validates FKs).
    db.insert_thought(Thought {
        creature_id: cid,
        seq: 42,
        kind: "manual".into(),
        tick: 100,
    })
    .unwrap();

    assert_eq!(db.thoughts.len(), 1);
    assert_eq!(db.thoughts.next_seq(), 43);
}

#[test]
fn insert_thought_with_invalid_fk_fails() {
    let mut db = TestDb::new();
    let err = db
        .insert_thought(Thought {
            creature_id: CreatureId(99),
            seq: 0,
            kind: "orphan".into(),
            tick: 100,
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
    assert_eq!(db.thoughts.len(), 0);
}

// =============================================================================
// Index queries after database operations
// =============================================================================

#[test]
fn index_queries_after_auto_insert() {
    let mut db = TestDb::new();
    let c1 = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf A".into(),
        })
        .unwrap();
    let c2 = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf B".into(),
        })
        .unwrap();

    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: c1,
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: c1,
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();
    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: c2,
            seq,
            kind: "happy".into(),
            tick: 102,
        })
        .unwrap();

    assert_eq!(db.thoughts.count_by_creature_id(&c1, QueryOpts::ASC), 2);
    assert_eq!(db.thoughts.count_by_creature_id(&c2, QueryOpts::ASC), 1);
}

// =============================================================================
// Serde roundtrip
// =============================================================================

#[cfg(feature = "serde")]
#[test]
fn db_roundtrip_preserves_nonpk_auto_counter() {
    let mut db = TestDb::new();
    let cid = db
        .insert_creature_auto(|pk| Creature {
            id: pk,
            name: "Elf".into(),
        })
        .unwrap();

    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: cid,
            seq,
            kind: "happy".into(),
            tick: 100,
        })
        .unwrap();
    db.thoughts
        .insert_auto_no_fk(|seq| Thought {
            creature_id: cid,
            seq,
            kind: "sad".into(),
            tick: 101,
        })
        .unwrap();

    assert_eq!(db.thoughts.next_seq(), 2);

    let json = serde_json::to_string(&db).unwrap();
    let restored: TestDb = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.creatures.len(), 1);
    assert_eq!(restored.thoughts.len(), 2);
    assert_eq!(restored.thoughts.next_seq(), 2);

    // Index still works after roundtrip.
    assert_eq!(
        restored.thoughts.count_by_creature_id(&cid, QueryOpts::ASC),
        2
    );
}

#[cfg(feature = "serde")]
#[test]
fn db_serde_missing_nonpk_auto_table_defaults_to_empty() {
    // Simulate old save that doesn't have the "thoughts" key at all.
    // The Database derive should default it to an empty table.
    let json = r#"{"creatures": {"next_id": 1, "rows": [{"id": 0, "name": "Elf"}]}}"#;
    let restored: TestDb = serde_json::from_str(json).unwrap();
    assert_eq!(restored.creatures.len(), 1);
    assert_eq!(restored.thoughts.len(), 0);
    assert_eq!(restored.thoughts.next_seq(), 0);
}
