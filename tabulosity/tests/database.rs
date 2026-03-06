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
