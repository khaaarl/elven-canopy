//! Integration tests for `#[derive(Table)]` with secondary indexes.

use tabulosity::{Bounded, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CreatureId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Species {
    Elf,
    Capybara,
    Squirrel,
}

#[derive(Table, Clone, Debug, PartialEq)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
    #[indexed]
    pub species: Species,
    #[indexed]
    pub hunger: u32,
}

// --- Equality queries ---

#[test]
fn by_indexed_field_equality() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "Thorn".into(),
            species: Species::Capybara,
            hunger: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(3),
            name: "Whisper".into(),
            species: Species::Elf,
            hunger: 5,
        })
        .unwrap();

    let elves = table.by_species(&Species::Elf);
    assert_eq!(elves.len(), 2);
    assert_eq!(elves[0].id, CreatureId(1));
    assert_eq!(elves[1].id, CreatureId(3));

    let capybaras = table.by_species(&Species::Capybara);
    assert_eq!(capybaras.len(), 1);
    assert_eq!(capybaras[0].name, "Thorn");

    let squirrels = table.by_species(&Species::Squirrel);
    assert!(squirrels.is_empty());
}

#[test]
fn iter_by_indexed_field() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Elf,
            hunger: 5,
        })
        .unwrap();

    let names: Vec<_> = table
        .iter_by_species(&Species::Elf)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["A", "B"]);
}

#[test]
fn count_by_indexed_field() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Capybara,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(3),
            name: "C".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 2);
    assert_eq!(table.count_by_species(&Species::Capybara), 1);
    assert_eq!(table.count_by_species(&Species::Squirrel), 0);
}

// --- Range queries ---

#[test]
fn by_range_inclusive_start() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // hunger >= 20
    let hungry = table.by_hunger_range(20..);
    assert_eq!(hungry.len(), 3);
    assert_eq!(hungry[0].hunger, 20);
    assert_eq!(hungry[1].hunger, 30);
    assert_eq!(hungry[2].hunger, 40);
}

#[test]
fn by_range_exclusive_end() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // hunger < 30
    let not_hungry = table.by_hunger_range(..30);
    assert_eq!(not_hungry.len(), 3);
    assert_eq!(not_hungry[0].hunger, 0);
    assert_eq!(not_hungry[1].hunger, 10);
    assert_eq!(not_hungry[2].hunger, 20);
}

#[test]
fn by_range_inclusive_end() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // hunger <= 20
    let result = table.by_hunger_range(..=20);
    assert_eq!(result.len(), 3);
}

#[test]
fn by_range_closed() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // 10 <= hunger <= 30
    let result = table.by_hunger_range(10..=30);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].hunger, 10);
    assert_eq!(result[1].hunger, 20);
    assert_eq!(result[2].hunger, 30);
}

#[test]
fn count_by_range() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    assert_eq!(table.count_by_hunger_range(20..), 3);
    assert_eq!(table.count_by_hunger_range(..30), 3);
    assert_eq!(table.count_by_hunger_range(..), 5);
}

#[test]
fn iter_by_range() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    let ids: Vec<_> = table.iter_by_hunger_range(10..30).map(|c| c.id).collect();
    assert_eq!(ids, vec![CreatureId(1), CreatureId(2)]);
}

// --- Index maintenance on mutations ---

#[test]
fn index_maintained_on_update() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_species(&Species::Capybara), 0);

    table
        .update_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Capybara,
            hunger: 0,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 0);
    assert_eq!(table.count_by_species(&Species::Capybara), 1);
}

#[test]
fn index_maintained_on_upsert() {
    let mut table = CreatureTable::new();
    table.upsert_no_fk(Creature {
        id: CreatureId(1),
        name: "A".into(),
        species: Species::Elf,
        hunger: 0,
    });

    assert_eq!(table.count_by_species(&Species::Elf), 1);

    table.upsert_no_fk(Creature {
        id: CreatureId(1),
        name: "A".into(),
        species: Species::Capybara,
        hunger: 0,
    });

    assert_eq!(table.count_by_species(&Species::Elf), 0);
    assert_eq!(table.count_by_species(&Species::Capybara), 1);
}

#[test]
fn index_maintained_on_remove() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_hunger(&10), 1);

    table.remove_no_fk(&CreatureId(1)).unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 0);
    assert_eq!(table.count_by_hunger(&10), 0);
}

#[test]
fn rebuild_indexes() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 5,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Capybara,
            hunger: 10,
        })
        .unwrap();

    table.rebuild_indexes();

    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_species(&Species::Capybara), 1);
    assert_eq!(table.count_by_hunger(&5), 1);
    assert_eq!(table.count_by_hunger(&10), 1);
}

// --- Option<T> indexed fields ---

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

#[test]
fn by_range_excluded_start() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // hunger in (10, 40] — excludes 10, includes 40
    use std::ops::Bound;
    let result = table.by_hunger_range((Bound::Excluded(10), Bound::Included(40)));
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].hunger, 20);
    assert_eq!(result[1].hunger, 30);
    assert_eq!(result[2].hunger, 40);
}

#[test]
fn by_range_excluded_both() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // hunger in (10, 40) — excludes both endpoints
    use std::ops::Bound;
    let result = table.by_hunger_range((Bound::Excluded(10), Bound::Excluded(40)));
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].hunger, 20);
    assert_eq!(result[1].hunger, 30);
}

#[test]
fn iter_by_range_multiple_forms() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{}", i),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // RangeFrom: hunger >= 20
    let ids: Vec<_> = table.iter_by_hunger_range(20..).map(|c| c.hunger).collect();
    assert_eq!(ids, vec![20, 30, 40]);

    // RangeToInclusive: hunger <= 20
    let ids: Vec<_> = table
        .iter_by_hunger_range(..=20)
        .map(|c| c.hunger)
        .collect();
    assert_eq!(ids, vec![0, 10, 20]);

    // RangeInclusive: 10 <= hunger <= 30
    let ids: Vec<_> = table
        .iter_by_hunger_range(10..=30)
        .map(|c| c.hunger)
        .collect();
    assert_eq!(ids, vec![10, 20, 30]);

    // RangeFull: all
    let count = table.iter_by_hunger_range(..).count();
    assert_eq!(count, 5);
}

// --- Upsert with multiple indexed fields changing ---

#[test]
fn upsert_updates_multiple_indexed_fields() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_hunger(&10), 1);

    // Upsert changing both species AND hunger simultaneously.
    table.upsert_no_fk(Creature {
        id: CreatureId(1),
        name: "A".into(),
        species: Species::Capybara,
        hunger: 50,
    });

    // Old index entries removed, new ones added.
    assert_eq!(table.count_by_species(&Species::Elf), 0);
    assert_eq!(table.count_by_species(&Species::Capybara), 1);
    assert_eq!(table.count_by_hunger(&10), 0);
    assert_eq!(table.count_by_hunger(&50), 1);
}

// --- Option<T> indexed field iterator ---

#[test]
fn option_indexed_field_none_and_some() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 5,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(2),
            assignee: Some(CreatureId(10)),
            priority: 3,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(3),
            assignee: Some(CreatureId(10)),
            priority: 1,
        })
        .unwrap();

    // Query by None
    let unassigned = table.by_assignee(&None);
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].id, TaskId(1));

    // Query by Some
    let assigned_to_10 = table.by_assignee(&Some(CreatureId(10)));
    assert_eq!(assigned_to_10.len(), 2);
    assert_eq!(assigned_to_10[0].id, TaskId(2));
    assert_eq!(assigned_to_10[1].id, TaskId(3));

    // Count
    assert_eq!(table.count_by_assignee(&None), 1);
    assert_eq!(table.count_by_assignee(&Some(CreatureId(10))), 2);
    assert_eq!(table.count_by_assignee(&Some(CreatureId(99))), 0);
}

#[test]
fn iter_by_option_indexed_field() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 5,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(2),
            assignee: Some(CreatureId(10)),
            priority: 3,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(3),
            assignee: Some(CreatureId(10)),
            priority: 1,
        })
        .unwrap();

    // iter_by_assignee with None
    let unassigned: Vec<_> = table.iter_by_assignee(&None).collect();
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].id, TaskId(1));

    // iter_by_assignee with Some
    let assigned: Vec<_> = table.iter_by_assignee(&Some(CreatureId(10))).collect();
    assert_eq!(assigned.len(), 2);
    assert_eq!(assigned[0].id, TaskId(2));
    assert_eq!(assigned[1].id, TaskId(3));

    // iter_by_assignee with nonexistent value
    let empty: Vec<_> = table.iter_by_assignee(&Some(CreatureId(99))).collect();
    assert!(empty.is_empty());
}

// --- 3a: Option<T> indexed field mutation paths ---

#[test]
fn update_optional_index_some_to_none() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: Some(CreatureId(10)),
            priority: 5,
        })
        .unwrap();

    assert_eq!(table.count_by_assignee(&Some(CreatureId(10))), 1);
    assert_eq!(table.count_by_assignee(&None), 0);

    table
        .update_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 5,
        })
        .unwrap();

    assert_eq!(table.count_by_assignee(&Some(CreatureId(10))), 0);
    assert_eq!(table.count_by_assignee(&None), 1);
    assert_eq!(table.get(&TaskId(1)).unwrap().assignee, None);
}

#[test]
fn update_optional_index_none_to_some() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 5,
        })
        .unwrap();

    assert_eq!(table.count_by_assignee(&None), 1);
    assert_eq!(table.count_by_assignee(&Some(CreatureId(20))), 0);

    table
        .update_no_fk(Task {
            id: TaskId(1),
            assignee: Some(CreatureId(20)),
            priority: 5,
        })
        .unwrap();

    assert_eq!(table.count_by_assignee(&None), 0);
    assert_eq!(table.count_by_assignee(&Some(CreatureId(20))), 1);
    assert_eq!(
        table.get(&TaskId(1)).unwrap().assignee,
        Some(CreatureId(20))
    );
}

#[test]
fn remove_row_with_none_optional_index() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 5,
        })
        .unwrap();

    assert_eq!(table.count_by_assignee(&None), 1);

    table.remove_no_fk(&TaskId(1)).unwrap();

    assert_eq!(table.count_by_assignee(&None), 0);
    assert!(table.is_empty());
}

// --- 3b: Duplicate insert on indexed table leaves indexes clean ---

#[test]
fn duplicate_insert_indexed_table_preserves_indexes() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_hunger(&10), 1);

    // Attempt duplicate insert — should fail.
    let err = table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "B".into(),
            species: Species::Capybara,
            hunger: 20,
        })
        .unwrap_err();
    assert!(matches!(err, tabulosity::Error::DuplicateKey { .. }));

    // Index counts must be unchanged — no ghost entries from the failed insert.
    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_species(&Species::Capybara), 0);
    assert_eq!(table.count_by_hunger(&10), 1);
    assert_eq!(table.count_by_hunger(&20), 0);
}

// --- 3c: Partial removal from shared index bucket ---

#[test]
fn remove_one_from_shared_index_bucket() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Elf,
            hunger: 5,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 2);

    // Remove one of the two elves.
    table.remove_no_fk(&CreatureId(1)).unwrap();

    let elves = table.by_species(&Species::Elf);
    assert_eq!(elves.len(), 1);
    assert_eq!(elves[0].id, CreatureId(2));
    assert_eq!(elves[0].name, "B");
}

// --- 5a: update_no_fk does not mutate indexes when indexed field unchanged ---

#[test]
fn update_unchanged_indexed_fields_preserves_index_counts() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "Aelindra".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_hunger(&10), 1);

    // Update only the name — species and hunger stay the same.
    table
        .update_no_fk(Creature {
            id: CreatureId(1),
            name: "Whisper".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();

    // Index counts should be unchanged (still 1, not 0 or 2).
    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_species(&Species::Capybara), 0);
    assert_eq!(table.count_by_hunger(&10), 1);

    // Verify the row actually updated.
    assert_eq!(table.get(&CreatureId(1)).unwrap().name, "Whisper");
}

// --- 5b: rebuild_indexes idempotency ---

#[test]
fn rebuild_indexes_idempotent() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 5,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Capybara,
            hunger: 10,
        })
        .unwrap();

    // Rebuild twice in a row — counts must not double.
    table.rebuild_indexes();
    table.rebuild_indexes();

    assert_eq!(table.count_by_species(&Species::Elf), 1);
    assert_eq!(table.count_by_species(&Species::Capybara), 1);
    assert_eq!(table.count_by_hunger(&5), 1);
    assert_eq!(table.count_by_hunger(&10), 1);
}

// --- 5c: Range queries with duplicate field values at boundary ---

#[test]
fn range_query_duplicate_values_at_boundary() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(3),
            name: "C".into(),
            species: Species::Elf,
            hunger: 20,
        })
        .unwrap();

    // 10..20 (exclusive end) — should return both hunger=10 rows, not hunger=20.
    let result = table.by_hunger_range(10..20);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, CreatureId(1));
    assert_eq!(result[1].id, CreatureId(2));

    // 10..=10 (inclusive both) — should return both hunger=10 rows.
    let result = table.by_hunger_range(10..=10);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, CreatureId(1));
    assert_eq!(result[1].id, CreatureId(2));

    // ..=10 (unbounded start, inclusive end) — should return both hunger=10 rows.
    let count = table.count_by_hunger_range(..=10);
    assert_eq!(count, 2);
}

// --- 5d: by_field equality query with non-contiguous PKs ---

#[test]
fn by_field_equality_non_contiguous_pks() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(50),
            name: "B".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(100),
            name: "C".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();

    let elves = table.by_species(&Species::Elf);
    assert_eq!(elves.len(), 3);
    assert_eq!(elves[0].id, CreatureId(1));
    assert_eq!(elves[1].id, CreatureId(50));
    assert_eq!(elves[2].id, CreatureId(100));
}

// --- 3d: Range queries on Option<T> indexed fields ---

#[test]
fn option_indexed_range_queries() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 1,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(2),
            assignee: Some(CreatureId(5)),
            priority: 2,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(3),
            assignee: Some(CreatureId(10)),
            priority: 3,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(4),
            assignee: Some(CreatureId(20)),
            priority: 4,
        })
        .unwrap();

    // Full range — all rows.
    let all = table.by_assignee_range(..);
    assert_eq!(all.len(), 4);

    // None < Some(..) ordering: range starting from None captures everything.
    let from_none = table.by_assignee_range(None..);
    assert_eq!(from_none.len(), 4);
    assert_eq!(from_none[0].id, TaskId(1)); // None first

    // Range that captures only Some values.
    let some_only = table.by_assignee_range(Some(CreatureId(0))..);
    assert_eq!(some_only.len(), 3);
    assert_eq!(some_only[0].id, TaskId(2));

    // Closed range within Some values.
    let mid = table.by_assignee_range(Some(CreatureId(5))..=Some(CreatureId(10)));
    assert_eq!(mid.len(), 2);
    assert_eq!(mid[0].id, TaskId(2));
    assert_eq!(mid[1].id, TaskId(3));

    // Range up to (exclusive) Some(CreatureId(10)) — should get None + Some(5).
    let up_to = table.by_assignee_range(..Some(CreatureId(10)));
    assert_eq!(up_to.len(), 2);
    assert_eq!(up_to[0].id, TaskId(1)); // None
    assert_eq!(up_to[1].id, TaskId(2)); // Some(5)
}

#[test]
fn option_indexed_iter_range() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 1,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(2),
            assignee: Some(CreatureId(5)),
            priority: 2,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(3),
            assignee: Some(CreatureId(10)),
            priority: 3,
        })
        .unwrap();

    let ids: Vec<_> = table
        .iter_by_assignee_range(Some(CreatureId(5))..=Some(CreatureId(10)))
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, vec![TaskId(2), TaskId(3)]);
}

#[test]
fn option_indexed_count_range() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 1,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(2),
            assignee: Some(CreatureId(5)),
            priority: 2,
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            id: TaskId(3),
            assignee: Some(CreatureId(10)),
            priority: 3,
        })
        .unwrap();

    assert_eq!(table.count_by_assignee_range(..), 3);
    assert_eq!(
        table.count_by_assignee_range(Some(CreatureId(5))..=Some(CreatureId(10))),
        2
    );
    assert_eq!(table.count_by_assignee_range(..Some(CreatureId(0))), 1); // Only None
}

// --- 4a: Range query returning zero results ---

#[test]
fn range_query_no_matches() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 10,
        })
        .unwrap();

    let result = table.by_hunger_range(1000..);
    assert!(result.is_empty());

    let count = table.count_by_hunger_range(1000..);
    assert_eq!(count, 0);

    let iter_count = table.iter_by_hunger_range(1000..).count();
    assert_eq!(iter_count, 0);
}

// --- 4b: Range queries on empty tables ---

#[test]
fn range_queries_on_empty_table() {
    let table = CreatureTable::new();

    let result = table.by_hunger_range(..);
    assert!(result.is_empty());

    let count = table.count_by_hunger_range(..);
    assert_eq!(count, 0);

    let iter_count = table.iter_by_hunger_range(..).count();
    assert_eq!(iter_count, 0);
}

// --- 4c: iter_by PK ordering with out-of-order insertion ---

#[test]
fn iter_by_returns_pk_order_regardless_of_insert_order() {
    let mut table = CreatureTable::new();
    // Insert out of PK order: 3, 1, 2
    table
        .insert_no_fk(Creature {
            id: CreatureId(3),
            name: "C".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();

    let ids: Vec<_> = table.iter_by_species(&Species::Elf).map(|c| c.id).collect();
    assert_eq!(ids, vec![CreatureId(1), CreatureId(2), CreatureId(3)]);
}
