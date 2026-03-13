//! Serde integration tests for parent-PK-as-child-PK (1:1 relations).
//!
//! Tests serialization/deserialization roundtrips, orphan detection, and schema
//! versioning for tables using the `pk` FK keyword.

#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use tabulosity::{Bounded, Database, Table};

// --- Row types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct CreatureId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CreatureStats {
    #[primary_key]
    pub id: CreatureId,
    pub health: u32,
    pub mana: u32,
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CreaturePosition {
    #[primary_key]
    pub id: CreatureId,
    pub x: i32,
    pub y: i32,
}

#[derive(Database)]
struct TestDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk))]
    pub creature_stats: CreatureStatsTable,

    #[table(singular = "creature_position", fks(id = "creatures" pk on_delete cascade))]
    pub creature_positions: CreaturePositionTable,
}

// --- Versioned DB ---

#[derive(Database)]
#[schema_version(1)]
struct VersionedDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk))]
    pub creature_stats: CreatureStatsTable,
}

// --- Helpers ---

fn make_creature(id: u32, name: &str) -> Creature {
    Creature {
        id: CreatureId(id),
        name: name.to_string(),
    }
}

fn make_stats(id: u32, health: u32, mana: u32) -> CreatureStats {
    CreatureStats {
        id: CreatureId(id),
        health,
        mana,
    }
}

// --- Roundtrip tests ---

#[test]
fn serde_roundtrip_empty() {
    let db = TestDb::new();
    let json = serde_json::to_string(&db).unwrap();
    let db2: TestDb = serde_json::from_str(&json).unwrap();
    assert_eq!(db2.creatures.len(), 0);
    assert_eq!(db2.creature_stats.len(), 0);
    assert_eq!(db2.creature_positions.len(), 0);
}

#[test]
fn serde_roundtrip_with_data() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_position(CreaturePosition {
        id: CreatureId(2),
        x: 10,
        y: 20,
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let db2: TestDb = serde_json::from_str(&json).unwrap();

    assert_eq!(db2.creatures.len(), 2);
    assert_eq!(db2.creature_stats.len(), 1);
    assert_eq!(db2.creature_stats.get(&CreatureId(1)).unwrap().health, 100);
    assert_eq!(db2.creature_positions.len(), 1);
    assert_eq!(db2.creature_positions.get(&CreatureId(2)).unwrap().x, 10);
}

#[test]
fn serde_roundtrip_preserves_all_fields() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 42, 99)).unwrap();

    let json = serde_json::to_string_pretty(&db).unwrap();
    let db2: TestDb = serde_json::from_str(&json).unwrap();
    let stats = db2.creature_stats.get(&CreatureId(1)).unwrap();
    assert_eq!(stats.health, 42);
    assert_eq!(stats.mana, 99);
}

// --- Orphan detection (FK validation on deserialize) ---

#[test]
fn serde_orphaned_child_detected() {
    // Build JSON with a child row whose parent doesn't exist.
    let json = r#"{
        "creatures": [],
        "creature_stats": [{"id": 1, "health": 100, "mana": 50}],
        "creature_positions": []
    }"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    assert!(result.is_err(), "should reject orphaned child row");
    let err_msg = result.err().unwrap().to_string();
    assert!(
        err_msg.contains("FK target not found"),
        "error should mention FK target not found: {err_msg}"
    );
}

#[test]
fn serde_valid_parent_child_accepted() {
    let json = r#"{
        "creatures": [{"id": 1, "name": "Elf"}],
        "creature_stats": [{"id": 1, "health": 100, "mana": 50}],
        "creature_positions": []
    }"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creature_stats.len(), 1);
}

#[test]
fn serde_multiple_orphans_all_reported() {
    let json = r#"{
        "creatures": [],
        "creature_stats": [{"id": 1, "health": 100, "mana": 50}],
        "creature_positions": [{"id": 2, "x": 10, "y": 20}]
    }"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    assert!(result.is_err());
    let err_msg = result.err().unwrap().to_string();
    // Both orphans should be reported.
    assert!(
        err_msg.contains("creature_stats"),
        "should mention creature_stats table: {err_msg}"
    );
    assert!(
        err_msg.contains("creature_positions"),
        "should mention creature_positions table: {err_msg}"
    );
}

// --- Missing child table in JSON defaults to empty ---

#[test]
fn serde_missing_child_table_defaults_empty() {
    let json = r#"{
        "creatures": [{"id": 1, "name": "Elf"}]
    }"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creature_stats.len(), 0);
    assert_eq!(db.creature_positions.len(), 0);
}

// --- Versioned DB ---

#[test]
fn serde_versioned_roundtrip() {
    let mut db = VersionedDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();

    let json = serde_json::to_string(&db).unwrap();
    assert!(json.contains("\"schema_version\":1"));

    let db2: VersionedDb = serde_json::from_str(&json).unwrap();
    assert_eq!(db2.creatures.len(), 1);
    assert_eq!(db2.creature_stats.len(), 1);
}

// --- Duplicate PK in deserialized child table ---

#[test]
fn serde_auto_parent_pk_fk_roundtrip() {
    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize,
    )]
    struct AutoId(u32);

    #[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct AutoParent {
        #[primary_key(auto_increment)]
        pub id: AutoId,
        pub name: String,
    }

    #[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct AutoChild {
        #[primary_key]
        pub id: AutoId,
        pub value: u32,
    }

    #[derive(Database)]
    struct AutoDb {
        #[table(singular = "auto_parent", auto)]
        pub auto_parents: AutoParentTable,

        #[table(singular = "auto_child", fks(id = "auto_parents" pk on_delete cascade))]
        pub auto_children: AutoChildTable,
    }

    let mut db = AutoDb::new();
    db.insert_auto_parent_auto(|id| AutoParent {
        id,
        name: "first".to_string(),
    })
    .unwrap();
    db.insert_auto_parent_auto(|id| AutoParent {
        id,
        name: "second".to_string(),
    })
    .unwrap();
    db.insert_auto_child(AutoChild {
        id: AutoId(0),
        value: 42,
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let db2: AutoDb = serde_json::from_str(&json).unwrap();

    assert_eq!(db2.auto_parents.len(), 2);
    assert_eq!(db2.auto_children.len(), 1);
    assert_eq!(db2.auto_children.get(&AutoId(0)).unwrap().value, 42);
    // Verify auto-increment counter is preserved.
    assert_eq!(db2.auto_parents.next_id(), AutoId(2));
}

#[test]
fn serde_duplicate_child_pk_detected() {
    let json = r#"{
        "creatures": [{"id": 1, "name": "Elf"}],
        "creature_stats": [
            {"id": 1, "health": 100, "mana": 50},
            {"id": 1, "health": 200, "mana": 60}
        ],
        "creature_positions": []
    }"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    assert!(result.is_err(), "should reject duplicate child PK");
    let err_msg = result.err().unwrap().to_string();
    assert!(
        err_msg.contains("duplicate key"),
        "error should mention duplicate key: {err_msg}"
    );
}
