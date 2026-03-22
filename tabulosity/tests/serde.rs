//! Integration tests for tabulosity serde support.

#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use tabulosity::{Bounded, Database, Error, MatchAll, QueryOpts, Table};

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
    assert_eq!(table2.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(
        table2.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
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
    // Should fail because of duplicate PK, and the error message should say so.
    let err_msg = match result {
        Ok(_) => panic!("should reject duplicate PK"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("duplicate"),
        "error should mention 'duplicate': {}",
        err_msg,
    );
}

#[test]
fn empty_table_serializes_as_empty_array() {
    let table = CreatureTable::new();
    let json = serde_json::to_string(&table).unwrap();
    assert_eq!(json, "[]");
}

// --- Backward-compat: plain table accepts old auto-PK format ---

#[test]
fn plain_table_deserializes_old_auto_format() {
    // Old auto-PK tables serialized as {"next_id": N, "rows": [...]}.
    // After converting to a natural/compound PK, the plain table deserializer
    // should still accept this format (extracting rows, ignoring next_id).
    let json = r#"{"next_id": 5, "rows": [
        {"id": 1, "name": "A", "species": "Elf"},
        {"id": 2, "name": "B", "species": "Capybara"}
    ]}"#;

    let table: CreatureTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 2);
    assert_eq!(table.get(&CreatureId(1)).unwrap().name, "A");
    assert_eq!(table.get(&CreatureId(2)).unwrap().name, "B");
    // Index should be rebuilt.
    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
}

#[test]
fn plain_table_old_format_ignores_extra_fields() {
    // Old format may have fields like "next_id" that should be silently ignored.
    let json = r#"{"next_id": 99, "extra": "stuff", "rows": [
        {"id": 1, "name": "A", "species": "Elf"}
    ]}"#;

    let table: CreatureTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 1);
}

#[test]
fn plain_table_old_format_empty_rows() {
    let json = r#"{"next_id": 0, "rows": []}"#;
    let table: CreatureTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 0);
}

#[test]
fn plain_table_old_format_missing_rows_defaults_to_empty() {
    // If the old format somehow has no "rows" key, default to empty.
    let json = r#"{"next_id": 0}"#;
    let table: CreatureTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 0);
}

// --- Serde roundtrip verifies PK order ---

#[test]
fn table_roundtrip_preserves_pk_order() {
    let mut table = CreatureTable::new();
    // Insert out of PK order: 3, 1, 2
    table
        .insert_no_fk(Creature {
            id: CreatureId(3),
            name: "C".into(),
            species: Species::Elf,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Capybara,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Elf,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: CreatureTable = serde_json::from_str(&json).unwrap();

    // all() should return rows in PK order.
    let all = table2.all();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, CreatureId(1));
    assert_eq!(all[1].id, CreatureId(2));
    assert_eq!(all[2].id, CreatureId(3));

    // keys() should return keys in PK order.
    let keys = table2.keys();
    assert_eq!(keys, vec![CreatureId(1), CreatureId(2), CreatureId(3)]);

    // Indexes should be correct after deserialization.
    assert_eq!(table2.count_by_species(&Species::Elf, QueryOpts::ASC), 2);
    assert_eq!(
        table2.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
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
    assert_eq!(
        db2.creatures
            .count_by_species(&Species::Elf, QueryOpts::ASC),
        1
    );
    assert_eq!(
        db2.tasks
            .count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
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
fn database_deserialize_missing_table_defaults_to_empty() {
    // Omit the "tasks" field entirely — should succeed with an empty tasks table.
    let json = r#"{"creatures": [{"id": 1, "name": "A", "species": "Elf"}]}"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert!(db.tasks.is_empty());
}

#[test]
fn database_deserialize_all_tables_missing_gives_empty_db() {
    // Completely empty JSON object — all tables default to empty.
    let json = r#"{}"#;

    let db: TestDb = serde_json::from_str(json).unwrap();
    assert!(db.creatures.is_empty());
    assert!(db.tasks.is_empty());
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
    assert_eq!(
        db.creatures.count_by_species(&Species::Elf, QueryOpts::ASC),
        2
    );
    assert_eq!(
        db.creatures
            .count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );

    // Task indexes.
    assert_eq!(
        db.tasks
            .count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        2
    );
    assert_eq!(db.tasks.count_by_assignee(&None, QueryOpts::ASC), 1);

    // Verify actual row data from index query returns correct PK order.
    let elves = db.creatures.by_species(&Species::Elf, QueryOpts::ASC);
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
    assert_eq!(
        db.creatures.count_by_species(&Species::Elf, QueryOpts::ASC),
        1
    );
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

// --- Compound index serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct CompTaskId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum TaskStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(name = "assignee_priority", fields("assignee", "priority"))]
struct CompTask {
    #[primary_key]
    pub id: CompTaskId,
    #[indexed]
    pub assignee: Option<CreatureId>,
    pub priority: u8,
    pub status: TaskStatus,
}

#[test]
fn compound_index_serde_roundtrip() {
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(2),
            assignee: Some(CreatureId(1)),
            priority: 3,
            status: TaskStatus::InProgress,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(2)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: CompTaskTable = serde_json::from_str(&json).unwrap();

    // Rows preserved.
    assert_eq!(table2.len(), 3);

    // Simple index rebuilt.
    assert_eq!(
        table2.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        2
    );

    // Compound index rebuilt.
    assert_eq!(
        table2.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table2.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        2
    );
    assert_eq!(
        table2.count_by_assignee_priority(&Some(CreatureId(2)), &5u8, QueryOpts::ASC),
        1
    );

    // Range query on compound index works after deserialization.
    let result = table2.by_assignee_priority(&Some(CreatureId(1)), 3u8..=5, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
}

// --- Filtered index serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct FiltTaskId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(
    name = "active_assignee",
    fields("assignee"),
    filter = "FiltTask::is_active"
)]
struct FiltTask {
    #[primary_key]
    pub id: FiltTaskId,
    #[indexed]
    pub assignee: Option<CreatureId>,
    pub priority: u8,
    pub status: TaskStatus,
}

impl FiltTask {
    fn is_active(&self) -> bool {
        matches!(self.status, TaskStatus::Pending | TaskStatus::InProgress)
    }
}

#[test]
fn filtered_index_serde_roundtrip() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending, // active
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(1)),
            priority: 3,
            status: TaskStatus::Done, // inactive
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(3),
            assignee: Some(CreatureId(2)),
            priority: 1,
            status: TaskStatus::InProgress, // active
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: FiltTaskTable = serde_json::from_str(&json).unwrap();

    // All rows preserved.
    assert_eq!(table2.len(), 3);

    // Simple index includes all rows.
    assert_eq!(
        table2.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        2
    );

    // Filtered index only includes active rows after rebuild.
    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(2)), QueryOpts::ASC),
        1
    );
    assert_eq!(table2.count_by_active_assignee(MatchAll, QueryOpts::ASC), 2);
}

#[test]
fn filtered_index_serde_preserves_mutation_behavior() {
    // After deserializing, subsequent mutations must correctly maintain
    // the filtered index.
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: FiltTaskTable = serde_json::from_str(&json).unwrap();

    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );

    // Insert a row that fails the filter.
    table2
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(1)),
            priority: 3,
            status: TaskStatus::Done,
        })
        .unwrap();
    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );

    // Update existing row to exit filter.
    table2
        .update_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();
    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );

    // Update to re-enter filter.
    table2
        .update_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::InProgress,
        })
        .unwrap();
    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
}

#[test]
fn serde_roundtrip_bounds_recomputed_from_current_data() {
    // Tracked bounds after deserialization reflect current data,
    // not stale pre-serialization bounds.
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 1,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(2),
            assignee: Some(CreatureId(100)),
            priority: 255,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Delete the extreme row.
    table.remove_no_fk(&CompTaskId(2)).unwrap();

    // Serialize with stale bounds, deserialize.
    let json = serde_json::to_string(&table).unwrap();
    let mut table2: CompTaskTable = serde_json::from_str(&json).unwrap();

    // Bounds should be tight after rebuild during deserialization.
    // Insert a new row and verify queries still work.
    table2
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table2.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        2
    );
    assert_eq!(
        table2.count_by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC),
        2
    );
}

// --- Cascade/nullify schema serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct ProjectId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Project {
    #[primary_key]
    pub id: ProjectId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct JobId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Job {
    #[primary_key]
    pub id: JobId,
    #[indexed]
    pub project: ProjectId,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct WatcherId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Watcher {
    #[primary_key]
    pub id: WatcherId,
    #[indexed]
    pub project: Option<ProjectId>,
    pub name: String,
}

#[derive(Database)]
struct CascadeDb {
    #[table(singular = "project")]
    pub projects: ProjectTable,

    #[table(singular = "job", fks(project = "projects" on_delete cascade))]
    pub jobs: JobTable,

    #[table(singular = "watcher", fks(project? = "projects" on_delete nullify))]
    pub watchers: WatcherTable,
}

#[test]
fn cascade_nullify_db_serde_roundtrip() {
    let mut db = CascadeDb::new();
    db.insert_project(Project {
        id: ProjectId(1),
        name: "Alpha".into(),
    })
    .unwrap();
    db.insert_job(Job {
        id: JobId(1),
        project: ProjectId(1),
        label: "build".into(),
    })
    .unwrap();
    db.insert_watcher(Watcher {
        id: WatcherId(1),
        project: Some(ProjectId(1)),
        name: "Alice".into(),
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let mut db2: CascadeDb = serde_json::from_str(&json).unwrap();

    assert_eq!(db2.projects.len(), 1);
    assert_eq!(db2.jobs.len(), 1);
    assert_eq!(db2.watchers.len(), 1);

    // Indexes rebuilt — cascade/nullify still work after deserialization.
    db2.remove_project(&ProjectId(1)).unwrap();
    assert!(db2.projects.is_empty());
    assert!(db2.jobs.is_empty());
    assert_eq!(db2.watchers.len(), 1);
    assert_eq!(db2.watchers.get(&WatcherId(1)).unwrap().project, None);
}

#[test]
fn cascade_db_serde_fk_validation() {
    // Job references a project that doesn't exist.
    let json = r#"{
        "projects": [],
        "jobs": [{"id": 1, "project": 99, "label": "orphan"}],
        "watchers": []
    }"#;

    let result: Result<CascadeDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject broken FK"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("FK target not found"),
        "error should mention FK violation: {}",
        err_msg,
    );
}

// =============================================================================
// Auto-increment serde tests
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoItemId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AutoItem {
    #[primary_key(auto_increment)]
    pub id: AutoItemId,
    pub name: String,
}

#[test]
fn auto_table_serializes_as_struct() {
    let mut table = AutoItemTable::new();
    table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "B".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(val.is_object(), "auto table should serialize as object");
    assert_eq!(val["next_id"], 2);
    assert!(val["rows"].is_array());
    assert_eq!(val["rows"].as_array().unwrap().len(), 2);
}

#[test]
fn auto_table_roundtrip_preserves_next_id() {
    let mut table = AutoItemTable::new();
    table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "B".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: AutoItemTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 2);
    assert_eq!(table2.next_id(), AutoItemId(2));
    assert_eq!(table2.get(&AutoItemId(0)).unwrap().name, "A");
    assert_eq!(table2.get(&AutoItemId(1)).unwrap().name, "B");
}

#[test]
fn auto_table_deserialize_with_gaps_auto_insert_correct() {
    // Rows at IDs 0 and 5 — next_id serialized as 6.
    let json = r#"{"next_id": 6, "rows": [
        {"id": 0, "name": "A"},
        {"id": 5, "name": "B"}
    ]}"#;

    let mut table: AutoItemTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 2);
    assert_eq!(table.next_id(), AutoItemId(6));

    let new_id = table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "C".into(),
        })
        .unwrap();
    assert_eq!(new_id, AutoItemId(6));
}

#[test]
fn auto_table_deserialize_defensive_next_id_correction() {
    // Deserialized next_id (3) is less than max PK successor (6).
    // Should be corrected to 6.
    let json = r#"{"next_id": 3, "rows": [
        {"id": 0, "name": "A"},
        {"id": 5, "name": "B"}
    ]}"#;

    let table: AutoItemTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.next_id(), AutoItemId(6));
}

#[test]
fn auto_table_empty_roundtrip() {
    let table = AutoItemTable::new();
    let json = serde_json::to_string(&table).unwrap();
    let table2: AutoItemTable = serde_json::from_str(&json).unwrap();
    assert!(table2.is_empty());
    assert_eq!(table2.next_id(), AutoItemId(0));
}

// --- Database-level auto-increment serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoProjectId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AutoProject {
    #[primary_key(auto_increment)]
    pub id: AutoProjectId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoTaskId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AutoTask {
    #[primary_key(auto_increment)]
    pub id: AutoTaskId,
    #[indexed]
    pub project: AutoProjectId,
    pub label: String,
}

#[derive(Database)]
struct AutoDb {
    #[table(singular = "project", auto)]
    pub projects: AutoProjectTable,

    #[table(singular = "task", auto, fks(project = "projects"))]
    pub tasks: AutoTaskTable,
}

#[test]
fn auto_database_roundtrip() {
    let mut db = AutoDb::new();
    let pid = db
        .insert_project_auto(|pk| AutoProject {
            id: pk,
            name: "Alpha".into(),
        })
        .unwrap();
    db.insert_task_auto(|pk| AutoTask {
        id: pk,
        project: pid,
        label: "build".into(),
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let db2: AutoDb = serde_json::from_str(&json).unwrap();

    assert_eq!(db2.projects.len(), 1);
    assert_eq!(db2.tasks.len(), 1);
    assert_eq!(db2.projects.get(&AutoProjectId(0)).unwrap().name, "Alpha");
    assert_eq!(db2.tasks.get(&AutoTaskId(0)).unwrap().label, "build");

    // next_id preserved — auto insert gets correct next ID.
    assert_eq!(db2.projects.next_id(), AutoProjectId(1));
    assert_eq!(db2.tasks.next_id(), AutoTaskId(1));
}

#[test]
fn auto_database_roundtrip_then_auto_insert() {
    let mut db = AutoDb::new();
    db.insert_project_auto(|pk| AutoProject {
        id: pk,
        name: "A".into(),
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let mut db2: AutoDb = serde_json::from_str(&json).unwrap();

    // Auto insert after deserialization should work correctly.
    let pid = db2
        .insert_project_auto(|pk| AutoProject {
            id: pk,
            name: "B".into(),
        })
        .unwrap();
    assert_eq!(pid, AutoProjectId(1));
}

#[test]
fn auto_database_empty_roundtrip() {
    let db = AutoDb::new();
    let json = serde_json::to_string(&db).unwrap();
    let db2: AutoDb = serde_json::from_str(&json).unwrap();
    assert!(db2.projects.is_empty());
    assert!(db2.tasks.is_empty());
    assert_eq!(db2.projects.next_id(), AutoProjectId(0));
    assert_eq!(db2.tasks.next_id(), AutoTaskId(0));
}

// =============================================================================
// Auto-increment serde edge cases
// =============================================================================

#[test]
fn auto_table_deserialize_duplicate_pk() {
    // Auto table JSON with duplicate PKs in the rows array.
    let json = r#"{"next_id": 3, "rows": [
        {"id": 0, "name": "A"},
        {"id": 0, "name": "B"}
    ]}"#;

    let result: Result<AutoItemTable, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject duplicate PK"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("duplicate"),
        "error should mention 'duplicate': {}",
        err_msg,
    );
}

#[test]
fn auto_table_deserialize_missing_next_id() {
    let json = r#"{"rows": [{"id": 0, "name": "A"}]}"#;

    let result: Result<AutoItemTable, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject missing next_id"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("next_id"),
        "error should mention 'next_id': {}",
        err_msg,
    );
}

#[test]
fn auto_table_deserialize_missing_rows() {
    let json = r#"{"next_id": 5}"#;

    let result: Result<AutoItemTable, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject missing rows"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("rows"),
        "error should mention 'rows': {}",
        err_msg,
    );
}

#[test]
fn auto_table_deserialize_ignores_extra_fields() {
    // Extra unknown fields should be silently skipped.
    let json = r#"{"next_id": 2, "rows": [{"id": 0, "name": "A"}], "extra": "ignored"}"#;

    let table: AutoItemTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 1);
    assert_eq!(table.next_id(), AutoItemId(2));
}

#[test]
fn auto_table_roundtrip_after_remove() {
    // Remove a row then serialize — next_id should persist the high-water mark.
    let mut table = AutoItemTable::new();
    table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "B".into(),
        })
        .unwrap();
    table.remove_no_fk(&AutoItemId(0)).unwrap();

    // next_id should be 2, one row with id=1.
    let json = serde_json::to_string(&table).unwrap();
    let table2: AutoItemTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 1);
    assert_eq!(table2.next_id(), AutoItemId(2));

    // Auto-insert continues from 2.
    let mut table2 = table2;
    let new_id = table2
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "C".into(),
        })
        .unwrap();
    assert_eq!(new_id, AutoItemId(2));
}

// --- Auto-increment + compound index serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoEntryId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum EntryStatus {
    Active,
    Archived,
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(name = "assignee_status", fields("assignee", "status"))]
struct AutoEntry {
    #[primary_key(auto_increment)]
    pub id: AutoEntryId,
    #[indexed]
    pub assignee: Option<CreatureId>,
    pub status: EntryStatus,
    pub label: String,
}

#[test]
fn auto_compound_index_serde_roundtrip() {
    let mut table = AutoEntryTable::new();
    table
        .insert_auto_no_fk(|pk| AutoEntry {
            id: pk,
            assignee: Some(CreatureId(1)),
            status: EntryStatus::Active,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| AutoEntry {
            id: pk,
            assignee: Some(CreatureId(1)),
            status: EntryStatus::Archived,
            label: "B".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| AutoEntry {
            id: pk,
            assignee: Some(CreatureId(2)),
            status: EntryStatus::Active,
            label: "C".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: AutoEntryTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 3);
    assert_eq!(table2.next_id(), AutoEntryId(3));

    // Simple index rebuilt.
    assert_eq!(
        table2.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        2
    );

    // Compound index rebuilt.
    assert_eq!(
        table2.count_by_assignee_status(&Some(CreatureId(1)), &EntryStatus::Active, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table2.count_by_assignee_status(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        2
    );
}

// --- Auto-increment + filtered index serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoFiltId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(
    name = "active_assignee",
    fields("assignee"),
    filter = "AutoFiltEntry::is_active"
)]
struct AutoFiltEntry {
    #[primary_key(auto_increment)]
    pub id: AutoFiltId,
    #[indexed]
    pub assignee: Option<CreatureId>,
    pub status: EntryStatus,
    pub label: String,
}

impl AutoFiltEntry {
    fn is_active(&self) -> bool {
        matches!(self.status, EntryStatus::Active)
    }
}

#[test]
fn auto_filtered_index_serde_roundtrip() {
    let mut table = AutoFiltEntryTable::new();
    table
        .insert_auto_no_fk(|pk| AutoFiltEntry {
            id: pk,
            assignee: Some(CreatureId(1)),
            status: EntryStatus::Active,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| AutoFiltEntry {
            id: pk,
            assignee: Some(CreatureId(1)),
            status: EntryStatus::Archived,
            label: "B".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: AutoFiltEntryTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 2);
    assert_eq!(table2.next_id(), AutoFiltId(2));

    // Simple index includes all.
    assert_eq!(
        table2.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        2
    );

    // Filtered index only includes active.
    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
}

#[test]
fn auto_filtered_index_serde_preserves_mutation_behavior() {
    let mut table = AutoFiltEntryTable::new();
    table
        .insert_auto_no_fk(|pk| AutoFiltEntry {
            id: pk,
            assignee: Some(CreatureId(1)),
            status: EntryStatus::Active,
            label: "A".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: AutoFiltEntryTable = serde_json::from_str(&json).unwrap();

    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );

    // Insert an archived entry — should not appear in filtered index.
    table2
        .insert_auto_no_fk(|pk| AutoFiltEntry {
            id: pk,
            assignee: Some(CreatureId(1)),
            status: EntryStatus::Archived,
            label: "B".into(),
        })
        .unwrap();
    assert_eq!(
        table2.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
    assert_eq!(
        table2.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        2
    );
    assert_eq!(table2.next_id(), AutoFiltId(2));
}

// --- Database-level auto-increment serde with broken FKs ---

#[test]
fn auto_database_deserialize_broken_fk() {
    // Auto task references a non-existent project.
    let json = r#"{
        "projects": {"next_id": 1, "rows": [{"id": 0, "name": "Alpha"}]},
        "tasks": {"next_id": 1, "rows": [{"id": 0, "project": 99, "label": "orphan"}]}
    }"#;

    let result: Result<AutoDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject broken FK in auto DB"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("FK target not found"),
        "error should mention FK violation: {}",
        err_msg,
    );
}

#[test]
fn auto_database_deserialize_duplicate_pk_in_auto_table() {
    // Duplicate PK in an auto table embedded in a database.
    let json = r#"{
        "projects": {"next_id": 2, "rows": [
            {"id": 0, "name": "A"},
            {"id": 0, "name": "B"}
        ]},
        "tasks": {"next_id": 0, "rows": []}
    }"#;

    let result: Result<AutoDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject duplicate PK in auto DB table"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("duplicate"),
        "error should mention 'duplicate': {}",
        err_msg,
    );
}

// --- Mixed auto and non-auto tables in a database serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct ManualId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ManualRow {
    #[primary_key]
    pub id: ManualId,
    pub value: String,
}

#[derive(Database)]
struct MixedAutoDb {
    #[table(singular = "manual_row")]
    pub manual_rows: ManualRowTable,

    #[table(singular = "project", auto)]
    pub projects: AutoProjectTable,
}

#[test]
fn mixed_auto_non_auto_db_serde_roundtrip() {
    let mut db = MixedAutoDb::new();
    db.insert_manual_row(ManualRow {
        id: ManualId(42),
        value: "hello".into(),
    })
    .unwrap();
    let pid = db
        .insert_project_auto(|pk| AutoProject {
            id: pk,
            name: "Alpha".into(),
        })
        .unwrap();
    assert_eq!(pid, AutoProjectId(0));

    let json = serde_json::to_string(&db).unwrap();
    let db2: MixedAutoDb = serde_json::from_str(&json).unwrap();

    assert_eq!(db2.manual_rows.len(), 1);
    assert_eq!(db2.manual_rows.get(&ManualId(42)).unwrap().value, "hello");
    assert_eq!(db2.projects.len(), 1);
    assert_eq!(db2.projects.next_id(), AutoProjectId(1));
}

#[test]
fn mixed_auto_non_auto_db_serialization_format() {
    let mut db = MixedAutoDb::new();
    db.insert_manual_row(ManualRow {
        id: ManualId(1),
        value: "hi".into(),
    })
    .unwrap();
    db.insert_project_auto(|pk| AutoProject {
        id: pk,
        name: "A".into(),
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Manual table serializes as array.
    assert!(val["manual_rows"].is_array());
    // Auto table serializes as object with next_id and rows.
    assert!(val["projects"].is_object());
    assert!(val["projects"]["next_id"].is_number());
    assert!(val["projects"]["rows"].is_array());
}

// --- Auto-increment + cascade/nullify serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoCatId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AutoCat {
    #[primary_key(auto_increment)]
    pub id: AutoCatId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoArticleId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AutoArticle {
    #[primary_key(auto_increment)]
    pub id: AutoArticleId,
    #[indexed]
    pub category: AutoCatId,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoTagId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AutoTag {
    #[primary_key(auto_increment)]
    pub id: AutoTagId,
    #[indexed]
    pub category: Option<AutoCatId>,
    pub name: String,
}

#[derive(Database)]
struct CascadeAutoDb {
    #[table(singular = "category", auto)]
    pub categories: AutoCatTable,

    #[table(singular = "article", auto, fks(category = "categories" on_delete cascade))]
    pub articles: AutoArticleTable,

    #[table(singular = "tag", auto, fks(category? = "categories" on_delete nullify))]
    pub tags: AutoTagTable,
}

#[test]
fn cascade_auto_db_serde_roundtrip() {
    let mut db = CascadeAutoDb::new();
    let cat = db
        .insert_category_auto(|pk| AutoCat {
            id: pk,
            name: "Tech".into(),
        })
        .unwrap();
    db.insert_article_auto(|pk| AutoArticle {
        id: pk,
        category: cat,
        title: "Rust".into(),
    })
    .unwrap();
    db.insert_tag_auto(|pk| AutoTag {
        id: pk,
        category: Some(cat),
        name: "rust".into(),
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let mut db2: CascadeAutoDb = serde_json::from_str(&json).unwrap();

    assert_eq!(db2.categories.len(), 1);
    assert_eq!(db2.articles.len(), 1);
    assert_eq!(db2.tags.len(), 1);

    // Indexes rebuilt — cascade/nullify still work after deserialization.
    db2.remove_category(&cat).unwrap();
    assert!(db2.categories.is_empty());
    assert!(db2.articles.is_empty());
    assert_eq!(db2.tags.len(), 1);
    assert_eq!(db2.tags.get(&AutoTagId(0)).unwrap().category, None);

    // next_id preserved after roundtrip + cascade.
    assert_eq!(db2.categories.next_id(), AutoCatId(1));
    assert_eq!(db2.articles.next_id(), AutoArticleId(1));
    assert_eq!(db2.tags.next_id(), AutoTagId(1));
}

#[test]
fn cascade_auto_db_serde_fk_validation() {
    // Article references a category that doesn't exist.
    let json = r#"{
        "categories": {"next_id": 0, "rows": []},
        "articles": {"next_id": 1, "rows": [{"id": 0, "category": 99, "title": "orphan"}]},
        "tags": {"next_id": 0, "rows": []}
    }"#;

    let result: Result<CascadeAutoDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject broken FK in cascade auto DB"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("FK target not found"),
        "error should mention FK violation: {}",
        err_msg,
    );
}

#[test]
fn auto_database_deserialize_field_order_independent() {
    // Tasks appear before projects in JSON — should still work.
    let json = r#"{
        "tasks": {"next_id": 1, "rows": [{"id": 0, "project": 0, "label": "build"}]},
        "projects": {"next_id": 1, "rows": [{"id": 0, "name": "Alpha"}]}
    }"#;

    let db: AutoDb = serde_json::from_str(&json).unwrap();
    assert_eq!(db.projects.len(), 1);
    assert_eq!(db.tasks.len(), 1);
    assert_eq!(db.projects.next_id(), AutoProjectId(1));
    assert_eq!(db.tasks.next_id(), AutoTaskId(1));
}

// =============================================================================
// Unique index serde tests
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct UserId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct User {
    #[primary_key]
    pub id: UserId,
    #[indexed(unique)]
    pub email: String,
    pub name: String,
}

#[test]
fn unique_index_table_serde_roundtrip() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: UserTable = serde_json::from_str(&json).unwrap();

    // Rows preserved.
    assert_eq!(table2.len(), 2);
    assert_eq!(table2.get(&UserId(1)).unwrap().email, "a@b.com");
    assert_eq!(table2.get(&UserId(2)).unwrap().email, "c@d.com");

    // Unique index rebuilt — queries work.
    assert_eq!(
        table2
            .by_email(&"a@b.com".to_string(), QueryOpts::ASC)
            .len(),
        1
    );
    assert_eq!(
        table2.count_by_email(&"a@b.com".to_string(), QueryOpts::ASC),
        1
    );
}

#[test]
fn unique_index_enforced_after_deserialization() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: UserTable = serde_json::from_str(&json).unwrap();

    // Unique constraint should be enforced after deserialization.
    let err = table2
        .insert_no_fk(User {
            id: UserId(2),
            email: "a@b.com".into(),
            name: "Bob".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

#[test]
fn unique_index_update_works_after_deserialization() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: UserTable = serde_json::from_str(&json).unwrap();

    // Update to a new unique value — should work.
    table2
        .update_no_fk(User {
            id: UserId(1),
            email: "new@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(table2.get_ref(&UserId(1)).unwrap().email, "new@b.com");

    // Update to conflict — should fail.
    let err = table2
        .update_no_fk(User {
            id: UserId(1),
            email: "c@d.com".into(),
            name: "Alice".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

// --- Compound unique index serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct SlotId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(name = "building_slot", fields("building", "slot"), unique)]
struct Assignment {
    #[primary_key]
    pub id: SlotId,
    pub building: u32,
    pub slot: u32,
    pub elf_name: String,
}

#[test]
fn compound_unique_index_serde_roundtrip() {
    let mut table = AssignmentTable::new();
    table
        .insert_no_fk(Assignment {
            id: SlotId(1),
            building: 1,
            slot: 1,
            elf_name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(Assignment {
            id: SlotId(2),
            building: 1,
            slot: 2,
            elf_name: "Bob".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: AssignmentTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 2);

    // Compound unique constraint enforced after deserialization.
    let err = table2
        .insert_no_fk(Assignment {
            id: SlotId(3),
            building: 1,
            slot: 1,
            elf_name: "Carol".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Different compound key — succeeds.
    table2
        .insert_no_fk(Assignment {
            id: SlotId(4),
            building: 2,
            slot: 1,
            elf_name: "Dave".into(),
        })
        .unwrap();
    assert_eq!(table2.len(), 3);
}

// --- Filtered unique index serde ---

fn is_active_reg(r: &SerdeRegistration) -> bool {
    r.active
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct SerdeRegId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(
    name = "active_email",
    fields("email"),
    filter = "is_active_reg",
    unique
)]
struct SerdeRegistration {
    #[primary_key]
    pub id: SerdeRegId,
    pub email: String,
    pub active: bool,
}

#[test]
fn filtered_unique_index_serde_roundtrip() {
    let mut table = SerdeRegistrationTable::new();
    table
        .insert_no_fk(SerdeRegistration {
            id: SerdeRegId(1),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap();
    // Same email, inactive — allowed.
    table
        .insert_no_fk(SerdeRegistration {
            id: SerdeRegId(2),
            email: "a@b.com".into(),
            active: false,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: SerdeRegistrationTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 2);

    // Filtered unique constraint enforced after deserialization.
    let err = table2
        .insert_no_fk(SerdeRegistration {
            id: SerdeRegId(3),
            email: "a@b.com".into(),
            active: true,
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Inactive with same email — still allowed.
    table2
        .insert_no_fk(SerdeRegistration {
            id: SerdeRegId(4),
            email: "a@b.com".into(),
            active: false,
        })
        .unwrap();
    assert_eq!(table2.len(), 3);
}

// --- Deserialization with unique violations in data ---

#[test]
fn deserialize_table_with_duplicate_unique_values() {
    // Hand-craft JSON with duplicate unique values.
    let json = r#"[
        {"id": 1, "email": "a@b.com", "name": "Alice"},
        {"id": 2, "email": "a@b.com", "name": "Bob"}
    ]"#;

    // Deserialization currently bypasses unique checks (inserts directly into
    // BTreeMap then calls manual_rebuild_all_indexes). The rebuild reconstructs the index
    // from data, so both entries will be in the index. But subsequent inserts
    // with the same value should still be rejected because the index has entries.
    // This is not ideal — ideally deserialization would reject duplicates — but
    // this test documents the current behavior.
    let table: UserTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 2);

    // The index has both entries (since manual_rebuild_all_indexes just inserts all).
    // A query for that email should return 2 (the index is not truly "unique"
    // at this point since we bypassed the check).
    assert_eq!(
        table.by_email(&"a@b.com".to_string(), QueryOpts::ASC).len(),
        2
    );
}

// --- Auto-increment + unique index serde ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded, Serialize, Deserialize)]
struct AutoUserId(u32);

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct AutoUser {
    #[primary_key(auto_increment)]
    pub id: AutoUserId,
    #[indexed(unique)]
    pub email: String,
    pub name: String,
}

#[test]
fn auto_unique_index_serde_roundtrip() {
    let mut table = AutoUserTable::new();
    table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: AutoUserTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 2);
    assert_eq!(table2.next_id(), AutoUserId(2));

    // Unique constraint enforced after deserialization.
    let err = table2
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "a@b.com".into(),
            name: "Charlie".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));

    // Different email — succeeds.
    let id = table2
        .insert_auto_no_fk(|pk| AutoUser {
            id: pk,
            email: "e@f.com".into(),
            name: "Dave".into(),
        })
        .unwrap();
    assert_eq!(id, AutoUserId(2));
    assert_eq!(table2.len(), 3);
}

// =============================================================================
// Gap C10: Unique index enforcement post-deserialize
// =============================================================================

#[test]
fn unique_index_enforced_after_deserialize() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_no_fk(User {
            id: UserId(2),
            email: "c@d.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: UserTable = serde_json::from_str(&json).unwrap();

    // Insert with duplicate unique value should fail after deserialization.
    let err = table2
        .insert_no_fk(User {
            id: UserId(3),
            email: "a@b.com".into(),
            name: "Charlie".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

// =============================================================================
// Gap I12: Serde roundtrip after modify_unchecked mutations
// =============================================================================

#[test]
fn serde_roundtrip_after_modify_unchecked() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "Thorn".into(),
            species: Species::Capybara,
        })
        .unwrap();

    // Modify non-indexed field.
    table
        .modify_unchecked(&CreatureId(1), |c| {
            c.name = "Modified".into();
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let table2: CreatureTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.get(&CreatureId(1)).unwrap().name, "Modified");
    assert_eq!(table2.get(&CreatureId(2)).unwrap().name, "Thorn");
    // Indexes rebuilt correctly.
    assert_eq!(table2.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(
        table2.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
}

// =============================================================================
// Gap I13: Auto-increment serde: next_id > max_pk preserved
// =============================================================================

#[test]
fn auto_increment_serde_next_id_after_deletes() {
    let mut table = AutoItemTable::new();
    for _ in 0..5 {
        table
            .insert_auto_no_fk(|pk| AutoItem {
                id: pk,
                name: "x".into(),
            })
            .unwrap();
    }
    // Delete last 3.
    table.remove_no_fk(&AutoItemId(4)).unwrap();
    table.remove_no_fk(&AutoItemId(3)).unwrap();
    table.remove_no_fk(&AutoItemId(2)).unwrap();
    assert_eq!(table.len(), 2);
    assert_eq!(table.next_id(), AutoItemId(5)); // IDs 0,1 remain; next_id = 5

    let json = serde_json::to_string(&table).unwrap();
    let mut table2: AutoItemTable = serde_json::from_str(&json).unwrap();

    assert_eq!(table2.len(), 2);
    assert_eq!(table2.next_id(), AutoItemId(5));

    // Next auto insert should get ID 5, not 2.
    let id = table2
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "new".into(),
        })
        .unwrap();
    assert_eq!(id, AutoItemId(5));
}

// =============================================================================
// Gap I31: Serde roundtrip then query with DESC/offset
// =============================================================================

#[test]
fn serde_roundtrip_then_desc_offset_query() {
    let mut table = CreatureTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{i}"),
                species: Species::Elf,
            })
            .unwrap();
    }

    let json = serde_json::to_string(&table).unwrap();
    let table2: CreatureTable = serde_json::from_str(&json).unwrap();

    // DESC + offset on deserialized table.
    let result = table2.by_species(&Species::Elf, QueryOpts::DESC.with_offset(2));
    let ids: Vec<u32> = result.iter().map(|c| c.id.0).collect();
    // DESC: 5,4,3,2,1. Offset 2: 3,2,1.
    assert_eq!(ids, vec![3, 2, 1]);

    // count_by with offset.
    assert_eq!(
        table2.count_by_species(&Species::Elf, QueryOpts::DESC.with_offset(2)),
        3
    );
}

// =============================================================================
// Gap I36: Auto-increment next_id defensive correction on deserialize
// =============================================================================

#[test]
fn auto_increment_defensive_next_id_correction() {
    // Manually craft JSON with next_id < max(PKs)+1.
    let json = r#"{"next_id": 2, "rows": [
        {"id": 0, "name": "zero"},
        {"id": 5, "name": "five"}
    ]}"#;

    let mut table: AutoItemTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 2);

    // Deserializer should have corrected next_id to max(5)+1 = 6.
    assert_eq!(table.next_id(), AutoItemId(6));

    // Next auto insert should get ID 6.
    let id = table
        .insert_auto_no_fk(|pk| AutoItem {
            id: pk,
            name: "new".into(),
        })
        .unwrap();
    assert_eq!(id, AutoItemId(6));
}

// =============================================================================
// Gap I20: Multiple serde roundtrip cycles with mutations between each
// =============================================================================

#[test]
fn multiple_serde_roundtrip_cycles() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
        })
        .unwrap();

    // Cycle 1: serialize -> deserialize -> mutate.
    let json1 = serde_json::to_string(&table).unwrap();
    let mut table: CreatureTable = serde_json::from_str(&json1).unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Capybara,
        })
        .unwrap();

    // Cycle 2: serialize -> deserialize -> mutate.
    let json2 = serde_json::to_string(&table).unwrap();
    let mut table: CreatureTable = serde_json::from_str(&json2).unwrap();
    table
        .update_no_fk(Creature {
            id: CreatureId(1),
            name: "A-updated".into(),
            species: Species::Elf,
        })
        .unwrap();

    // Cycle 3: serialize -> deserialize -> verify.
    let json3 = serde_json::to_string(&table).unwrap();
    let table: CreatureTable = serde_json::from_str(&json3).unwrap();

    assert_eq!(table.len(), 2);
    assert_eq!(table.get(&CreatureId(1)).unwrap().name, "A-updated");
    assert_eq!(table.get(&CreatureId(2)).unwrap().name, "B");
    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
}

// =============================================================================
// Gap I35: Serde deserialization with duplicate unique -- documents behavior
// =============================================================================

#[test]
fn deserialize_duplicate_unique_silently_accepted() {
    // Currently, tabulosity deserialization does NOT validate unique constraints.
    // Duplicate unique values in serialized data are silently accepted because
    // manual_rebuild_all_indexes just inserts all entries. This test documents the behavior.
    let json = r#"[
        {"id": 1, "email": "dup@test.com", "name": "Alice"},
        {"id": 2, "email": "dup@test.com", "name": "Bob"},
        {"id": 3, "email": "dup@test.com", "name": "Charlie"}
    ]"#;

    let table: UserTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 3);

    // All 3 entries are in the "unique" index (constraint not enforced on deser).
    assert_eq!(
        table
            .by_email(&"dup@test.com".to_string(), QueryOpts::ASC)
            .len(),
        3
    );

    // But new inserts with that value ARE rejected (unique check works on the
    // rebuilt index).
    let mut table = table;
    let err = table
        .insert_no_fk(User {
            id: UserId(4),
            email: "dup@test.com".into(),
            name: "Dave".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateIndex { .. }));
}

// =============================================================================
// Schema versioning
// =============================================================================

#[derive(Database)]
#[schema_version(3)]
struct VersionedDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "task", fks(assignee? = "creatures"))]
    pub tasks: TaskTable,
}

#[test]
fn versioned_db_serialization_includes_version() {
    let db = VersionedDb::new();
    let json = serde_json::to_string(&db).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(val["schema_version"], 3);
}

#[test]
fn versioned_db_roundtrip() {
    let mut db = VersionedDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Aelindra".into(),
        species: Species::Elf,
    })
    .unwrap();

    let json = serde_json::to_string(&db).unwrap();
    let db2: VersionedDb = serde_json::from_str(&json).unwrap();
    assert_eq!(db2.creatures.len(), 1);
}

#[test]
fn versioned_db_rejects_wrong_version() {
    // Version 99 instead of the expected 3.
    let json = r#"{"schema_version": 99, "creatures": [], "tasks": []}"#;
    let result: Result<VersionedDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject wrong version"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("schema version mismatch"),
        "expected schema version mismatch error, got: {}",
        err_msg,
    );
}

#[test]
fn versioned_db_rejects_missing_version() {
    // No schema_version field at all.
    let json = r#"{"creatures": [], "tasks": []}"#;
    let result: Result<VersionedDb, _> = serde_json::from_str(json);
    let err_msg = match result {
        Ok(_) => panic!("should reject missing version"),
        Err(e) => e.to_string(),
    };
    assert!(
        err_msg.contains("schema_version"),
        "expected missing schema_version error, got: {}",
        err_msg,
    );
}

#[test]
fn unversioned_db_has_no_schema_version_field() {
    // TestDb has no #[schema_version] attribute — serialized form should
    // not contain a schema_version field.
    let db = TestDb::new();
    let json = serde_json::to_string(&db).unwrap();
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        val.get("schema_version").is_none(),
        "unversioned DB should not have schema_version in JSON"
    );
}

// =============================================================================
// serde(default) convention for additive schema changes
// =============================================================================

/// Row type with a new field that has a serde default. This simulates adding
/// a column to an existing table across save versions.
#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CreatureV2 {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
    #[indexed]
    pub species: Species,
    /// New field added in V2 — old saves won't have it.
    #[serde(default)]
    pub mood_score: i32,
}

#[derive(Database)]
struct TestDbV2 {
    #[table(singular = "creature")]
    pub creatures: CreatureV2Table,
}

#[test]
fn serde_default_field_on_old_data() {
    // Simulate loading data that was serialized without the `mood_score` field.
    let json = r#"{"creatures": [{"id": 1, "name": "A", "species": "Elf"}]}"#;

    let db: TestDbV2 = serde_json::from_str(json).unwrap();
    assert_eq!(db.creatures.len(), 1);
    let creature = db.creatures.get(&CreatureId(1)).unwrap();
    assert_eq!(creature.mood_score, 0); // i32::default()
}

// =============================================================================
// Hash index serde roundtrip
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct HashCreature {
    #[primary_key(auto_increment)]
    id: u32,
    #[indexed(hash)]
    species: String,
    name: String,
}

#[test]
fn hash_index_serde_roundtrip() {
    let mut table = HashCreatureTable::new();
    table
        .insert_auto_no_fk(|id| HashCreature {
            id,
            species: "elf".into(),
            name: "Aelara".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashCreature {
            id,
            species: "dwarf".into(),
            name: "Thorin".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashCreature {
            id,
            species: "elf".into(),
            name: "Legolas".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let restored: HashCreatureTable = serde_json::from_str(&json).unwrap();

    // Verify all data survived.
    assert_eq!(restored.len(), 3);
    assert_eq!(restored.get(&0).unwrap().name, "Aelara");

    // Verify hash index was rebuilt correctly.
    let elves = restored.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 2);

    let dwarves = restored.by_species(&"dwarf".to_string(), QueryOpts::ASC);
    assert_eq!(dwarves.len(), 1);
    assert_eq!(dwarves[0].name, "Thorin");
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct HashUser {
    #[primary_key(auto_increment)]
    id: u32,
    #[indexed(hash, unique)]
    email: String,
    name: String,
}

#[test]
fn unique_hash_index_serde_roundtrip() {
    let mut table = HashUserTable::new();
    table
        .insert_auto_no_fk(|id| HashUser {
            id,
            email: "alice@example.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashUser {
            id,
            email: "bob@example.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let restored: HashUserTable = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.len(), 2);

    // Unique hash index works after restore.
    let alice = restored.by_email(&"alice@example.com".to_string(), QueryOpts::ASC);
    assert_eq!(alice.len(), 1);
    assert_eq!(alice[0].name, "Alice");

    // Unique constraint still enforced after restore.
    let mut restored = restored;
    let result = restored.insert_auto_no_fk(|id| HashUser {
        id,
        email: "alice@example.com".into(),
        name: "Eve".into(),
    });
    assert!(matches!(result, Err(Error::DuplicateIndex { .. })));
}

// =============================================================================
// Hash primary storage serde roundtrip
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[table(primary_storage = "hash")]
struct HashPrimaryCreature {
    #[primary_key(auto_increment)]
    id: u32,
    #[indexed(hash)]
    species: String,
    name: String,
}

#[test]
fn hash_primary_serde_roundtrip() {
    let mut table = HashPrimaryCreatureTable::new();
    table
        .insert_auto_no_fk(|id| HashPrimaryCreature {
            id,
            species: "elf".into(),
            name: "Aelara".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashPrimaryCreature {
            id,
            species: "dwarf".into(),
            name: "Thorin".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let restored: HashPrimaryCreatureTable = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.len(), 2);
    assert_eq!(restored.get(&0).unwrap().name, "Aelara");
    assert_eq!(restored.get(&1).unwrap().name, "Thorin");

    // Hash index rebuilt correctly.
    let elves = restored.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 1);

    // Auto-increment counter preserved — next insert gets id=2.
    let mut restored = restored;
    let id = restored
        .insert_auto_no_fk(|id| HashPrimaryCreature {
            id,
            species: "human".into(),
            name: "New".into(),
        })
        .unwrap();
    assert_eq!(id, 2);
}

// =============================================================================
// Hash index insertion order preservation across serde roundtrip
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct OrderTest {
    #[primary_key]
    id: u32,
    #[indexed(hash)]
    category: String,
    name: String,
}

#[test]
fn hash_index_insertion_order_preserved_across_serde() {
    let mut table = OrderTestTable::new();

    // Insert rows in a specific non-alphabetical order for the "tool" category.
    table
        .insert_no_fk(OrderTest {
            id: 10,
            category: "tool".into(),
            name: "Hammer".into(),
        })
        .unwrap();
    table
        .insert_no_fk(OrderTest {
            id: 20,
            category: "weapon".into(),
            name: "Sword".into(),
        })
        .unwrap();
    table
        .insert_no_fk(OrderTest {
            id: 30,
            category: "tool".into(),
            name: "Saw".into(),
        })
        .unwrap();
    table
        .insert_no_fk(OrderTest {
            id: 40,
            category: "tool".into(),
            name: "Wrench".into(),
        })
        .unwrap();
    table
        .insert_no_fk(OrderTest {
            id: 50,
            category: "weapon".into(),
            name: "Bow".into(),
        })
        .unwrap();

    // Verify pre-roundtrip iteration order: MatchAll iterates hash index
    // entries in insertion order of field values, then within each group
    // by PK order (BTree primary).
    let all_before: Vec<String> = table
        .iter_by_category(MatchAll, QueryOpts::ASC)
        .map(|r| r.name.clone())
        .collect();

    // Serialize and deserialize.
    let json = serde_json::to_string(&table).unwrap();
    let restored: OrderTestTable = serde_json::from_str(&json).unwrap();

    // The critical assertion: iteration order through the hash index
    // must be identical after roundtrip.
    let all_after: Vec<String> = restored
        .iter_by_category(MatchAll, QueryOpts::ASC)
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(all_before, all_after);

    // Also verify exact-match queries still work.
    let tools = restored.by_category(&"tool".to_string(), QueryOpts::ASC);
    assert_eq!(tools.len(), 3);
    let weapons = restored.by_category(&"weapon".to_string(), QueryOpts::ASC);
    assert_eq!(weapons.len(), 2);
}

#[test]
fn hash_index_order_differs_from_row_order_after_roundtrip() {
    // This test verifies that hash index insertion order is preserved
    // independently of row storage order. We insert rows such that the
    // hash index's category insertion order (tool, weapon, armor) differs
    // from PK order (which is the rows iteration order for BTree primary).
    let mut table = OrderTestTable::new();

    // Category "weapon" is inserted first (PK 1).
    table
        .insert_no_fk(OrderTest {
            id: 1,
            category: "weapon".into(),
            name: "Sword".into(),
        })
        .unwrap();
    // Category "armor" second (PK 2).
    table
        .insert_no_fk(OrderTest {
            id: 2,
            category: "armor".into(),
            name: "Shield".into(),
        })
        .unwrap();
    // Category "tool" third (PK 3).
    table
        .insert_no_fk(OrderTest {
            id: 3,
            category: "tool".into(),
            name: "Hammer".into(),
        })
        .unwrap();

    // Hash index iteration order for MatchAll should be: weapon, armor, tool
    // (insertion order of distinct category values).
    let categories_before: Vec<String> = table
        .iter_by_category(MatchAll, QueryOpts::ASC)
        .map(|r| r.category.clone())
        .collect();
    assert_eq!(categories_before, vec!["weapon", "armor", "tool"]);

    // If we rebuilt from rows (PK order), the hash index would iterate
    // in PK order: weapon(1), armor(2), tool(3) — same in this case.
    // But let's add a second weapon with higher PK to make the point clearer.
    table
        .insert_no_fk(OrderTest {
            id: 0,
            category: "weapon".into(),
            name: "Bow".into(),
        })
        .unwrap();

    // Now rows in PK order: 0(weapon), 1(weapon), 2(armor), 3(tool)
    // Hash index insertion order: weapon(first seen at id=1), armor, tool
    // Within weapon group (BTree primary inner): PKs 0, 1

    let json = serde_json::to_string(&table).unwrap();
    let restored: OrderTestTable = serde_json::from_str(&json).unwrap();

    let categories_after: Vec<String> = restored
        .iter_by_category(MatchAll, QueryOpts::ASC)
        .map(|r| r.category.clone())
        .collect();

    // The key assertion: category iteration order is weapon, armor, tool
    // (hash index insertion order), NOT the rows' PK order.
    // After roundtrip, the hash index was deserialized directly, preserving
    // this insertion order.
    let expected_order: Vec<String> = table
        .iter_by_category(MatchAll, QueryOpts::ASC)
        .map(|r| r.category.clone())
        .collect();
    assert_eq!(categories_after, expected_order);
}

#[test]
fn hash_index_backward_compat_missing_idx_field() {
    // Simulate loading old-format data (no hash index fields) into a table
    // that now has hash indexes. The table should rebuild indexes from rows.
    let json = r#"[
        {"id": 1, "category": "weapon", "name": "Sword"},
        {"id": 2, "category": "armor", "name": "Shield"},
        {"id": 3, "category": "weapon", "name": "Bow"}
    ]"#;

    let table: OrderTestTable = serde_json::from_str(json).unwrap();

    // Hash index should have been rebuilt from rows (backward compat path).
    assert_eq!(table.len(), 3);
    let weapons = table.by_category(&"weapon".to_string(), QueryOpts::ASC);
    assert_eq!(weapons.len(), 2);
    let armor = table.by_category(&"armor".to_string(), QueryOpts::ASC);
    assert_eq!(armor.len(), 1);
    assert_eq!(armor[0].name, "Shield");
}

#[test]
fn hash_index_backward_compat_mutation_after_rebuild() {
    // After loading old-format data and rebuilding, mutations should
    // correctly maintain the hash index.
    let json = r#"[
        {"id": 1, "category": "weapon", "name": "Sword"}
    ]"#;

    let mut table: OrderTestTable = serde_json::from_str(json).unwrap();
    assert_eq!(
        table
            .by_category(&"weapon".to_string(), QueryOpts::ASC)
            .len(),
        1
    );

    // Insert a new row — hash index should be maintained.
    table
        .insert_no_fk(OrderTest {
            id: 2,
            category: "weapon".into(),
            name: "Bow".into(),
        })
        .unwrap();
    assert_eq!(
        table
            .by_category(&"weapon".to_string(), QueryOpts::ASC)
            .len(),
        2
    );

    // Update — should update hash index.
    table
        .update_no_fk(OrderTest {
            id: 1,
            category: "armor".into(),
            name: "Sword".into(),
        })
        .unwrap();
    assert_eq!(
        table
            .by_category(&"weapon".to_string(), QueryOpts::ASC)
            .len(),
        1
    );
    assert_eq!(
        table
            .by_category(&"armor".to_string(), QueryOpts::ASC)
            .len(),
        1
    );
}

#[test]
fn hash_index_serde_format_is_struct_not_array() {
    // Verify the raw JSON shape for tables with hash indexes.
    let mut table = OrderTestTable::new();
    table
        .insert_no_fk(OrderTest {
            id: 1,
            category: "weapon".into(),
            name: "Sword".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Should be a struct with "rows" and "idx_category" fields.
    assert!(parsed.is_object(), "should be a JSON object, got: {}", json);
    assert!(parsed.get("rows").is_some(), "missing 'rows' field");
    assert!(
        parsed.get("idx_category").is_some(),
        "missing 'idx_category' field"
    );
}

// =============================================================================
// Filtered hash index serde roundtrip
// =============================================================================

fn is_adult_serde(c: &FilteredSerdeCreature) -> bool {
    c.age >= 18
}

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(
    name = "adult_species",
    fields("species"),
    kind = "hash",
    filter = "is_adult_serde"
)]
struct FilteredSerdeCreature {
    #[primary_key(auto_increment)]
    id: u32,
    species: String,
    name: String,
    age: u32,
}

#[test]
fn filtered_hash_index_serde_roundtrip() {
    let mut table = FilteredSerdeCreatureTable::new();
    table
        .insert_auto_no_fk(|id| FilteredSerdeCreature {
            id,
            species: "elf".into(),
            name: "Child".into(),
            age: 10,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| FilteredSerdeCreature {
            id,
            species: "elf".into(),
            name: "Elder".into(),
            age: 200,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| FilteredSerdeCreature {
            id,
            species: "dwarf".into(),
            name: "Adult Dwarf".into(),
            age: 50,
        })
        .unwrap();

    // Verify pre-roundtrip: only adults in filtered index.
    let adults_before = table.by_adult_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(adults_before.len(), 1);
    assert_eq!(adults_before[0].name, "Elder");

    let json = serde_json::to_string(&table).unwrap();
    let restored: FilteredSerdeCreatureTable = serde_json::from_str(&json).unwrap();

    // Filtered index preserved.
    let adults_after = restored.by_adult_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(adults_after.len(), 1);
    assert_eq!(adults_after[0].name, "Elder");

    let dwarf_adults = restored.by_adult_species(&"dwarf".to_string(), QueryOpts::ASC);
    assert_eq!(dwarf_adults.len(), 1);

    // Total rows preserved.
    assert_eq!(restored.len(), 3);
}

// =============================================================================
// Compound hash index serde roundtrip
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[index(name = "by_species_age", fields("species", "age"), kind = "hash")]
struct CompoundHashAnimal {
    #[primary_key(auto_increment)]
    id: u32,
    species: String,
    age: u32,
    name: String,
}

#[test]
fn compound_hash_index_serde_roundtrip() {
    let mut table = CompoundHashAnimalTable::new();
    table
        .insert_auto_no_fk(|id| CompoundHashAnimal {
            id,
            species: "cat".into(),
            age: 3,
            name: "Whiskers".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| CompoundHashAnimal {
            id,
            species: "cat".into(),
            age: 5,
            name: "Mittens".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| CompoundHashAnimal {
            id,
            species: "dog".into(),
            age: 3,
            name: "Rex".into(),
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let restored: CompoundHashAnimalTable = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.len(), 3);

    let cats_3 = restored.by_by_species_age(&"cat".to_string(), &3u32, QueryOpts::ASC);
    assert_eq!(cats_3.len(), 1);
    assert_eq!(cats_3[0].name, "Whiskers");

    let all = restored.by_by_species_age(MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(all.len(), 3);
}
