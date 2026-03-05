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
fn database_default_is_empty() {
    let db: TestDb = Default::default();
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

// --- 3e: Database upsert/update success paths with FK ---

#[test]
fn upsert_task_insert_path_with_valid_fk() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();

    // Upsert (insert path) with valid Some FK.
    db.upsert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    assert_eq!(db.tasks.len(), 1);
    assert_eq!(
        db.tasks.get(&TaskId(1)).unwrap().assignee,
        Some(CreatureId(1))
    );
}

#[test]
fn upsert_task_update_path_with_valid_fk() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();

    // Insert via upsert with creature 1.
    db.upsert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    // Update via upsert with creature 2.
    db.upsert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(2)),
        priority: 3,
    })
    .unwrap();

    assert_eq!(db.tasks.len(), 1);
    assert_eq!(
        db.tasks.get(&TaskId(1)).unwrap().assignee,
        Some(CreatureId(2))
    );
    assert_eq!(db.tasks.get(&TaskId(1)).unwrap().priority, 3);
}

#[test]
fn update_task_change_fk_to_different_valid_target() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    // Change FK from creature 1 to creature 2.
    db.update_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(2)),
        priority: 5,
    })
    .unwrap();

    assert_eq!(
        db.tasks.get(&TaskId(1)).unwrap().assignee,
        Some(CreatureId(2))
    );
}

#[test]
fn update_task_change_fk_from_some_to_none() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    // Change FK from Some(1) to None.
    db.update_task(Task {
        id: TaskId(1),
        assignee: None,
        priority: 5,
    })
    .unwrap();

    assert_eq!(db.tasks.get(&TaskId(1)).unwrap().assignee, None);
}

// --- 5a: FkViolation with count > 1 on bare FK field ---

#[test]
fn remove_creature_blocked_by_multiple_bare_fk_refs() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();

    // Two friendships both with source = creature_1.
    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(2),
    })
    .unwrap();
    db.insert_friendship(Friendship {
        id: FriendshipId(2),
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
            referenced_by: vec![("friendships", "source", 2)],
        }
    );
}

// --- 5b: FkViolation ordering with two FK fields both violated ---

#[test]
fn remove_creature_two_fk_fields_same_table_both_violated() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();

    // Friendship where both source and target point to creature_1.
    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(1),
    })
    .unwrap();

    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    // referenced_by should list source before target (struct field declaration order).
    assert_eq!(
        err,
        Error::FkViolation {
            table: "creatures",
            key: "CreatureId(1)".into(),
            referenced_by: vec![("friendships", "source", 1), ("friendships", "target", 1),],
        }
    );
}

// --- 5c: Database upsert_task with None FK on update path ---

#[test]
fn upsert_task_update_path_set_fk_to_none() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();

    // Insert via upsert with Some FK.
    db.upsert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();
    assert_eq!(
        db.tasks.get(&TaskId(1)).unwrap().assignee,
        Some(CreatureId(1))
    );

    // Update via upsert with None FK.
    db.upsert_task(Task {
        id: TaskId(1),
        assignee: None,
        priority: 3,
    })
    .unwrap();

    assert_eq!(db.tasks.len(), 1);
    assert_eq!(db.tasks.get(&TaskId(1)).unwrap().assignee, None);
    assert_eq!(db.tasks.get(&TaskId(1)).unwrap().priority, 3);
}

// --- 5d: remove_task and remove_friendship (no inbound FK refs) ---

#[test]
fn remove_task_and_friendship_succeed() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_creature(make_creature(2, "B")).unwrap();

    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();
    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(2),
    })
    .unwrap();

    // Remove task — no inbound FK references, should succeed.
    let removed_task = db.remove_task(&TaskId(1)).unwrap();
    assert_eq!(removed_task.id, TaskId(1));
    assert_eq!(removed_task.priority, 5);
    assert!(db.tasks.is_empty());

    // Remove friendship — no inbound FK references, should succeed.
    let removed_friendship = db.remove_friendship(&FriendshipId(1)).unwrap();
    assert_eq!(removed_friendship.id, FriendshipId(1));
    assert_eq!(removed_friendship.source, CreatureId(1));
    assert_eq!(removed_friendship.target, CreatureId(2));
    assert!(db.friendships.is_empty());
}

// --- M2: update_creature succeeds even when it has active inbound FK references ---

#[test]
fn update_creature_with_active_inbound_refs() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();
    db.insert_friendship(Friendship {
        id: FriendshipId(1),
        source: CreatureId(1),
        target: CreatureId(1),
    })
    .unwrap();

    // Update the creature — restrict-on-delete should NOT fire on updates.
    db.update_creature(Creature {
        id: CreatureId(1),
        name: "Updated".into(),
        species: Species::Capybara,
    })
    .unwrap();

    assert_eq!(db.creatures.get(&CreatureId(1)).unwrap().name, "Updated");
}

// --- 4d: remove NotFound when table is non-empty ---

#[test]
fn remove_not_found_when_table_non_empty() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();

    // Try to remove a nonexistent creature while another exists.
    let err = db.remove_creature(&CreatureId(99)).unwrap_err();
    assert_eq!(
        err,
        Error::NotFound {
            table: "creatures",
            key: "CreatureId(99)".into(),
        }
    );

    // The existing creature should be untouched.
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creatures.get(&CreatureId(1)).unwrap().name, "A");
}
