//! Integration tests for tabulosity serde support.

#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use tabulosity::{Bounded, Database, Table};

// --- Row types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct CreatureId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Species {
    Elf,
    Capybara,
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
    #[indexed]
    pub species: Species,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct TaskId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Task {
    #[primary_key]
    pub id: TaskId,
    #[indexed]
    pub assignee: Option<CreatureId>,
    pub priority: u8,
}

#[derive(Database)]
struct TestDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "task", fks(assignee? = "creatures"))]
    pub tasks: TaskTable,
}

// --- 3-table schema with bare (non-optional) FK fields ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct FriendshipId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Friendship {
    #[primary_key]
    pub id: FriendshipId,
    #[indexed]
    pub source: CreatureId,
    #[indexed]
    pub target: CreatureId,
}

#[derive(Database)]
struct TestDb3 {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "task", fks(assignee? = "creatures"))]
    pub tasks: TaskTable,

    #[table(
        singular = "friendship",
        fks(source = "creatures", target = "creatures")
    )]
    pub friendships: FriendshipTable,
}

// --- Table-level serde ---

#[test]
fn table_roundtrip() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Capybara,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: CreatureTable = serde_json::from_str(&json).unwrap();

    // Rows preserved.
    assert_eq!(table2.len(), 2);
    assert_eq!(table2.get(&CreatureId(1)).unwrap().name, "A");
    assert_eq!(table2.get(&CreatureId(2)).unwrap().name, "B");

    // Indexes rebuilt.
    assert_eq!(table2.count_by_species(&Species::Elf), 1);
    assert_eq!(table2.count_by_species(&Species::Capybara), 1);
}

#[test]
fn table_serializes_as_vec() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Elf,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    // Should serialize as a JSON array in PK order.
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.is_array());
    let arr = val.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // PK order: id=1 first, id=2 second.
    assert_eq!(arr[0]["id"], 1);
    assert_eq!(arr[1]["id"], 2);
}

#[test]
fn table_deserialize_duplicate_pk() {
    // Manually construct JSON with duplicate PKs.
    let json = r#"[
        {"id": 1, "name": "A", "species": "Elf"},
        {"id": 1, "name": "B", "species": "Elf"}
    ]"#;

    let result: Result<CreatureTable, _> = serde_json::from_str(json);
    // Should fail because of duplicate PK.
    assert!(result.is_err());
}

// --- Database-level serde ---

#[test]
fn database_roundtrip() {
    let mut db = TestDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Aelindra".into(),
        species: Species::Elf,
    })
    .unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let db2: TestDb = serde_json::from_str(&json).unwrap();

    assert_eq!(db2.creatures.len(), 1);
    assert_eq!(db2.tasks.len(), 1);
    assert_eq!(db2.creatures.get(&CreatureId(1)).unwrap().name, "Aelindra");

    // Indexes rebuilt after deserialization.
    assert_eq!(db2.creatures.count_by_species(&Species::Elf), 1);
    assert_eq!(db2.tasks.count_by_assignee(&Some(CreatureId(1))), 1);
}

#[test]
fn database_empty_roundtrip() {
    let db = TestDb::new();
    let json = serde_json::to_string(&db).unwrap();
    let db2: TestDb = serde_json::from_str(&json).unwrap();
    assert!(db2.creatures.is_empty());
    assert!(db2.tasks.is_empty());
}

#[test]
fn database_deserialize_broken_fk() {
    // Task references a creature that doesn't exist.
    let json = r#"{
        "creatures": [
            {"id": 1, "name": "Aelindra", "species": "Elf"}
        ],
        "tasks": [
            {"id": 1, "assignee": 999, "priority": 5}
        ]
    }"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject broken FK on deserialize"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("FK target not found"),
        "error should mention FK violation: {}",
        err_msg,
    );
}

#[test]
fn database_deserialize_collects_all_errors() {
    // Duplicate creature PK AND a broken task FK — both should be reported.
    let json = r#"{
        "creatures": [
            {"id": 1, "name": "Aelindra", "species": "Elf"},
            {"id": 1, "name": "Duplicate", "species": "Capybara"}
        ],
        "tasks": [
            {"id": 1, "assignee": 999, "priority": 5}
        ]
    }"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject invalid data"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("duplicate key"),
        "error should mention dup PK: {}",
        err_msg,
    );
    assert!(
        err_msg.contains("FK target not found"),
        "error should mention FK violation: {}",
        err_msg,
    );
}

// --- Database deserialization edge cases ---

#[test]
fn database_deserialize_field_order_independent() {
    // Tasks appear before creatures in JSON — FK validation is deferred until
    // all tables are built, so field order should not matter.
    let json = r#"{
        "tasks": [{"id": 1, "assignee": 1, "priority": 5}],
        "creatures": [{"id": 1, "name": "A", "species": "Elf"}]
    }"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.tasks.len(), 1);
}

#[test]
fn database_deserialize_missing_field() {
    // Omit the "tasks" field entirely.
    let json = r#"{"creatures": [{"id": 1, "name": "A", "species": "Elf"}]}"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject missing field"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("tasks"),
        "error should mention the missing field: {}",
        err_msg,
    );
}

#[test]
fn database_deserialize_optional_fk_none_valid() {
    // Null FK value should pass validation even with no creatures.
    let json = r#"{
        "creatures": [],
        "tasks": [{"id": 1, "assignee": null, "priority": 5}]
    }"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert_eq!(db.tasks.len(), 1);
    assert!(db.tasks.get(&TaskId(1)).unwrap().assignee.is_none());
}

#[test]
fn database_deserialize_ignores_extra_fields() {
    // Unknown fields should be silently skipped (forward-compatibility).
    let json = r#"{
        "creatures": [],
        "tasks": [],
        "unknown_future_table": [1, 2, 3]
    }"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert!(db.creatures.is_empty());
    assert!(db.tasks.is_empty());
}

#[test]
fn database_deserialize_multiple_fk_violations_same_table() {
    // Multiple rows in one table each with broken FKs — all collected.
    let json = r#"{
        "creatures": [{"id": 1, "name": "A", "species": "Elf"}],
        "tasks": [
            {"id": 1, "assignee": 888, "priority": 5},
            {"id": 2, "assignee": 999, "priority": 3}
        ]
    }"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject broken FKs"),
        Err(e) => e.to_string(),
    };
    // Both broken FKs should be reported.
    assert!(
        err_msg.contains("888"),
        "should report first broken FK: {}",
        err_msg,
    );
    assert!(
        err_msg.contains("999"),
        "should report second broken FK: {}",
        err_msg,
    );
}

#[test]
fn database_deserialize_duplicate_keys_in_multiple_tables() {
    // Dup PKs in both tables — errors from all tables collected.
    let json = r#"{
        "creatures": [
            {"id": 1, "name": "A", "species": "Elf"},
            {"id": 1, "name": "B", "species": "Elf"}
        ],
        "tasks": [
            {"id": 1, "assignee": null, "priority": 5},
            {"id": 1, "assignee": null, "priority": 3}
        ]
    }"#;

    let result: Result<TestDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject duplicate keys"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("creatures"),
        "should report creature dup: {}",
        err_msg,
    );
    assert!(
        err_msg.contains("tasks"),
        "should report task dup: {}",
        err_msg,
    );
}

#[test]
fn database_deserialize_indexes_multi_row() {
    // Multiple rows per table — verify secondary indexes are correctly built.
    let json = r#"{
        "creatures": [
            {"id": 1, "name": "A", "species": "Elf"},
            {"id": 2, "name": "B", "species": "Capybara"},
            {"id": 3, "name": "C", "species": "Elf"}
        ],
        "tasks": [
            {"id": 1, "assignee": 1, "priority": 5},
            {"id": 2, "assignee": null, "priority": 3},
            {"id": 3, "assignee": 1, "priority": 1}
        ]
    }"#;

    let db: TestDb = serde_json::from_str(json).unwrap();

    // Creature indexes.
    assert_eq!(db.creatures.count_by_species(&Species::Elf), 2);
    assert_eq!(db.creatures.count_by_species(&Species::Capybara), 1);

    // Task indexes.
    assert_eq!(db.tasks.count_by_assignee(&Some(CreatureId(1))), 2);
    assert_eq!(db.tasks.count_by_assignee(&None), 1);

    // Verify actual row data from index query returns correct PK order.
    let elves = db.creatures.by_species(&Species::Elf);
    assert_eq!(elves.len(), 2);
    assert_eq!(elves[0].id, CreatureId(1));
    assert_eq!(elves[1].id, CreatureId(3));
}

#[test]
fn database_deserialize_mixed_empty_and_populated() {
    // One table populated, one empty.
    let json = r#"{
        "creatures": [
            {"id": 1, "name": "A", "species": "Elf"},
            {"id": 2, "name": "B", "species": "Capybara"}
        ],
        "tasks": []
    }"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert_eq!(db.creatures.len(), 2);
    assert!(db.tasks.is_empty());
    assert_eq!(db.creatures.count_by_species(&Species::Elf), 1);
}

// --- 3-table schema tests (bare FK fields) ---

#[test]
fn database_deserialize_bare_fk_validation() {
    // Bare (non-optional) FK with invalid target should fail.
    let json = r#"{
        "creatures": [{"id": 1, "name": "A", "species": "Elf"}],
        "tasks": [],
        "friendships": [{"id": 1, "source": 99, "target": 1}]
    }"#;

    let result: Result<TestDb3, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject broken bare FK"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("FK target not found"),
        "error should mention FK violation: {}",
        err_msg,
    );
    assert!(
        err_msg.contains("source"),
        "error should mention the FK field: {}",
        err_msg,
    );
}

#[test]
fn database_deserialize_fk_violations_across_tables() {
    // FK violations in both tasks and friendships — all collected.
    let json = r#"{
        "creatures": [{"id": 1, "name": "A", "species": "Elf"}],
        "tasks": [{"id": 1, "assignee": 888, "priority": 5}],
        "friendships": [{"id": 1, "source": 1, "target": 999}]
    }"#;

    let result: Result<TestDb3, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject broken FKs"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("tasks"),
        "task FK violation should be reported: {}",
        err_msg,
    );
    assert!(
        err_msg.contains("friendships"),
        "friendship FK violation should be reported: {}",
        err_msg,
    );
}
