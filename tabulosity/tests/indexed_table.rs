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
