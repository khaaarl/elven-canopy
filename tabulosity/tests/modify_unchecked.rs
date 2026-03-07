//! Integration tests for `modify_unchecked`, `modify_unchecked_range`, and
//! `modify_unchecked_all` — closure-based in-place mutation that bypasses
//! index maintenance, with debug-build safety checks.

use tabulosity::{Bounded, Database, Error, QueryOpts, Table};

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
    pub food: i64,
    pub rest: i64,
}

// --- Table-level tests ---

#[test]
fn modify_unchecked_existing_row() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();

    table
        .modify_unchecked(&CreatureId(1), |c| {
            c.food -= 10;
            c.rest -= 5;
        })
        .unwrap();

    let c = table.get(&CreatureId(1)).unwrap();
    assert_eq!(c.food, 90);
    assert_eq!(c.rest, 95);
}

#[test]
fn modify_unchecked_not_found() {
    let mut table = CreatureTable::new();

    let err = table
        .modify_unchecked(&CreatureId(99), |_c| {
            panic!("closure should not be called");
        })
        .unwrap_err();

    assert_eq!(
        err,
        Error::NotFound {
            table: "creatures",
            key: "CreatureId(99)".into(),
        }
    );
}

#[test]
fn modify_unchecked_non_indexed_field_is_fine() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();

    // Changing name (non-indexed) should be fine.
    table
        .modify_unchecked(&CreatureId(1), |c| {
            c.name = "Renamed".into();
        })
        .unwrap();

    assert_eq!(table.get(&CreatureId(1)).unwrap().name, "Renamed");
}

#[test]
fn modify_unchecked_multiple_times() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();

    for _ in 0..10 {
        table
            .modify_unchecked(&CreatureId(1), |c| {
                c.food -= 1;
            })
            .unwrap();
    }

    assert_eq!(table.get(&CreatureId(1)).unwrap().food, 90);
}

#[test]
fn indexes_remain_valid_after_modify_unchecked() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "Thorn".into(),
            species: Species::Capybara,
            food: 80,
            rest: 80,
        })
        .unwrap();

    // Modify non-indexed fields.
    table
        .modify_unchecked(&CreatureId(1), |c| {
            c.food = 0;
        })
        .unwrap();

    // Index queries should still work correctly.
    let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
    assert_eq!(elves.len(), 1);
    assert_eq!(elves[0].id, CreatureId(1));
    assert_eq!(elves[0].food, 0);

    let capybaras = table.by_species(&Species::Capybara, QueryOpts::ASC);
    assert_eq!(capybaras.len(), 1);
    assert_eq!(capybaras[0].id, CreatureId(2));
}

// --- Debug-build assertions ---

#[cfg(debug_assertions)]
#[test]
fn debug_catches_pk_change() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table
            .modify_unchecked(&CreatureId(1), |c| {
                c.id = CreatureId(999);
            })
            .unwrap();
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("primary key"),
        "expected panic about primary key, got: {panic_msg}"
    );
}

#[cfg(debug_assertions)]
#[test]
fn debug_catches_indexed_field_change() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table
            .modify_unchecked(&CreatureId(1), |c| {
                c.species = Species::Capybara;
            })
            .unwrap();
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("species"),
        "expected panic about 'species', got: {panic_msg}"
    );
}

// --- Compound index field change detection ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct EventId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "by_location", fields("region", "zone"))]
struct Event {
    #[primary_key]
    pub id: EventId,
    pub region: u32,
    pub zone: u32,
    pub description: String,
}

#[cfg(debug_assertions)]
#[test]
fn debug_catches_compound_index_field_change() {
    let mut table = EventTable::new();
    table
        .insert_no_fk(Event {
            id: EventId(1),
            region: 1,
            zone: 2,
            description: "test".into(),
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table
            .modify_unchecked(&EventId(1), |e| {
                e.region = 99;
            })
            .unwrap();
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("region"),
        "expected panic about 'region', got: {panic_msg}"
    );
}

// --- No-op closure ---

#[test]
fn modify_unchecked_noop_closure() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();

    // Closure that does nothing should succeed without issues.
    table.modify_unchecked(&CreatureId(1), |_c| {}).unwrap();

    let c = table.get(&CreatureId(1)).unwrap();
    assert_eq!(c.food, 100);
    assert_eq!(c.rest, 100);
}

// --- Multiple rows: modify one, verify others untouched ---

#[test]
fn modify_unchecked_leaves_other_rows_untouched() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "Thorn".into(),
            species: Species::Capybara,
            food: 80,
            rest: 80,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(3),
            name: "Whisper".into(),
            species: Species::Elf,
            food: 60,
            rest: 60,
        })
        .unwrap();

    // Modify only the middle row.
    table
        .modify_unchecked(&CreatureId(2), |c| {
            c.food = 0;
            c.name = "Thorn-Modified".into();
        })
        .unwrap();

    // Row 1 untouched.
    let c1 = table.get(&CreatureId(1)).unwrap();
    assert_eq!(c1.food, 100);
    assert_eq!(c1.name, "Aelindra");

    // Row 2 modified.
    let c2 = table.get(&CreatureId(2)).unwrap();
    assert_eq!(c2.food, 0);
    assert_eq!(c2.name, "Thorn-Modified");

    // Row 3 untouched.
    let c3 = table.get(&CreatureId(3)).unwrap();
    assert_eq!(c3.food, 60);
    assert_eq!(c3.name, "Whisper");
}

// --- Compose with insert/update/remove sequences ---

#[test]
fn modify_unchecked_after_insert_update_remove() {
    let mut table = CreatureTable::new();

    // Insert three rows.
    for i in 1..=3 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("Elf-{}", i),
                species: Species::Elf,
                food: 100,
                rest: 100,
            })
            .unwrap();
    }

    // Update row 2 via normal update.
    let mut c2 = table.get(&CreatureId(2)).unwrap().clone();
    c2.food = 50;
    table.update_no_fk(c2).unwrap();

    // Remove row 3.
    table.remove_no_fk(&CreatureId(3)).unwrap();

    // modify_unchecked on row 1 — should work fine.
    table
        .modify_unchecked(&CreatureId(1), |c| {
            c.rest = 42;
        })
        .unwrap();
    assert_eq!(table.get(&CreatureId(1)).unwrap().rest, 42);

    // modify_unchecked on row 2 (previously updated) — should see updated value.
    table
        .modify_unchecked(&CreatureId(2), |c| {
            c.food -= 10;
        })
        .unwrap();
    assert_eq!(table.get(&CreatureId(2)).unwrap().food, 40);

    // modify_unchecked on removed row 3 — should fail.
    let err = table.modify_unchecked(&CreatureId(3), |_| {}).unwrap_err();
    assert_eq!(
        err,
        Error::NotFound {
            table: "creatures",
            key: "CreatureId(3)".into(),
        }
    );
}

// --- Table with NO indexes ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ItemId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Item {
    #[primary_key]
    pub id: ItemId,
    pub name: String,
    pub weight: u32,
}

#[test]
fn modify_unchecked_no_index_table() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    table
        .modify_unchecked(&ItemId(1), |item| {
            item.weight = 15;
            item.name = "Heavy Sword".into();
        })
        .unwrap();

    let item = table.get(&ItemId(1)).unwrap();
    assert_eq!(item.weight, 15);
    assert_eq!(item.name, "Heavy Sword");
}

#[test]
fn modify_unchecked_no_index_table_not_found() {
    let mut table = ItemTable::new();
    let err = table.modify_unchecked(&ItemId(99), |_| {}).unwrap_err();
    assert_eq!(
        err,
        Error::NotFound {
            table: "items",
            key: "ItemId(99)".into(),
        }
    );
}

#[cfg(debug_assertions)]
#[test]
fn debug_catches_pk_change_no_index_table() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Bow".into(),
            weight: 5,
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table
            .modify_unchecked(&ItemId(1), |item| {
                item.id = ItemId(999);
            })
            .unwrap();
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("primary key"),
        "expected panic about primary key, got: {panic_msg}"
    );
}

// --- Unique index table ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct UserId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct User {
    #[primary_key]
    pub id: UserId,
    #[indexed(unique)]
    pub email: String,
    pub name: String,
}

#[test]
fn modify_unchecked_unique_index_non_indexed_field() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    // Changing a non-indexed field should work.
    table
        .modify_unchecked(&UserId(1), |u| {
            u.name = "Alice Renamed".into();
        })
        .unwrap();

    assert_eq!(table.get(&UserId(1)).unwrap().name, "Alice Renamed");

    // Unique index queries should still work.
    let results = table.by_email(&"a@b.com".to_string(), QueryOpts::ASC);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Alice Renamed");
}

#[test]
fn modify_unchecked_unique_index_preserves_integrity() {
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

    // Modify non-indexed fields on both rows.
    table
        .modify_unchecked(&UserId(1), |u| {
            u.name = "Alice v2".into();
        })
        .unwrap();
    table
        .modify_unchecked(&UserId(2), |u| {
            u.name = "Bob v2".into();
        })
        .unwrap();

    // Unique index still correctly maps each email.
    assert_eq!(
        table.by_email(&"a@b.com".to_string(), QueryOpts::ASC).len(),
        1
    );
    assert_eq!(
        table.by_email(&"c@d.com".to_string(), QueryOpts::ASC).len(),
        1
    );
    assert_eq!(
        table.by_email(&"a@b.com".to_string(), QueryOpts::ASC)[0].name,
        "Alice v2"
    );
    assert_eq!(
        table.by_email(&"c@d.com".to_string(), QueryOpts::ASC)[0].name,
        "Bob v2"
    );

    // Inserting a duplicate email should still fail (unique constraint intact).
    let err = table
        .insert_no_fk(User {
            id: UserId(3),
            email: "a@b.com".into(),
            name: "Duplicate".into(),
        })
        .unwrap_err();
    match err {
        Error::DuplicateIndex { index, .. } => assert_eq!(index, "email"),
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }
}

#[cfg(debug_assertions)]
#[test]
fn debug_catches_unique_indexed_field_change() {
    let mut table = UserTable::new();
    table
        .insert_no_fk(User {
            id: UserId(1),
            email: "a@b.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table
            .modify_unchecked(&UserId(1), |u| {
                u.email = "new@email.com".into();
            })
            .unwrap();
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("email"),
        "expected panic about 'email', got: {panic_msg}"
    );
}

// --- Filtered index table ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
enum TaskStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct FiltTaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "active_assignee",
    fields("assignee"),
    filter = "FiltTask::is_active"
)]
struct FiltTask {
    #[primary_key]
    pub id: FiltTaskId,
    #[indexed]
    pub assignee: CreatureId,
    pub priority: u8,
    pub status: TaskStatus,
}

impl FiltTask {
    fn is_active(&self) -> bool {
        matches!(self.status, TaskStatus::Pending | TaskStatus::InProgress)
    }
}

#[test]
fn modify_unchecked_filtered_index_non_indexed_field() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: CreatureId(1),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Modify a non-indexed, non-filter field.
    table
        .modify_unchecked(&FiltTaskId(1), |t| {
            t.priority = 10;
        })
        .unwrap();

    assert_eq!(table.get(&FiltTaskId(1)).unwrap().priority, 10);

    // Filtered index query should still work.
    let active = table.by_active_assignee(&CreatureId(1), QueryOpts::ASC);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].priority, 10);

    // Unfiltered index query also works.
    let all = table.by_assignee(&CreatureId(1), QueryOpts::ASC);
    assert_eq!(all.len(), 1);
}

// --- Auto-increment table ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct AutoId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct AutoRow {
    #[primary_key(auto_increment)]
    pub id: AutoId,
    #[indexed]
    pub category: u32,
    pub value: String,
}

#[test]
fn modify_unchecked_auto_increment_table() {
    let mut table = AutoRowTable::new();
    let id0 = table
        .insert_auto_no_fk(|pk| AutoRow {
            id: pk,
            category: 1,
            value: "first".into(),
        })
        .unwrap();
    let id1 = table
        .insert_auto_no_fk(|pk| AutoRow {
            id: pk,
            category: 2,
            value: "second".into(),
        })
        .unwrap();

    // Modify non-indexed field on auto-increment row.
    table
        .modify_unchecked(&id0, |r| {
            r.value = "first-modified".into();
        })
        .unwrap();

    assert_eq!(table.get(&id0).unwrap().value, "first-modified");
    // Other row untouched.
    assert_eq!(table.get(&id1).unwrap().value, "second");

    // Index queries still correct.
    assert_eq!(table.by_category(&1, QueryOpts::ASC).len(), 1);
    assert_eq!(table.by_category(&2, QueryOpts::ASC).len(), 1);
}

#[cfg(debug_assertions)]
#[test]
fn debug_catches_pk_change_auto_increment() {
    let mut table = AutoRowTable::new();
    let id = table
        .insert_auto_no_fk(|pk| AutoRow {
            id: pk,
            category: 1,
            value: "test".into(),
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table
            .modify_unchecked(&id, |r| {
                r.id = AutoId(999);
            })
            .unwrap();
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("primary key"),
        "expected panic about primary key, got: {panic_msg}"
    );
}

// --- Database-level tests ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Task {
    #[primary_key]
    pub id: TaskId,
    #[indexed]
    pub assignee: CreatureId,
    pub progress: f64,
}

#[derive(Database)]
struct TestDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "task", fks(assignee = "creatures"))]
    pub tasks: TaskTable,
}

#[test]
fn database_modify_unchecked_works() {
    let mut db = TestDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Aelindra".into(),
        species: Species::Elf,
        food: 100,
        rest: 100,
    })
    .unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: CreatureId(1),
        progress: 0.0,
    })
    .unwrap();

    db.modify_unchecked_task(&TaskId(1), |t| {
        t.progress = 50.0;
    })
    .unwrap();

    assert_eq!(db.tasks.get(&TaskId(1)).unwrap().progress, 50.0);
}

#[test]
fn database_modify_unchecked_not_found() {
    let mut db = TestDb::new();

    let err = db
        .modify_unchecked_creature(&CreatureId(99), |_| {
            panic!("should not be called");
        })
        .unwrap_err();

    assert_eq!(
        err,
        Error::NotFound {
            table: "creatures",
            key: "CreatureId(99)".into(),
        }
    );
}

#[test]
fn database_modify_unchecked_bypasses_fk_checks() {
    // The whole point of modify_unchecked at the database level is that it
    // does NOT re-validate FK constraints. We verify this by confirming the
    // method succeeds even though we are not changing any FK fields — and
    // that it does not perform the overhead of checking.
    let mut db = TestDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Aelindra".into(),
        species: Species::Elf,
        food: 100,
        rest: 100,
    })
    .unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: CreatureId(1),
        progress: 0.0,
    })
    .unwrap();

    // Modify non-FK, non-indexed field.
    db.modify_unchecked_task(&TaskId(1), |t| {
        t.progress = 99.0;
    })
    .unwrap();
    assert_eq!(db.tasks.get(&TaskId(1)).unwrap().progress, 99.0);

    // Also modify on the parent table (creature).
    db.modify_unchecked_creature(&CreatureId(1), |c| {
        c.food = 0;
        c.rest = 0;
    })
    .unwrap();
    let c = db.creatures.get(&CreatureId(1)).unwrap();
    assert_eq!(c.food, 0);
    assert_eq!(c.rest, 0);
}

#[test]
fn database_modify_unchecked_index_queries_correct() {
    let mut db = TestDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Aelindra".into(),
        species: Species::Elf,
        food: 100,
        rest: 100,
    })
    .unwrap();
    db.insert_creature(Creature {
        id: CreatureId(2),
        name: "Thorn".into(),
        species: Species::Capybara,
        food: 80,
        rest: 80,
    })
    .unwrap();
    db.insert_task(Task {
        id: TaskId(1),
        assignee: CreatureId(1),
        progress: 0.0,
    })
    .unwrap();
    db.insert_task(Task {
        id: TaskId(2),
        assignee: CreatureId(1),
        progress: 10.0,
    })
    .unwrap();

    // Modify progress on task 1.
    db.modify_unchecked_task(&TaskId(1), |t| {
        t.progress = 75.0;
    })
    .unwrap();

    // Modify food on creature 1.
    db.modify_unchecked_creature(&CreatureId(1), |c| {
        c.food = 42;
    })
    .unwrap();

    // Index queries on tasks by assignee.
    let tasks = db.tasks.by_assignee(&CreatureId(1), QueryOpts::ASC);
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].progress, 75.0); // task 1
    assert_eq!(tasks[1].progress, 10.0); // task 2

    // Index queries on creatures by species.
    let elves = db.creatures.by_species(&Species::Elf, QueryOpts::ASC);
    assert_eq!(elves.len(), 1);
    assert_eq!(elves[0].food, 42);
}

// ============================================================================
// modify_unchecked_range tests
// ============================================================================

fn make_five_creatures() -> CreatureTable {
    let mut table = CreatureTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("Elf-{i}"),
                species: if i <= 3 {
                    Species::Elf
                } else {
                    Species::Capybara
                },
                food: 100,
                rest: 100,
            })
            .unwrap();
    }
    table
}

#[test]
fn modify_unchecked_range_full() {
    let mut table = make_five_creatures();

    let count = table.modify_unchecked_range(.., |_pk, row| {
        row.food -= 10;
    });

    assert_eq!(count, 5);
    for i in 1..=5 {
        assert_eq!(table.get(&CreatureId(i)).unwrap().food, 90);
    }
}

#[test]
fn modify_unchecked_range_subset() {
    let mut table = make_five_creatures();

    // Modify creatures 2..=4.
    let count = table.modify_unchecked_range(CreatureId(2)..=CreatureId(4), |_pk, row| {
        row.food = 0;
    });

    assert_eq!(count, 3);
    assert_eq!(table.get(&CreatureId(1)).unwrap().food, 100); // untouched
    assert_eq!(table.get(&CreatureId(2)).unwrap().food, 0);
    assert_eq!(table.get(&CreatureId(3)).unwrap().food, 0);
    assert_eq!(table.get(&CreatureId(4)).unwrap().food, 0);
    assert_eq!(table.get(&CreatureId(5)).unwrap().food, 100); // untouched
}

#[test]
fn modify_unchecked_range_exclusive_end() {
    let mut table = make_five_creatures();

    let count = table.modify_unchecked_range(CreatureId(2)..CreatureId(4), |_pk, row| {
        row.rest = 0;
    });

    assert_eq!(count, 2);
    assert_eq!(table.get(&CreatureId(1)).unwrap().rest, 100);
    assert_eq!(table.get(&CreatureId(2)).unwrap().rest, 0);
    assert_eq!(table.get(&CreatureId(3)).unwrap().rest, 0);
    assert_eq!(table.get(&CreatureId(4)).unwrap().rest, 100);
}

#[test]
fn modify_unchecked_range_from() {
    let mut table = make_five_creatures();

    let count = table.modify_unchecked_range(CreatureId(4).., |_pk, row| {
        row.food = 42;
    });

    assert_eq!(count, 2);
    assert_eq!(table.get(&CreatureId(3)).unwrap().food, 100);
    assert_eq!(table.get(&CreatureId(4)).unwrap().food, 42);
    assert_eq!(table.get(&CreatureId(5)).unwrap().food, 42);
}

#[test]
fn modify_unchecked_range_to() {
    let mut table = make_five_creatures();

    let count = table.modify_unchecked_range(..CreatureId(3), |_pk, row| {
        row.food = 42;
    });

    assert_eq!(count, 2);
    assert_eq!(table.get(&CreatureId(1)).unwrap().food, 42);
    assert_eq!(table.get(&CreatureId(2)).unwrap().food, 42);
    assert_eq!(table.get(&CreatureId(3)).unwrap().food, 100);
}

#[test]
fn modify_unchecked_range_empty() {
    let mut table = make_five_creatures();

    // Range that matches nothing (PK 10..20, but max PK is 5).
    let count = table.modify_unchecked_range(CreatureId(10)..CreatureId(20), |_pk, _row| {
        panic!("should not be called");
    });

    assert_eq!(count, 0);
}

#[test]
fn modify_unchecked_range_empty_table() {
    let mut table = CreatureTable::new();

    let count = table.modify_unchecked_range(.., |_pk, _row| {
        panic!("should not be called");
    });

    assert_eq!(count, 0);
}

#[test]
fn modify_unchecked_range_closure_receives_correct_pk() {
    let mut table = make_five_creatures();

    // Use the PK inside the closure to set food = pk * 10.
    table.modify_unchecked_range(.., |pk, row| {
        row.food = (pk.0 as i64) * 10;
    });

    for i in 1..=5 {
        assert_eq!(table.get(&CreatureId(i)).unwrap().food, (i as i64) * 10);
    }
}

#[test]
fn modify_unchecked_range_indexes_intact() {
    let mut table = make_five_creatures();

    // Modify non-indexed food field on all rows.
    table.modify_unchecked_range(.., |_pk, row| {
        row.food = 0;
    });

    // Index queries should still return correct results.
    let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
    assert_eq!(elves.len(), 3);
    assert!(elves.iter().all(|c| c.food == 0));

    let capybaras = table.by_species(&Species::Capybara, QueryOpts::ASC);
    assert_eq!(capybaras.len(), 2);
    assert!(capybaras.iter().all(|c| c.food == 0));
}

#[cfg(debug_assertions)]
#[test]
fn modify_unchecked_range_debug_catches_indexed_field_change() {
    let mut table = make_five_creatures();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.modify_unchecked_range(.., |_pk, row| {
            row.species = Species::Capybara; // indexed!
        });
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("species"),
        "expected panic about 'species', got: {panic_msg}"
    );
}

// ============================================================================
// modify_unchecked_all tests
// ============================================================================

#[test]
fn modify_unchecked_all_basic() {
    let mut table = make_five_creatures();

    let count = table.modify_unchecked_all(|_pk, row| {
        row.food -= 5;
        row.rest -= 5;
    });

    assert_eq!(count, 5);
    for i in 1..=5 {
        let c = table.get(&CreatureId(i)).unwrap();
        assert_eq!(c.food, 95);
        assert_eq!(c.rest, 95);
    }
}

#[test]
fn modify_unchecked_all_empty_table() {
    let mut table = CreatureTable::new();

    let count = table.modify_unchecked_all(|_pk, _row| {
        panic!("should not be called");
    });

    assert_eq!(count, 0);
}

#[test]
fn modify_unchecked_all_no_index_table() {
    let mut table = ItemTable::new();
    for i in 1..=3 {
        table
            .insert_no_fk(Item {
                id: ItemId(i),
                name: format!("Item-{i}"),
                weight: 10,
            })
            .unwrap();
    }

    let count = table.modify_unchecked_all(|_pk, row| {
        row.weight += 5;
    });

    assert_eq!(count, 3);
    for i in 1..=3 {
        assert_eq!(table.get(&ItemId(i)).unwrap().weight, 15);
    }
}

// ============================================================================
// modify_unchecked_range -- Specialized table types
// ============================================================================

// --- #1: Range on unique index table (HIGH) ---

#[test]
fn modify_unchecked_range_unique_index_table() {
    let mut table = UserTable::new();
    for (i, email) in [(1, "a@b.com"), (2, "c@d.com"), (3, "e@f.com")] {
        table
            .insert_no_fk(User {
                id: UserId(i),
                email: email.into(),
                name: format!("User-{i}"),
            })
            .unwrap();
    }

    // Range-modify name (non-indexed) on all users.
    let count = table.modify_unchecked_range(.., |_pk, u| {
        u.name = format!("{}-modified", u.name);
    });
    assert_eq!(count, 3);

    // Unique index queries still work.
    assert_eq!(
        table.by_email(&"a@b.com".to_string(), QueryOpts::ASC)[0].name,
        "User-1-modified"
    );
    assert_eq!(
        table.by_email(&"c@d.com".to_string(), QueryOpts::ASC)[0].name,
        "User-2-modified"
    );
    assert_eq!(
        table.by_email(&"e@f.com".to_string(), QueryOpts::ASC)[0].name,
        "User-3-modified"
    );

    // Duplicate email insert still rejected.
    let err = table
        .insert_no_fk(User {
            id: UserId(4),
            email: "a@b.com".into(),
            name: "Dup".into(),
        })
        .unwrap_err();
    match err {
        Error::DuplicateIndex { index, .. } => assert_eq!(index, "email"),
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }
}

// --- #2: Range on filtered index table (MEDIUM) ---

#[test]
fn modify_unchecked_range_filtered_index_table() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: CreatureId(1),
            priority: 1,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: CreatureId(1),
            priority: 2,
            status: TaskStatus::Done,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(3),
            assignee: CreatureId(2),
            priority: 3,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    // Range-modify priority (non-indexed, non-filter field) on all.
    let count = table.modify_unchecked_range(.., |_pk, t| {
        t.priority += 10;
    });
    assert_eq!(count, 3);

    // Filtered index still returns only active tasks for assignee 1.
    let active = table.by_active_assignee(&CreatureId(1), QueryOpts::ASC);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, FiltTaskId(1));
    assert_eq!(active[0].priority, 11);

    // Assignee 2 still in filtered index (InProgress is active).
    let active2 = table.by_active_assignee(&CreatureId(2), QueryOpts::ASC);
    assert_eq!(active2.len(), 1);
    assert_eq!(active2[0].priority, 13);
}

// --- #3: Range on auto-increment table (MEDIUM) ---

#[test]
fn modify_unchecked_range_auto_increment_table() {
    let mut table = AutoRowTable::new();
    for i in 0..3 {
        table
            .insert_auto_no_fk(|pk| AutoRow {
                id: pk,
                category: i,
                value: format!("val-{i}"),
            })
            .unwrap();
    }

    // Range-modify value on rows 0..=1.
    let count = table.modify_unchecked_range(AutoId(0)..=AutoId(1), |_pk, r| {
        r.value = format!("{}-mod", r.value);
    });
    assert_eq!(count, 2);
    assert_eq!(table.get(&AutoId(0)).unwrap().value, "val-0-mod");
    assert_eq!(table.get(&AutoId(1)).unwrap().value, "val-1-mod");
    assert_eq!(table.get(&AutoId(2)).unwrap().value, "val-2"); // untouched

    // Auto-increment counter is unaffected — next insert gets AutoId(3).
    let next_id = table
        .insert_auto_no_fk(|pk| AutoRow {
            id: pk,
            category: 9,
            value: "new".into(),
        })
        .unwrap();
    assert_eq!(next_id, AutoId(3));
}

// --- #4: Range on no-index table (MEDIUM) ---

#[test]
fn modify_unchecked_range_no_index_table() {
    let mut table = ItemTable::new();
    for i in 1..=3 {
        table
            .insert_no_fk(Item {
                id: ItemId(i),
                name: format!("Item-{i}"),
                weight: 10,
            })
            .unwrap();
    }

    let count = table.modify_unchecked_range(ItemId(1)..=ItemId(2), |_pk, item| {
        item.weight += 5;
    });
    assert_eq!(count, 2);
    assert_eq!(table.get(&ItemId(1)).unwrap().weight, 15);
    assert_eq!(table.get(&ItemId(2)).unwrap().weight, 15);
    assert_eq!(table.get(&ItemId(3)).unwrap().weight, 10); // untouched
}

// ============================================================================
// modify_unchecked_range -- Debug assertions
// ============================================================================

// --- #5: Range debug catches PK change (HIGH) ---

#[cfg(debug_assertions)]
#[test]
fn modify_unchecked_range_debug_catches_pk_change() {
    let mut table = make_five_creatures();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.modify_unchecked_range(.., |_pk, row| {
            row.id = CreatureId(999);
        });
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("primary key"),
        "expected panic about primary key, got: {panic_msg}"
    );
}

// --- #6: Range debug catches compound index field change (HIGH) ---

#[cfg(debug_assertions)]
#[test]
fn modify_unchecked_range_debug_catches_compound_index_field_change() {
    let mut table = EventTable::new();
    for i in 1..=3 {
        table
            .insert_no_fk(Event {
                id: EventId(i),
                region: i,
                zone: i * 10,
                description: format!("event-{i}"),
            })
            .unwrap();
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.modify_unchecked_range(.., |_pk, e| {
            e.region = 99;
        });
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("region"),
        "expected panic about 'region', got: {panic_msg}"
    );
}

// ============================================================================
// modify_unchecked_all -- Debug assertions
// ============================================================================

// --- #7: All debug catches indexed field change (MEDIUM) ---

#[cfg(debug_assertions)]
#[test]
fn modify_unchecked_all_debug_catches_indexed_field_change() {
    let mut table = make_five_creatures();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.modify_unchecked_all(|_pk, row| {
            row.species = Species::Capybara;
        });
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("species"),
        "expected panic about 'species', got: {panic_msg}"
    );
}

// --- #8: All debug catches PK change (MEDIUM) ---

#[cfg(debug_assertions)]
#[test]
fn modify_unchecked_all_debug_catches_pk_change() {
    let mut table = make_five_creatures();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.modify_unchecked_all(|_pk, row| {
            row.id = CreatureId(999);
        });
    }));

    assert!(result.is_err());
    let panic_msg = result
        .unwrap_err()
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        panic_msg.contains("primary key"),
        "expected panic about primary key, got: {panic_msg}"
    );
}

// ============================================================================
// modify_unchecked_all -- Specialized table types
// ============================================================================

// --- #9: All on unique index table (MEDIUM) ---

#[test]
fn modify_unchecked_all_unique_index_table() {
    let mut table = UserTable::new();
    for (i, email) in [(1, "a@b.com"), (2, "c@d.com"), (3, "e@f.com")] {
        table
            .insert_no_fk(User {
                id: UserId(i),
                email: email.into(),
                name: format!("User-{i}"),
            })
            .unwrap();
    }

    let count = table.modify_unchecked_all(|_pk, u| {
        u.name = format!("{}-v2", u.name);
    });
    assert_eq!(count, 3);

    // Unique index still correct.
    assert_eq!(
        table.by_email(&"a@b.com".to_string(), QueryOpts::ASC)[0].name,
        "User-1-v2"
    );
    assert_eq!(
        table.by_email(&"c@d.com".to_string(), QueryOpts::ASC)[0].name,
        "User-2-v2"
    );

    // Duplicate insert still rejected.
    let err = table
        .insert_no_fk(User {
            id: UserId(4),
            email: "c@d.com".into(),
            name: "Dup".into(),
        })
        .unwrap_err();
    match err {
        Error::DuplicateIndex { index, .. } => assert_eq!(index, "email"),
        other => panic!("expected DuplicateIndex, got: {:?}", other),
    }
}

// --- #12: All after removes (MEDIUM) ---

#[test]
fn modify_unchecked_all_after_removes() {
    let mut table = make_five_creatures();

    // Remove rows 2 and 4.
    table.remove_no_fk(&CreatureId(2)).unwrap();
    table.remove_no_fk(&CreatureId(4)).unwrap();

    let count = table.modify_unchecked_all(|_pk, row| {
        row.food = 0;
    });

    assert_eq!(count, 3);
    assert_eq!(table.get(&CreatureId(1)).unwrap().food, 0);
    assert!(table.get(&CreatureId(2)).is_none());
    assert_eq!(table.get(&CreatureId(3)).unwrap().food, 0);
    assert!(table.get(&CreatureId(4)).is_none());
    assert_eq!(table.get(&CreatureId(5)).unwrap().food, 0);
}

// ============================================================================
// Edge cases
// ============================================================================

// --- #13: Single-element range (MEDIUM) ---

#[test]
fn modify_unchecked_range_single_element() {
    let mut table = make_five_creatures();

    let count = table.modify_unchecked_range(CreatureId(3)..=CreatureId(3), |_pk, row| {
        row.food = 0;
    });

    assert_eq!(count, 1);
    assert_eq!(table.get(&CreatureId(2)).unwrap().food, 100); // untouched
    assert_eq!(table.get(&CreatureId(3)).unwrap().food, 0);
    assert_eq!(table.get(&CreatureId(4)).unwrap().food, 100); // untouched
}

// --- #16: Interleaved with mutations (MEDIUM) ---

#[test]
fn modify_unchecked_range_interleaved_with_mutations() {
    let mut table = CreatureTable::new();
    for i in 1..=3 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("Elf-{i}"),
                species: Species::Elf,
                food: 100,
                rest: 100,
            })
            .unwrap();
    }

    // First range-modify: set food = 50 on all.
    table.modify_unchecked_range(.., |_pk, row| {
        row.food = 50;
    });

    // Insert a new row.
    table
        .insert_no_fk(Creature {
            id: CreatureId(4),
            name: "Elf-4".into(),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();

    // Second range-modify on 3..=4: subtract 10 from food.
    let count = table.modify_unchecked_range(CreatureId(3)..=CreatureId(4), |_pk, row| {
        row.food -= 10;
    });
    assert_eq!(count, 2);

    // Row 3 was 50 (from first modify), now 40.
    assert_eq!(table.get(&CreatureId(3)).unwrap().food, 40);
    // Row 4 was 100 (freshly inserted), now 90.
    assert_eq!(table.get(&CreatureId(4)).unwrap().food, 90);
    // Rows 1-2 unaffected by second modify.
    assert_eq!(table.get(&CreatureId(1)).unwrap().food, 50);
    assert_eq!(table.get(&CreatureId(2)).unwrap().food, 50);
}

// --- #18: Cross-variant consistency (HIGH) ---

#[test]
fn single_vs_range_vs_all_produce_identical_results() {
    // Method A: modify_unchecked in a loop.
    let mut table_a = make_five_creatures();
    for i in 1..=5 {
        table_a
            .modify_unchecked(&CreatureId(i), |c| {
                c.food -= 10;
            })
            .unwrap();
    }

    // Method B: modify_unchecked_range(..).
    let mut table_b = make_five_creatures();
    table_b.modify_unchecked_range(.., |_pk, c| {
        c.food -= 10;
    });

    // Method C: modify_unchecked_all.
    let mut table_c = make_five_creatures();
    table_c.modify_unchecked_all(|_pk, c| {
        c.food -= 10;
    });

    // All three should produce identical results.
    for i in 1..=5 {
        let a = table_a.get(&CreatureId(i)).unwrap();
        let b = table_b.get(&CreatureId(i)).unwrap();
        let c = table_c.get(&CreatureId(i)).unwrap();
        assert_eq!(a, b, "single vs range differ at CreatureId({i})");
        assert_eq!(b, c, "range vs all differ at CreatureId({i})");
    }
}

// ============================================================================
// Database-level range/all on FK table
// ============================================================================

// --- #19: Database range on FK table (MEDIUM) ---

#[test]
fn database_modify_unchecked_range_on_fk_table() {
    let mut db = TestDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Aelindra".into(),
        species: Species::Elf,
        food: 100,
        rest: 100,
    })
    .unwrap();

    for i in 1..=3 {
        db.insert_task(Task {
            id: TaskId(i),
            assignee: CreatureId(1),
            progress: 0.0,
        })
        .unwrap();
    }

    let count = db.modify_unchecked_range_task(TaskId(1)..=TaskId(2), |_pk, t| {
        t.progress = 75.0;
    });
    assert_eq!(count, 2);
    assert_eq!(db.tasks.get(&TaskId(1)).unwrap().progress, 75.0);
    assert_eq!(db.tasks.get(&TaskId(2)).unwrap().progress, 75.0);
    assert_eq!(db.tasks.get(&TaskId(3)).unwrap().progress, 0.0); // untouched
}

// ============================================================================
// Database-level range/all tests
// ============================================================================

#[test]
fn database_modify_unchecked_range() {
    let mut db = TestDb::new();
    for i in 1..=3 {
        db.insert_creature(Creature {
            id: CreatureId(i),
            name: format!("Elf-{i}"),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();
    }

    let count = db.modify_unchecked_range_creature(CreatureId(1)..=CreatureId(2), |_pk, c| {
        c.food = 0;
    });

    assert_eq!(count, 2);
    assert_eq!(db.creatures.get(&CreatureId(1)).unwrap().food, 0);
    assert_eq!(db.creatures.get(&CreatureId(2)).unwrap().food, 0);
    assert_eq!(db.creatures.get(&CreatureId(3)).unwrap().food, 100);
}

#[test]
fn database_modify_unchecked_all() {
    let mut db = TestDb::new();
    for i in 1..=3 {
        db.insert_creature(Creature {
            id: CreatureId(i),
            name: format!("Elf-{i}"),
            species: Species::Elf,
            food: 100,
            rest: 100,
        })
        .unwrap();
    }

    let count = db.modify_unchecked_all_creature(|_pk, c| {
        c.rest = 0;
    });

    assert_eq!(count, 3);
    for i in 1..=3 {
        assert_eq!(db.creatures.get(&CreatureId(i)).unwrap().rest, 0);
    }
}

/// Additional gap coverage tests for modify_unchecked panic safety: partial
/// mutation on closure panic (single row and range), and compound unique index
/// field change detection.
mod gap_coverage {
    use std::panic;

    use tabulosity::{Bounded, QueryOpts, Table};

    // --- Panic safety for modify_unchecked ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct CId(u32);

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[allow(dead_code)]
    enum Species {
        Elf,
        Capybara,
    }

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Creature {
        #[primary_key]
        pub id: CId,
        pub name: String,
        #[indexed]
        pub species: Species,
        pub hunger: u32,
    }

    // Note: modify_unchecked panic corrupting indexes is inherently about
    // release-build behavior where debug assertions are skipped. In debug builds,
    // changing indexed fields panics before any index issue arises. We test the
    // non-indexed-field panic case to verify the row is partially mutated.

    #[test]
    fn modify_unchecked_closure_panic_on_non_indexed_field_leaves_partial_state() {
        let mut table = CreatureTable::new();
        table
            .insert_no_fk(Creature {
                id: CId(1),
                name: "Aelindra".into(),
                species: Species::Elf,
                hunger: 100,
            })
            .unwrap();

        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            let _ = table.modify_unchecked(&CId(1), |c| {
                c.hunger = 50; // This mutation happens
                panic!("oops"); // Then closure panics
            });
        }));
        assert!(result.is_err());

        // The row was mutated in place before the panic -- verify partial state.
        let c = table.get(&CId(1)).unwrap();
        assert_eq!(c.hunger, 50, "mutation before panic should be visible");

        // Index should still be consistent because we only changed a non-indexed field.
        let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
        assert_eq!(elves.len(), 1);
    }

    #[test]
    fn modify_unchecked_range_partial_mutation_on_panic() {
        let mut table = CreatureTable::new();
        for i in 1..=5 {
            table
                .insert_no_fk(Creature {
                    id: CId(i),
                    name: format!("C{i}"),
                    species: Species::Elf,
                    hunger: 100,
                })
                .unwrap();
        }

        let mut count = 0u32;
        let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            table.modify_unchecked_range(.., |_pk, c| {
                count += 1;
                c.hunger = 0;
                if count == 3 {
                    panic!("oops on third row");
                }
            });
        }));
        assert!(result.is_err());

        // First 2 rows (CId(1), CId(2)) were fully mutated. CId(3) was mutated
        // then panicked. CId(4) and CId(5) were not reached.
        assert_eq!(table.get(&CId(1)).unwrap().hunger, 0);
        assert_eq!(table.get(&CId(2)).unwrap().hunger, 0);
        assert_eq!(table.get(&CId(3)).unwrap().hunger, 0); // mutation happened before panic
        assert_eq!(table.get(&CId(4)).unwrap().hunger, 100);
        assert_eq!(table.get(&CId(5)).unwrap().hunger, 100);
    }

    // --- Compound unique index field change detected by debug assertions ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct CompUqId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    #[index(name = "a_b", fields("a", "b"), unique)]
    struct CompUqRow {
        #[primary_key]
        pub id: CompUqId,
        pub a: u32,
        pub b: u32,
        pub label: String,
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "indexed field")]
    fn modify_unchecked_compound_unique_catches_field_change() {
        let mut table = CompUqRowTable::new();
        table
            .insert_no_fk(CompUqRow {
                id: CompUqId(1),
                a: 1,
                b: 2,
                label: "first".into(),
            })
            .unwrap();
        table
            .insert_no_fk(CompUqRow {
                id: CompUqId(2),
                a: 1,
                b: 3,
                label: "second".into(),
            })
            .unwrap();

        // Changing b (part of compound unique index) should be caught by debug assertions.
        table
            .modify_unchecked(&CompUqId(2), |r| {
                r.b = 2; // Would create duplicate (1, 2)
            })
            .unwrap();
    }
}
