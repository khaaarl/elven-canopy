//! Integration tests for `#[derive(Database)]` with FK validation.

use tabulosity::{Bounded, Database, Error, Table};

// --- Row types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CreatureId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Species {
    Elf,
    Capybara,
}

#[derive(Table, Clone, Debug, PartialEq)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
    #[indexed]
    pub species: Species,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Task {
    #[primary_key]
    pub id: TaskId,
    #[indexed]
    pub assignee: Option<CreatureId>,
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct FriendshipId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Friendship {
    #[primary_key]
    pub id: FriendshipId,
    #[indexed]
    pub source: CreatureId,
    #[indexed]
    pub target: CreatureId,
}

// --- Database schema ---

#[derive(Database)]
struct TestDb {
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

// --- Tests ---

fn make_creature(id: u32, name: &str) -> Creature {
    Creature {
        id: CreatureId(id),
        name: name.into(),
        species: Species::Elf,
    }
}

#[test]
fn new_database_is_empty() {
    let db = TestDb::new();
    assert!(db.creatures.is_empty());
    assert!(db.tasks.is_empty());
    assert!(db.friendships.is_empty());
}

#[test]
fn insert_and_read() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Aelindra")).unwrap();

    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creatures.get(&CreatureId(1)).unwrap().name, "Aelindra");
}

#[test]
fn insert_duplicate_key() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    let err = db.insert_creature(make_creature(1, "B")).unwrap_err();
    assert_eq!(
        err,
        Error::DuplicateKey {
            table: "creatures",
            key: "CreatureId(1)".into(),
        }
    );
}

#[test]
fn update_creature() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.update_creature(Creature {
        id: CreatureId(1),
        name: "Updated".into(),
        species: Species::Capybara,
    })
    .unwrap();
    assert_eq!(db.creatures.get(&CreatureId(1)).unwrap().name, "Updated");
}

#[test]
fn update_not_found() {
    let mut db = TestDb::new();
    let err = db.update_creature(make_creature(99, "Ghost")).unwrap_err();
    assert_eq!(
        err,
        Error::NotFound {
            table: "creatures",
            key: "CreatureId(99)".into(),
        }
    );
}

#[test]
fn upsert_creature() {
    let mut db = TestDb::new();
    // Insert via upsert
    db.upsert_creature(make_creature(1, "A")).unwrap();
    assert_eq!(db.creatures.len(), 1);

    // Update via upsert
    db.upsert_creature(make_creature(1, "B")).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creatures.get(&CreatureId(1)).unwrap().name, "B");
}

#[test]
fn remove_creature_no_refs() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    let removed = db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(removed.name, "A");
    assert!(db.creatures.is_empty());
}

#[test]
fn remove_not_found() {
    let mut db = TestDb::new();
    let err = db.remove_creature(&CreatureId(99)).unwrap_err();
    assert_eq!(
        err,
        Error::NotFound {
            table: "creatures",
            key: "CreatureId(99)".into(),
        }
    );
}

// --- FK validation on insert/update ---

#[test]
fn insert_task_fk_target_not_found() {
    let mut db = TestDb::new();
    let err = db
        .insert_task(Task {
            id: TaskId(1),
            assignee: Some(CreatureId(99)),
            priority: 5,
        })
        .unwrap_err();
    assert_eq!(
        err,
        Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: format!("{:?}", Some(CreatureId(99))),
        }
    );
}

#[test]
fn insert_task_optional_fk_none_ok() {
    let mut db = TestDb::new();
    // None FK should pass without any creatures.
    db.insert_task(Task {
        id: TaskId(1),
        assignee: None,
        priority: 5,
    })
    .unwrap();
}

#[test]
fn insert_task_optional_fk_some_valid() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();
}

#[test]
fn update_task_fk_validation() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    // Update to point at nonexistent creature.
    let err = db
        .update_task(Task {
            id: TaskId(1),
            assignee: Some(CreatureId(99)),
            priority: 5,
        })
        .unwrap_err();
    assert_eq!(
        err,
        Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: format!("{:?}", Some(CreatureId(99))),
        }
    );
}

#[test]
fn upsert_task_fk_validation() {
    let mut db = TestDb::new();
    let err = db
        .upsert_task(Task {
            id: TaskId(1),
            assignee: Some(CreatureId(99)),
            priority: 5,
        })
        .unwrap_err();
    assert_eq!(
        err,
        Error::FkTargetNotFound {
            table: "tasks",
            field: "assignee",
            referenced_table: "creatures",
            key: format!("{:?}", Some(CreatureId(99))),
        }
    );
}

#[test]
fn insert_friendship_bare_fk() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();

    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(2),
    })
    .unwrap();
}

#[test]
fn insert_friendship_fk_source_not_found() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(2, "B")).unwrap();

    let err = db
        .insert_friendship(Friendship {
            id: FriendshipId(1),
            source: CreatureId(99),
            target: CreatureId(2),
        })
        .unwrap_err();
    assert_eq!(
        err,
        Error::FkTargetNotFound {
            table: "friendships",
            field: "source",
            referenced_table: "creatures",
            key: "CreatureId(99)".into(),
        }
    );
}

#[test]
fn insert_friendship_fk_target_not_found() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();

    let err = db
        .insert_friendship(Friendship {
            id: FriendshipId(1),
            source: CreatureId(1),
            target: CreatureId(99),
        })
        .unwrap_err();
    assert_eq!(
        err,
        Error::FkTargetNotFound {
            table: "friendships",
            field: "target",
            referenced_table: "creatures",
            key: "CreatureId(99)".into(),
        }
    );
}

// --- Restrict-on-delete ---

#[test]
fn remove_creature_blocked_by_optional_fk() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert_eq!(
        err,
        Error::FkViolation {
            table: "creatures",
            key: "CreatureId(1)".into(),
            referenced_by: vec![("tasks", "assignee", 1)],
        }
    );
}

#[test]
fn remove_creature_not_blocked_by_none() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    // Task with None assignee — should NOT block removal.
    db.insert_task(Task {
        id: TaskId(1),
        assignee: None,
        priority: 5,
    })
    .unwrap();

    let removed = db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(removed.name, "A");
}

#[test]
fn remove_creature_blocked_by_bare_fk() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();
    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(2),
    })
    .unwrap();

    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert_eq!(
        err,
        Error::FkViolation {
            table: "creatures",
            key: "CreatureId(1)".into(),
            referenced_by: vec![("friendships", "source", 1)],
        }
    );
}

#[test]
fn remove_creature_blocked_by_target_fk() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();
    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(2),
    })
    .unwrap();

    let err = db.remove_creature(&CreatureId(2)).unwrap_err();
    assert_eq!(
        err,
        Error::FkViolation {
            table: "creatures",
            key: "CreatureId(2)".into(),
            referenced_by: vec![("friendships", "target", 1)],
        }
    );
}

#[test]
fn remove_collects_all_violations() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();

    // 2 tasks referencing creature 1
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();
    db.insert_task(Task {
        id: TaskId(2),
        assignee: Some(CreatureId(1)),
        priority: 3,
    })
    .unwrap();

    // 1 friendship referencing creature 1 as source
    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(2),
    })
    .unwrap();

    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    // Should collect ALL violations, not short-circuit.
    match err {
        Error::FkViolation { referenced_by, .. } => {
            // Both tables should be listed — order matches struct field declaration.
            assert_eq!(referenced_by.len(), 2);
            assert_eq!(referenced_by[0], ("tasks", "assignee", 2));
            assert_eq!(referenced_by[1], ("friendships", "source", 1));
        }
        other => panic!("expected FkViolation, got {:?}", other),
    }
}
