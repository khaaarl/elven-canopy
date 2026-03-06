//! Integration tests for auto-increment primary keys.
//!
//! Tests both table-level (`insert_auto_no_fk`, `next_id`) and database-level
//! (`insert_*_auto`) auto-increment behavior.

use tabulosity::{Bounded, Database, Error, Table};

// =============================================================================
// Table-level schema
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ItemId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Item {
    #[primary_key(auto_increment)]
    pub id: ItemId,
    pub name: String,
    pub weight: u32,
}

// =============================================================================
// Table-level tests
// =============================================================================

#[test]
fn auto_insert_empty_table_returns_zero() {
    let mut table = ItemTable::new();
    let id = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    assert_eq!(id, ItemId(0));
    assert_eq!(table.get(&ItemId(0)).unwrap().name, "Sword");
}

#[test]
fn sequential_auto_inserts() {
    let mut table = ItemTable::new();
    let id0 = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "A".into(),
            weight: 1,
        })
        .unwrap();
    let id1 = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "B".into(),
            weight: 2,
        })
        .unwrap();
    let id2 = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "C".into(),
            weight: 3,
        })
        .unwrap();

    assert_eq!(id0, ItemId(0));
    assert_eq!(id1, ItemId(1));
    assert_eq!(id2, ItemId(2));
    assert_eq!(table.len(), 3);
}

#[test]
fn auto_insert_after_manual_insert_at_high_id() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(5),
            name: "Manual".into(),
            weight: 10,
        })
        .unwrap();

    let id = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "Auto".into(),
            weight: 1,
        })
        .unwrap();
    assert_eq!(id, ItemId(6));
}

#[test]
fn manual_insert_bumps_next_id() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(10),
            name: "High".into(),
            weight: 1,
        })
        .unwrap();
    assert_eq!(table.next_id(), ItemId(11));
}

#[test]
fn manual_insert_below_next_id_does_not_lower_it() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(10),
            name: "High".into(),
            weight: 1,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(3),
            name: "Low".into(),
            weight: 1,
        })
        .unwrap();
    assert_eq!(table.next_id(), ItemId(11));
}

#[test]
fn delete_does_not_affect_next_id() {
    let mut table = ItemTable::new();
    let id = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "A".into(),
            weight: 1,
        })
        .unwrap();
    assert_eq!(table.next_id(), ItemId(1));

    table.remove_no_fk(&id).unwrap();
    assert_eq!(table.next_id(), ItemId(1));
}

#[test]
fn upsert_bumps_next_id() {
    let mut table = ItemTable::new();
    table.upsert_no_fk(Item {
        id: ItemId(7),
        name: "Upserted".into(),
        weight: 1,
    });
    assert_eq!(table.next_id(), ItemId(8));
}

#[test]
fn interleaved_manual_and_auto_inserts() {
    let mut table = ItemTable::new();

    let id0 = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "Auto0".into(),
            weight: 1,
        })
        .unwrap();
    assert_eq!(id0, ItemId(0));

    table
        .insert_no_fk(Item {
            id: ItemId(5),
            name: "Manual5".into(),
            weight: 1,
        })
        .unwrap();

    let id1 = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "Auto6".into(),
            weight: 1,
        })
        .unwrap();
    assert_eq!(id1, ItemId(6));

    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "Manual2".into(),
            weight: 1,
        })
        .unwrap();

    // next_id should still be 7 (not lowered by manual insert at 2).
    let id2 = table
        .insert_auto_no_fk(|pk| Item {
            id: pk,
            name: "Auto7".into(),
            weight: 1,
        })
        .unwrap();
    assert_eq!(id2, ItemId(7));

    assert_eq!(table.len(), 5);
}

#[test]
fn next_id_accessor() {
    let table = ItemTable::new();
    assert_eq!(table.next_id(), ItemId(0));
}

// =============================================================================
// Database-level schema
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ProjectId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Project {
    #[primary_key(auto_increment)]
    pub id: ProjectId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Task {
    #[primary_key(auto_increment)]
    pub id: TaskId,
    #[indexed]
    pub project: ProjectId,
    pub label: String,
}

#[derive(Database)]
struct AutoDb {
    #[table(singular = "project", auto)]
    pub projects: ProjectTable,

    #[table(singular = "task", auto, fks(project = "projects"))]
    pub tasks: TaskTable,
}

// =============================================================================
// Database-level tests
// =============================================================================

#[test]
fn db_insert_auto_creates_row_with_auto_pk() {
    let mut db = AutoDb::new();
    let pid = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "Alpha".into(),
        })
        .unwrap();
    assert_eq!(pid, ProjectId(0));
    assert_eq!(db.projects.get(&ProjectId(0)).unwrap().name, "Alpha");
}

#[test]
fn db_insert_auto_validates_fk() {
    let mut db = AutoDb::new();
    // No projects exist — FK to project should fail.
    let err = db
        .insert_task_auto(|pk| Task {
            id: pk,
            project: ProjectId(99),
            label: "orphan".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn db_insert_auto_with_valid_fk() {
    let mut db = AutoDb::new();
    let pid = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "Alpha".into(),
        })
        .unwrap();

    let tid = db
        .insert_task_auto(|pk| Task {
            id: pk,
            project: pid,
            label: "build".into(),
        })
        .unwrap();
    assert_eq!(tid, TaskId(0));
    assert_eq!(db.tasks.get(&TaskId(0)).unwrap().project, pid);
}

#[test]
fn db_sequential_auto_inserts() {
    let mut db = AutoDb::new();
    let p0 = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "A".into(),
        })
        .unwrap();
    let p1 = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "B".into(),
        })
        .unwrap();
    let p2 = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "C".into(),
        })
        .unwrap();

    assert_eq!(p0, ProjectId(0));
    assert_eq!(p1, ProjectId(1));
    assert_eq!(p2, ProjectId(2));
    assert_eq!(db.projects.len(), 3);
}

#[test]
fn db_mix_manual_and_auto_inserts() {
    let mut db = AutoDb::new();

    // Manual insert at ID 5.
    db.insert_project(Project {
        id: ProjectId(5),
        name: "Manual".into(),
    })
    .unwrap();

    // Auto insert should get ID 6.
    let pid = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "Auto".into(),
        })
        .unwrap();
    assert_eq!(pid, ProjectId(6));
}
