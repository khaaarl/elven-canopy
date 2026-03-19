//! Serde integration tests for compound (multi-column) primary keys.

#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use tabulosity::{Bounded, Database, Table};

// --- Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct CreatureId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct TraitKind(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[primary_key("creature_id", "trait_kind")]
struct CreatureTrait {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
}

// --- Roundtrip tests ---

#[test]
fn serde_roundtrip_empty() {
    let table = CreatureTraitTable::new();
    let json = serde_json::to_string(&table).unwrap();
    let table2: CreatureTraitTable = serde_json::from_str(&json).unwrap();
    assert_eq!(table2.len(), 0);
}

#[test]
fn serde_roundtrip_with_data() {
    let mut table = CreatureTraitTable::new();
    table
        .insert_no_fk(CreatureTrait {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(CreatureTrait {
            creature_id: CreatureId(2),
            trait_kind: TraitKind(20),
            value: 99,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: CreatureTraitTable = serde_json::from_str(&json).unwrap();
    assert_eq!(table2.len(), 2);
    assert_eq!(
        table2.get(&(CreatureId(1), TraitKind(10))).unwrap().value,
        42
    );
    assert_eq!(
        table2.get(&(CreatureId(2), TraitKind(20))).unwrap().value,
        99
    );
}

#[test]
fn serde_duplicate_compound_pk_detected() {
    // Manually construct JSON with duplicate compound key.
    let json = r#"[
        {"creature_id": 1, "trait_kind": 10, "value": 42},
        {"creature_id": 1, "trait_kind": 10, "value": 99}
    ]"#;
    match serde_json::from_str::<CreatureTraitTable>(json) {
        Ok(_) => panic!("expected deserialization to fail"),
        Err(e) => assert!(
            e.to_string().contains("duplicate key"),
            "expected duplicate key error, got: {e}"
        ),
    }
}

#[test]
fn serde_roundtrip_preserves_index() {
    use tabulosity::QueryOpts;

    #[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[primary_key("creature_id", "trait_kind")]
    struct IndexedTrait {
        pub creature_id: CreatureId,
        pub trait_kind: TraitKind,
        #[indexed]
        pub value: i32,
    }

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

    let json = serde_json::to_string(&table).unwrap();
    let table2: IndexedTraitTable = serde_json::from_str(&json).unwrap();

    // Index should have been rebuilt.
    assert_eq!(table2.count_by_value(&42, QueryOpts::ASC), 2);
}

// --- Three-column compound PK serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct SlotId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[primary_key("creature_id", "trait_kind", "slot")]
struct ThreeColumnRow {
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub slot: SlotId,
    pub value: i32,
}

#[test]
fn serde_roundtrip_three_column_pk() {
    let mut table = ThreeColumnRowTable::new();
    table
        .insert_no_fk(ThreeColumnRow {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            slot: SlotId(0),
            value: 42,
        })
        .unwrap();
    table
        .insert_no_fk(ThreeColumnRow {
            creature_id: CreatureId(1),
            trait_kind: TraitKind(10),
            slot: SlotId(1),
            value: 43,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: ThreeColumnRowTable = serde_json::from_str(&json).unwrap();
    assert_eq!(table2.len(), 2);
    assert_eq!(
        table2
            .get(&(CreatureId(1), TraitKind(10), SlotId(0)))
            .unwrap()
            .value,
        42
    );
    assert_eq!(
        table2
            .get(&(CreatureId(1), TraitKind(10), SlotId(1)))
            .unwrap()
            .value,
        43
    );
}

// --- Database serde with compound PK child ---

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[primary_key("creature_id", "trait_kind")]
struct CreatureTraitFk {
    #[indexed]
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
}

#[derive(Database)]
#[schema_version(1)]
struct TestDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_trait_fk", fks(creature_id = "creatures" on_delete cascade))]
    pub creature_trait_fks: CreatureTraitFkTable,
}

#[test]
fn database_serde_roundtrip_compound_pk() {
    let mut db = TestDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_trait_fk(CreatureTraitFk {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let db2: TestDb = serde_json::from_str(&json).unwrap();
    assert_eq!(db2.creatures.len(), 1);
    assert_eq!(db2.creature_trait_fks.len(), 1);
    assert_eq!(
        db2.creature_trait_fks
            .get(&(CreatureId(1), TraitKind(10)))
            .unwrap()
            .value,
        42
    );
}

#[test]
fn database_serde_detects_orphaned_compound_pk_child() {
    // Manually construct JSON with a creature_trait_fk referencing a
    // non-existent creature.
    let json = r#"{
        "schema_version": 1,
        "creatures": [],
        "creature_trait_fks": [
            {"creature_id": 99, "trait_kind": 10, "value": 42}
        ]
    }"#;
    let result = serde_json::from_str::<TestDb>(json);
    match result {
        Ok(_) => panic!("expected deserialization to fail"),
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("FK") || err_msg.contains("fk") || err_msg.contains("foreign"),
                "expected FK error, got: {err_msg}"
            );
        }
    }
}
