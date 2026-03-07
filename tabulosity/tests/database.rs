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
    db.remove_creature(&CreatureId(1)).unwrap();
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

    db.remove_creature(&CreatureId(1)).unwrap();
    assert!(db.creatures.is_empty());
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
    db.remove_task(&TaskId(1)).unwrap();
    assert!(db.tasks.is_empty());

    // Remove friendship — no inbound FK references, should succeed.
    db.remove_friendship(&FriendshipId(1)).unwrap();
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

// =============================================================================
// Cascade and nullify on_delete tests
// =============================================================================

// --- Schema with cascade/nullify FKs ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ProjectId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Project {
    #[primary_key]
    pub id: ProjectId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct JobId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Job {
    #[primary_key]
    pub id: JobId,
    #[indexed]
    pub project: ProjectId,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct SubtaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Subtask {
    #[primary_key]
    pub id: SubtaskId,
    #[indexed]
    pub job: JobId,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct WatcherId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
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

    #[table(singular = "subtask", fks(job = "jobs" on_delete cascade))]
    pub subtasks: SubtaskTable,

    #[table(singular = "watcher", fks(project? = "projects" on_delete nullify))]
    pub watchers: WatcherTable,
}

fn make_project(id: u32, name: &str) -> Project {
    Project {
        id: ProjectId(id),
        name: name.into(),
    }
}

fn make_job(id: u32, project: u32, label: &str) -> Job {
    Job {
        id: JobId(id),
        project: ProjectId(project),
        label: label.into(),
    }
}

fn make_subtask(id: u32, job: u32, detail: &str) -> Subtask {
    Subtask {
        id: SubtaskId(id),
        job: JobId(job),
        detail: detail.into(),
    }
}

fn make_watcher(id: u32, project: Option<u32>, name: &str) -> Watcher {
    Watcher {
        id: WatcherId(id),
        project: project.map(ProjectId),
        name: name.into(),
    }
}

#[test]
fn cascade_removes_dependent_rows() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();
    db.insert_job(make_job(1, 1, "build")).unwrap();
    db.insert_job(make_job(2, 1, "test")).unwrap();

    db.remove_project(&ProjectId(1)).unwrap();
    assert!(db.projects.is_empty());
    assert!(db.jobs.is_empty());
}

#[test]
fn cascade_chain() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();
    db.insert_job(make_job(1, 1, "build")).unwrap();
    db.insert_subtask(make_subtask(1, 1, "compile")).unwrap();
    db.insert_subtask(make_subtask(2, 1, "link")).unwrap();

    // Deleting project cascades to jobs, which cascades to subtasks.
    db.remove_project(&ProjectId(1)).unwrap();
    assert!(db.projects.is_empty());
    assert!(db.jobs.is_empty());
    assert!(db.subtasks.is_empty());
}

#[test]
fn cascade_removes_nothing_when_no_refs() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();

    // No jobs reference this project, cascade is a no-op.
    db.remove_project(&ProjectId(1)).unwrap();
    assert!(db.projects.is_empty());
}

#[test]
fn cascade_only_removes_matching_refs() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();
    db.insert_project(make_project(2, "Beta")).unwrap();
    db.insert_job(make_job(1, 1, "build")).unwrap();
    db.insert_job(make_job(2, 2, "deploy")).unwrap();

    db.remove_project(&ProjectId(1)).unwrap();
    assert_eq!(db.projects.len(), 1);
    assert_eq!(db.jobs.len(), 1);
    assert_eq!(db.jobs.get(&JobId(2)).unwrap().label, "deploy");
}

#[test]
fn nullify_sets_fk_to_none() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();
    db.insert_watcher(make_watcher(1, Some(1), "Alice"))
        .unwrap();
    db.insert_watcher(make_watcher(2, Some(1), "Bob")).unwrap();

    db.remove_project(&ProjectId(1)).unwrap();
    assert!(db.projects.is_empty());
    // Watchers still exist but FK is nullified.
    assert_eq!(db.watchers.len(), 2);
    assert_eq!(db.watchers.get(&WatcherId(1)).unwrap().project, None);
    assert_eq!(db.watchers.get(&WatcherId(2)).unwrap().project, None);
}

#[test]
fn nullify_no_refs_is_noop() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();

    // No watchers point here.
    db.remove_project(&ProjectId(1)).unwrap();
    assert!(db.projects.is_empty());
}

#[test]
fn nullify_only_affects_matching_refs() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();
    db.insert_project(make_project(2, "Beta")).unwrap();
    db.insert_watcher(make_watcher(1, Some(1), "Alice"))
        .unwrap();
    db.insert_watcher(make_watcher(2, Some(2), "Bob")).unwrap();
    db.insert_watcher(make_watcher(3, None, "Charlie")).unwrap();

    db.remove_project(&ProjectId(1)).unwrap();
    assert_eq!(db.watchers.get(&WatcherId(1)).unwrap().project, None);
    assert_eq!(
        db.watchers.get(&WatcherId(2)).unwrap().project,
        Some(ProjectId(2))
    );
    assert_eq!(db.watchers.get(&WatcherId(3)).unwrap().project, None);
}

#[test]
fn cascade_then_nullify_both_fire() {
    let mut db = CascadeDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();
    db.insert_job(make_job(1, 1, "build")).unwrap();
    db.insert_watcher(make_watcher(1, Some(1), "Alice"))
        .unwrap();

    // Both cascade (jobs) and nullify (watchers) should fire.
    db.remove_project(&ProjectId(1)).unwrap();
    assert!(db.projects.is_empty());
    assert!(db.jobs.is_empty());
    assert_eq!(db.watchers.len(), 1);
    assert_eq!(db.watchers.get(&WatcherId(1)).unwrap().project, None);
}

// --- Cascade chain with nullify at the leaf ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct NoteId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Note {
    #[primary_key]
    pub id: NoteId,
    #[indexed]
    pub job: Option<JobId>,
    pub text: String,
}

#[derive(Database)]
struct CascadeNullifyChainDb {
    #[table(singular = "project")]
    pub projects: ProjectTable,

    #[table(singular = "job", fks(project = "projects" on_delete cascade))]
    pub jobs: JobTable,

    #[table(singular = "note", fks(job? = "jobs" on_delete nullify))]
    pub notes: NoteTable,
}

#[test]
fn cascade_chain_then_nullify_at_leaf() {
    // project -> job (cascade) -> note (nullify on job)
    // Deleting project cascades to remove jobs, which nullifies notes.
    let mut db = CascadeNullifyChainDb::new();
    db.insert_project(make_project(1, "Alpha")).unwrap();
    db.insert_job(make_job(1, 1, "build")).unwrap();
    db.insert_note(Note {
        id: NoteId(1),
        job: Some(JobId(1)),
        text: "important".into(),
    })
    .unwrap();

    db.remove_project(&ProjectId(1)).unwrap();
    assert!(db.projects.is_empty());
    assert!(db.jobs.is_empty());
    // Note survives but its FK is nullified.
    assert_eq!(db.notes.len(), 1);
    assert_eq!(db.notes.get(&NoteId(1)).unwrap().job, None);
}

#[test]
fn default_is_still_restrict() {
    // The original TestDb uses default (restrict) semantics.
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "A")).unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: Some(CreatureId(1)),
        priority: 5,
    })
    .unwrap();

    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert!(matches!(err, Error::FkViolation { .. }));
}

// --- Schema with mixed cascade and restrict on same target ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TeamId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Team {
    #[primary_key]
    pub id: TeamId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct MemoId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Memo {
    #[primary_key]
    pub id: MemoId,
    #[indexed]
    pub team: TeamId,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct RuleId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Rule {
    #[primary_key]
    pub id: RuleId,
    #[indexed]
    pub team: TeamId,
    pub description: String,
}

#[derive(Database)]
struct MixedDb {
    #[table(singular = "team")]
    pub teams: TeamTable,

    #[table(singular = "memo", fks(team = "teams" on_delete cascade))]
    pub memos: MemoTable,

    #[table(singular = "rule", fks(team = "teams"))]
    pub rules: RuleTable,
}

#[test]
fn cascade_mixed_with_restrict() {
    let mut db = MixedDb::new();
    db.insert_team(Team {
        id: TeamId(1),
        name: "Alpha".into(),
    })
    .unwrap();
    db.insert_memo(Memo {
        id: MemoId(1),
        team: TeamId(1),
        text: "hello".into(),
    })
    .unwrap();
    db.insert_rule(Rule {
        id: RuleId(1),
        team: TeamId(1),
        description: "no running".into(),
    })
    .unwrap();

    // Restrict on rules should block removal even though memos would cascade.
    let err = db.remove_team(&TeamId(1)).unwrap_err();
    assert!(matches!(err, Error::FkViolation { .. }));

    // Everything should still be intact.
    assert_eq!(db.teams.len(), 1);
    assert_eq!(db.memos.len(), 1);
    assert_eq!(db.rules.len(), 1);
}

#[test]
fn restrict_after_cascade_clears_refs() {
    let mut db = MixedDb::new();
    db.insert_team(Team {
        id: TeamId(1),
        name: "Alpha".into(),
    })
    .unwrap();
    db.insert_memo(Memo {
        id: MemoId(1),
        team: TeamId(1),
        text: "hello".into(),
    })
    .unwrap();

    // No rules reference this team, so restrict passes. Cascade clears memos.
    db.remove_team(&TeamId(1)).unwrap();
    assert!(db.teams.is_empty());
    assert!(db.memos.is_empty());
}

/// Additional gap coverage tests for database-level FK semantics: self-referential
/// FKs, deep cascade chains blocked by restrict, nullify + unique index
/// interactions, filtered/unique index cleanup on cascade, modify_unchecked
/// through the database API, mixed cascade/nullify on same parent, and
/// multi-FK nullification.
mod gap_coverage {
    use std::panic;

    use tabulosity::{Bounded, Database, Error, MatchAll, QueryOpts, Table};

    // --- Self-referential FKs ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct EmpId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Employee {
        #[primary_key]
        pub id: EmpId,
        #[indexed]
        pub manager: Option<EmpId>,
        pub name: String,
    }

    #[derive(Database)]
    struct SelfRefNullifyDb {
        #[table(singular = "employee", fks(manager? = "employees" on_delete nullify))]
        pub employees: EmployeeTable,
    }

    #[test]
    fn self_ref_nullify_deletes_manager() {
        let mut db = SelfRefNullifyDb::new();
        db.insert_employee(Employee {
            id: EmpId(1),
            name: "Boss".into(),
            manager: None,
        })
        .unwrap();
        db.insert_employee(Employee {
            id: EmpId(2),
            name: "Report".into(),
            manager: Some(EmpId(1)),
        })
        .unwrap();

        // Delete the manager -- report's manager should be nullified.
        db.remove_employee(&EmpId(1)).unwrap();
        assert_eq!(db.employees.len(), 1);
        assert_eq!(db.employees.get(&EmpId(2)).unwrap().manager, None);
    }

    #[test]
    fn self_ref_insert_referencing_self_fails() {
        let mut db = SelfRefNullifyDb::new();
        // Inserting an employee that references itself fails because the FK check
        // runs before the row is in the table.
        let err = db
            .insert_employee(Employee {
                id: EmpId(1),
                name: "Self-manager".into(),
                manager: Some(EmpId(1)),
            })
            .unwrap_err();
        assert!(matches!(err, Error::FkTargetNotFound { .. }));
    }

    #[test]
    fn self_ref_nullify_via_update_then_delete() {
        let mut db = SelfRefNullifyDb::new();
        // Insert with None, then update to reference self.
        db.insert_employee(Employee {
            id: EmpId(1),
            name: "Self-manager".into(),
            manager: None,
        })
        .unwrap();
        db.update_employee(Employee {
            id: EmpId(1),
            name: "Self-manager".into(),
            manager: Some(EmpId(1)),
        })
        .unwrap();

        // Delete should nullify the self-reference (via update_no_fk), then remove.
        db.remove_employee(&EmpId(1)).unwrap();
        assert!(db.employees.is_empty());
    }

    #[test]
    fn self_ref_nullify_chain() {
        let mut db = SelfRefNullifyDb::new();
        db.insert_employee(Employee {
            id: EmpId(1),
            name: "CEO".into(),
            manager: None,
        })
        .unwrap();
        db.insert_employee(Employee {
            id: EmpId(2),
            name: "VP".into(),
            manager: Some(EmpId(1)),
        })
        .unwrap();
        db.insert_employee(Employee {
            id: EmpId(3),
            name: "IC".into(),
            manager: Some(EmpId(2)),
        })
        .unwrap();

        // Delete CEO -- only VP's manager is nullified (VP references CEO).
        // IC still references VP, which is fine.
        db.remove_employee(&EmpId(1)).unwrap();
        assert_eq!(db.employees.len(), 2);
        assert_eq!(db.employees.get(&EmpId(2)).unwrap().manager, None);
        assert_eq!(db.employees.get(&EmpId(3)).unwrap().manager, Some(EmpId(2)));
    }

    // NOTE: Self-referential cascade is correctly rejected at compile time as a
    // cascade cycle. This is by design -- the cycle detection in derive(Database)
    // prevents employees -> employees cascade loops.

    // --- Deep cascade chain with restrict at leaf ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct RegionId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Region {
        #[primary_key]
        pub id: RegionId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct GuildId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Guild {
        #[primary_key]
        pub id: GuildId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct TeamId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Team {
        #[primary_key]
        pub id: TeamId,
        #[indexed]
        pub region: RegionId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct WorkerId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Worker {
        #[primary_key]
        pub id: WorkerId,
        #[indexed]
        pub team: TeamId,
        #[indexed]
        pub guild: GuildId,
        pub name: String,
    }

    #[derive(Database)]
    struct DeepCascadeDb {
        #[table(singular = "region")]
        pub regions: RegionTable,

        #[table(singular = "guild")]
        pub guilds: GuildTable,

        #[table(singular = "team", fks(region = "regions" on_delete cascade))]
        pub teams: TeamTable,

        // Worker has cascade from team but restrict from guild.
        #[table(singular = "worker", fks(team = "teams" on_delete cascade, guild = "guilds"))]
        pub workers: WorkerTable,
    }

    #[test]
    fn deep_cascade_blocked_by_restrict_is_not_atomic() {
        // NOTE: The restrict check is on the *guild* table (when removing a guild,
        // workers block it), not on the worker table when removing a worker. So
        // cascade delete of worker succeeds since workers don't have inbound FKs.
        let mut db = DeepCascadeDb::new();

        db.insert_region(Region {
            id: RegionId(1),
            name: "North".into(),
        })
        .unwrap();
        db.insert_guild(Guild {
            id: GuildId(1),
            name: "Fighters".into(),
        })
        .unwrap();
        db.insert_team(Team {
            id: TeamId(1),
            region: RegionId(1),
            name: "Alpha".into(),
        })
        .unwrap();
        db.insert_worker(Worker {
            id: WorkerId(1),
            team: TeamId(1),
            guild: GuildId(1),
            name: "Bob".into(),
        })
        .unwrap();

        db.remove_region(&RegionId(1)).unwrap();
        assert!(db.regions.is_empty());
        assert!(db.teams.is_empty());
        assert!(db.workers.is_empty());
        assert_eq!(db.guilds.len(), 1); // Guild is untouched
    }

    // Cascade leads to restrict failure: Region -> Team (cascade), Team restricted
    // by Rule table.

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct RuleId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Rule {
        #[primary_key]
        pub id: RuleId,
        #[indexed]
        pub team: TeamId,
        pub description: String,
    }

    #[derive(Database)]
    struct CascadeRestrictDb {
        #[table(singular = "region")]
        pub regions: RegionTable,

        #[table(singular = "team", fks(region = "regions" on_delete cascade))]
        pub teams: TeamTable,

        #[table(singular = "rule", fks(team = "teams"))]
        pub rules: RuleTable,
    }

    #[test]
    fn cascade_blocked_by_restrict_at_leaf() {
        let mut db = CascadeRestrictDb::new();

        db.insert_region(Region {
            id: RegionId(1),
            name: "North".into(),
        })
        .unwrap();
        db.insert_team(Team {
            id: TeamId(1),
            region: RegionId(1),
            name: "Alpha".into(),
        })
        .unwrap();
        db.insert_rule(Rule {
            id: RuleId(1),
            team: TeamId(1),
            description: "No running".into(),
        })
        .unwrap();

        // Deleting region cascades to remove team, but team has restrict refs from rules.
        let err = db.remove_region(&RegionId(1)).unwrap_err();
        assert!(matches!(err, Error::FkViolation { .. }));

        // Generated code order: restrict checks -> cascade -> nullify -> remove row.
        // Region has no restrict FKs, so restrict passes. Cascade collects team
        // PKs and calls remove_team for each. remove_team sees restrict violation
        // from rules and returns Err, which propagates up. The region row has NOT
        // been removed yet because row removal happens AFTER cascade in the
        // generated code. So the entire operation fails with everything intact.
        assert_eq!(db.regions.len(), 1, "region should still exist");
        assert_eq!(db.teams.len(), 1, "team should still exist");
        assert_eq!(db.rules.len(), 1, "rule should still exist");
    }

    // --- Nullify + unique index: multiple rows nullified to same None ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct ProjId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Project {
        #[primary_key]
        pub id: ProjId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct AssignId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Assignment {
        #[primary_key]
        pub id: AssignId,
        #[indexed(unique)]
        pub project: Option<ProjId>,
        pub label: String,
    }

    #[derive(Database)]
    struct NullifyUniqueDb {
        #[table(singular = "project")]
        pub projects: ProjectTable,

        #[table(singular = "assignment", fks(project? = "projects" on_delete nullify))]
        pub assignments: AssignmentTable,
    }

    #[test]
    fn nullify_unique_index_single_row_to_none_works() {
        let mut db = NullifyUniqueDb::new();

        db.insert_project(Project {
            id: ProjId(1),
            name: "Alpha".into(),
        })
        .unwrap();
        db.insert_project(Project {
            id: ProjId(2),
            name: "Beta".into(),
        })
        .unwrap();

        db.insert_assignment(Assignment {
            id: AssignId(1),
            project: Some(ProjId(1)),
            label: "A".into(),
        })
        .unwrap();
        db.insert_assignment(Assignment {
            id: AssignId(2),
            project: Some(ProjId(2)),
            label: "B".into(),
        })
        .unwrap();

        // First nullify works fine -- no other None in the unique index.
        db.remove_project(&ProjId(1)).unwrap();
        assert_eq!(db.assignments.get(&AssignId(1)).unwrap().project, None);
    }

    #[test]
    fn nullify_unique_index_multiple_rows_to_none_fails() {
        // BUG: When a unique-indexed Option<T> field is nullified via on_delete
        // nullify, the second nullification fails because the unique index already
        // contains a None entry. The nullify path uses update_no_fk, which enforces
        // unique constraints, and None is not exempt from uniqueness.
        //
        // This documents the current behavior. A fix would require either:
        // 1. Exempting None from unique checks on Option<T> fields, or
        // 2. Using a bypass path for nullify updates.
        let mut db = NullifyUniqueDb::new();

        db.insert_project(Project {
            id: ProjId(1),
            name: "Alpha".into(),
        })
        .unwrap();
        db.insert_project(Project {
            id: ProjId(2),
            name: "Beta".into(),
        })
        .unwrap();

        db.insert_assignment(Assignment {
            id: AssignId(1),
            project: Some(ProjId(1)),
            label: "A".into(),
        })
        .unwrap();
        db.insert_assignment(Assignment {
            id: AssignId(2),
            project: Some(ProjId(2)),
            label: "B".into(),
        })
        .unwrap();

        // First nullify succeeds.
        db.remove_project(&ProjId(1)).unwrap();

        // Second nullify panics -- None is already in the unique index.
        // The generated nullify code calls update_no_fk(...).unwrap(), which panics
        // when the unique check fails on the second None value.
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| db.remove_project(&ProjId(2))));
        assert!(
            result.is_err(),
            "Expected panic from nullify unique conflict"
        );
    }

    // --- Filtered index cleaned up on cascade delete ---

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Proj {
        #[primary_key]
        pub id: ProjId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct TaskId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    #[index(
        name = "active_priority",
        fields("priority"),
        filter = "FiltTask::is_active"
    )]
    struct FiltTask {
        #[primary_key]
        pub id: TaskId,
        #[indexed]
        pub project: ProjId,
        pub priority: u32,
        pub active: bool,
    }

    impl FiltTask {
        fn is_active(&self) -> bool {
            self.active
        }
    }

    #[derive(Database)]
    struct FiltCascadeDb {
        #[table(singular = "proj")]
        pub projs: ProjTable,

        #[table(singular = "filt_task", fks(project = "projs" on_delete cascade))]
        pub filt_tasks: FiltTaskTable,
    }

    #[test]
    fn filtered_index_cleaned_up_on_cascade_delete() {
        let mut db = FiltCascadeDb::new();
        db.insert_proj(Proj {
            id: ProjId(1),
            name: "Alpha".into(),
        })
        .unwrap();
        db.insert_filt_task(FiltTask {
            id: TaskId(1),
            project: ProjId(1),
            priority: 5,
            active: true,
        })
        .unwrap();
        db.insert_filt_task(FiltTask {
            id: TaskId(2),
            project: ProjId(1),
            priority: 3,
            active: false,
        })
        .unwrap();

        // Before cascade: filtered index has 1 active task.
        assert_eq!(
            db.filt_tasks
                .count_by_active_priority(MatchAll, QueryOpts::ASC),
            1
        );

        // Cascade delete removes both tasks.
        db.remove_proj(&ProjId(1)).unwrap();
        assert!(db.filt_tasks.is_empty());
        assert_eq!(
            db.filt_tasks
                .count_by_active_priority(MatchAll, QueryOpts::ASC),
            0
        );
    }

    // --- Cascade frees unique index values for reuse ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct UId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct UniqueChild {
        #[primary_key]
        pub id: UId,
        #[indexed]
        pub parent: ProjId,
        #[indexed(unique)]
        pub code: String,
    }

    #[derive(Database)]
    struct UniqueCascadeDb {
        #[table(singular = "proj")]
        pub projs: ProjTable,

        #[table(singular = "unique_child", fks(parent = "projs" on_delete cascade))]
        pub unique_children: UniqueChildTable,
    }

    #[test]
    fn cascade_delete_frees_unique_values_for_reuse() {
        let mut db = UniqueCascadeDb::new();
        db.insert_proj(Proj {
            id: ProjId(1),
            name: "Alpha".into(),
        })
        .unwrap();
        db.insert_unique_child(UniqueChild {
            id: UId(1),
            parent: ProjId(1),
            code: "ABC".into(),
        })
        .unwrap();

        // Cascade delete frees the unique value "ABC".
        db.remove_proj(&ProjId(1)).unwrap();
        assert!(db.unique_children.is_empty());

        // Re-insert with same unique value should succeed.
        db.insert_proj(Proj {
            id: ProjId(2),
            name: "Beta".into(),
        })
        .unwrap();
        db.insert_unique_child(UniqueChild {
            id: UId(2),
            parent: ProjId(2),
            code: "ABC".into(),
        })
        .unwrap();
        assert_eq!(db.unique_children.len(), 1);
    }

    // --- modify_unchecked through database API ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct OwnerId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Owner {
        #[primary_key]
        pub id: OwnerId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct PetId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Pet {
        #[primary_key]
        pub id: PetId,
        #[indexed]
        pub owner: Option<OwnerId>,
        pub name: String,
    }

    #[derive(Database)]
    struct PetDb {
        #[table(singular = "owner")]
        pub owners: OwnerTable,

        #[table(singular = "pet", fks(owner? = "owners" on_delete nullify))]
        pub pets: PetTable,
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "indexed field")]
    fn modify_unchecked_fk_field_change_caught_by_debug_assertions() {
        // FK fields are typically indexed. In debug builds, modify_unchecked catches
        // changes to indexed fields. In release builds, this would silently bypass
        // both FK validation and index maintenance, creating an orphaned FK.
        let mut db = PetDb::new();
        db.insert_owner(Owner {
            id: OwnerId(1),
            name: "Alice".into(),
        })
        .unwrap();
        db.insert_pet(Pet {
            id: PetId(1),
            owner: Some(OwnerId(1)),
            name: "Fluffy".into(),
        })
        .unwrap();

        db.modify_unchecked_pet(&PetId(1), |p| {
            p.owner = Some(OwnerId(999)); // nonexistent owner -- debug assertion fires
        })
        .unwrap();
    }

    #[test]
    fn modify_unchecked_non_fk_field_on_pet() {
        // Changing non-indexed fields is fine even through the database API.
        let mut db = PetDb::new();
        db.insert_owner(Owner {
            id: OwnerId(1),
            name: "Alice".into(),
        })
        .unwrap();
        db.insert_pet(Pet {
            id: PetId(1),
            owner: Some(OwnerId(1)),
            name: "Fluffy".into(),
        })
        .unwrap();

        db.modify_unchecked_pet(&PetId(1), |p| {
            p.name = "Renamed".into();
        })
        .unwrap();

        assert_eq!(db.pets.get(&PetId(1)).unwrap().name, "Renamed");
    }

    // --- Mixed cascade/nullify: two FK fields on same table pointing at same parent ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct UserId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct User {
        #[primary_key]
        pub id: UserId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct DocId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Doc {
        #[primary_key]
        pub id: DocId,
        #[indexed]
        pub created_by: UserId,
        #[indexed]
        pub reviewed_by: Option<UserId>,
        pub title: String,
    }

    #[derive(Database)]
    struct MixedFkDb {
        #[table(singular = "user")]
        pub users: UserTable,

        #[table(singular = "doc", fks(created_by = "users" on_delete cascade, reviewed_by? = "users" on_delete nullify))]
        pub docs: DocTable,
    }

    #[test]
    fn mixed_cascade_nullify_same_parent() {
        let mut db = MixedFkDb::new();
        db.insert_user(User {
            id: UserId(1),
            name: "Alice".into(),
        })
        .unwrap();
        db.insert_user(User {
            id: UserId(2),
            name: "Bob".into(),
        })
        .unwrap();

        // Doc created by user 1, reviewed by user 1.
        db.insert_doc(Doc {
            id: DocId(1),
            created_by: UserId(1),
            reviewed_by: Some(UserId(1)),
            title: "Draft".into(),
        })
        .unwrap();
        // Doc created by user 2, reviewed by user 1.
        db.insert_doc(Doc {
            id: DocId(2),
            created_by: UserId(2),
            reviewed_by: Some(UserId(1)),
            title: "Report".into(),
        })
        .unwrap();

        // Delete user 1:
        // - cascade on created_by: delete Doc(1) (created_by = user 1)
        // - nullify on reviewed_by: nullify Doc(2).reviewed_by
        db.remove_user(&UserId(1)).unwrap();

        assert_eq!(db.docs.len(), 1);
        let doc2 = db.docs.get(&DocId(2)).unwrap();
        assert_eq!(doc2.created_by, UserId(2));
        assert_eq!(doc2.reviewed_by, None);
    }

    // --- Multiple nullifiable FKs on same row ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct ParentAId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct ParentA {
        #[primary_key]
        pub id: ParentAId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct ParentBId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct ParentB {
        #[primary_key]
        pub id: ParentBId,
        pub name: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct ChildId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Child {
        #[primary_key]
        pub id: ChildId,
        #[indexed]
        pub parent_a: Option<ParentAId>,
        #[indexed]
        pub parent_b: Option<ParentBId>,
        pub label: String,
    }

    #[derive(Database)]
    struct MultiNullifyDb {
        #[table(singular = "parent_a")]
        pub parent_as: ParentATable,

        #[table(singular = "parent_b")]
        pub parent_bs: ParentBTable,

        #[table(singular = "child", fks(parent_a? = "parent_as" on_delete nullify, parent_b? = "parent_bs" on_delete nullify))]
        pub children: ChildTable,
    }

    #[test]
    fn multiple_fks_nullified_independently() {
        let mut db = MultiNullifyDb::new();
        db.insert_parent_a(ParentA {
            id: ParentAId(1),
            name: "A".into(),
        })
        .unwrap();
        db.insert_parent_b(ParentB {
            id: ParentBId(1),
            name: "B".into(),
        })
        .unwrap();
        db.insert_child(Child {
            id: ChildId(1),
            parent_a: Some(ParentAId(1)),
            parent_b: Some(ParentBId(1)),
            label: "C1".into(),
        })
        .unwrap();

        // Delete parent A -- nullifies parent_a FK.
        db.remove_parent_a(&ParentAId(1)).unwrap();
        let child = db.children.get(&ChildId(1)).unwrap();
        assert_eq!(child.parent_a, None);
        assert_eq!(child.parent_b, Some(ParentBId(1)));

        // Delete parent B -- nullifies parent_b FK.
        db.remove_parent_b(&ParentBId(1)).unwrap();
        let child = db.children.get(&ChildId(1)).unwrap();
        assert_eq!(child.parent_a, None);
        assert_eq!(child.parent_b, None);

        // Child still exists.
        assert_eq!(db.children.len(), 1);
    }

    // --- modify_each_by on indexed FK field caught by debug assertions ---

    #[cfg(debug_assertions)]
    #[test]
    fn modify_each_on_indexed_fk_field_caught_by_debug_assertions() {
        let mut db = PetDb::new();
        db.insert_owner(Owner {
            id: OwnerId(1),
            name: "Alice".into(),
        })
        .unwrap();
        db.insert_pet(Pet {
            id: PetId(1),
            owner: Some(OwnerId(1)),
            name: "Fluffy".into(),
        })
        .unwrap();
        db.insert_pet(Pet {
            id: PetId(2),
            owner: Some(OwnerId(1)),
            name: "Buddy".into(),
        })
        .unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            db.pets
                .modify_each_by_owner(&Some(OwnerId(1)), QueryOpts::ASC, |_pk, p| {
                    p.owner = Some(OwnerId(999));
                })
        }));

        // Debug assertions catch the indexed field change.
        assert!(result.is_err());

        // modify_each on non-indexed fields works fine through the table.
        let count = db
            .pets
            .modify_each_by_owner(&Some(OwnerId(1)), QueryOpts::ASC, |_pk, p| {
                p.name = "batch-renamed".into();
            });
        assert_eq!(count, 2);
        assert_eq!(db.pets.get(&PetId(1)).unwrap().name, "batch-renamed");
        assert_eq!(db.pets.get(&PetId(2)).unwrap().name, "batch-renamed");
    }

    // --- Cascade after modify_each ---

    #[test]
    fn cascade_after_modify_each() {
        let mut db = FiltCascadeDb::new();
        db.insert_proj(Proj {
            id: ProjId(1),
            name: "Alpha".into(),
        })
        .unwrap();
        db.insert_filt_task(FiltTask {
            id: TaskId(1),
            project: ProjId(1),
            priority: 5,
            active: true,
        })
        .unwrap();
        db.insert_filt_task(FiltTask {
            id: TaskId(2),
            project: ProjId(1),
            priority: 3,
            active: true,
        })
        .unwrap();

        // Batch modify non-indexed field.
        db.filt_tasks
            .modify_each_by_project(&ProjId(1), QueryOpts::ASC, |_pk, row| {
                row.active = false;
            });

        // FK graph should still be consistent -- cascade should work.
        db.remove_proj(&ProjId(1)).unwrap();
        assert!(db.filt_tasks.is_empty());
    }
}
