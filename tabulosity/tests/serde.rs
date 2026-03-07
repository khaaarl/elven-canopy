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
    // BTreeMap then calls rebuild_indexes). The rebuild reconstructs the index
    // from data, so both entries will be in the index. But subsequent inserts
    // with the same value should still be rejected because the index has entries.
    // This is not ideal — ideally deserialization would reject duplicates — but
    // this test documents the current behavior.
    let table: UserTable = serde_json::from_str(json).unwrap();
    assert_eq!(table.len(), 2);

    // The index has both entries (since rebuild_indexes just inserts all).
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
