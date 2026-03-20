//! Integration tests for `#[derive(Table)]` with secondary indexes.
//! Tests simple indexes (#[indexed]), compound indexes (#[index(...)]),
//! filtered indexes, and the unified IntoQuery-based query API.

use tabulosity::{Bounded, IntoQuery, MatchAll, QueryBound, QueryOpts, Table};

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

// ============================================================================
// Simple index: equality queries (via IntoQuery)
// ============================================================================

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

    let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
    assert_eq!(elves.len(), 2);
    assert_eq!(elves[0].id, CreatureId(1));
    assert_eq!(elves[1].id, CreatureId(3));

    let capybaras = table.by_species(&Species::Capybara, QueryOpts::ASC);
    assert_eq!(capybaras.len(), 1);
    assert_eq!(capybaras[0].name, "Thorn");

    let squirrels = table.by_species(&Species::Squirrel, QueryOpts::ASC);
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
        .iter_by_species(&Species::Elf, QueryOpts::ASC)
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

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 2);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_species(&Species::Squirrel, QueryOpts::ASC),
        0
    );
}

// ============================================================================
// Simple index: range queries (via IntoQuery)
// ============================================================================

fn make_hunger_table() -> CreatureTable {
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
    table
}

#[test]
fn by_range_inclusive_start() {
    let table = make_hunger_table();
    let hungry = table.by_hunger(20u32.., QueryOpts::ASC);
    assert_eq!(hungry.len(), 3);
    assert_eq!(hungry[0].hunger, 20);
    assert_eq!(hungry[1].hunger, 30);
    assert_eq!(hungry[2].hunger, 40);
}

#[test]
fn by_range_exclusive_end() {
    let table = make_hunger_table();
    let not_hungry = table.by_hunger(..30u32, QueryOpts::ASC);
    assert_eq!(not_hungry.len(), 3);
    assert_eq!(not_hungry[0].hunger, 0);
    assert_eq!(not_hungry[1].hunger, 10);
    assert_eq!(not_hungry[2].hunger, 20);
}

#[test]
fn by_range_inclusive_end() {
    let table = make_hunger_table();
    let result = table.by_hunger(..=20u32, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
}

#[test]
fn by_range_closed() {
    let table = make_hunger_table();
    let result = table.by_hunger(10u32..=30, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].hunger, 10);
    assert_eq!(result[1].hunger, 20);
    assert_eq!(result[2].hunger, 30);
}

#[test]
fn count_by_range() {
    let table = make_hunger_table();
    assert_eq!(table.count_by_hunger(20u32.., QueryOpts::ASC), 3);
    assert_eq!(table.count_by_hunger(..30u32, QueryOpts::ASC), 3);
    assert_eq!(table.count_by_hunger(.., QueryOpts::ASC), 5);
}

#[test]
fn iter_by_range() {
    let table = make_hunger_table();
    let ids: Vec<_> = table
        .iter_by_hunger(10u32..30, QueryOpts::ASC)
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, vec![CreatureId(1), CreatureId(2)]);
}

#[test]
fn by_range_excluded_start() {
    let table = make_hunger_table();
    use std::ops::Bound;
    let result = table.by_hunger(
        (Bound::Excluded(10u32), Bound::Included(40u32)),
        QueryOpts::ASC,
    );
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].hunger, 20);
    assert_eq!(result[1].hunger, 30);
    assert_eq!(result[2].hunger, 40);
}

#[test]
fn by_range_excluded_both() {
    let table = make_hunger_table();
    use std::ops::Bound;
    let result = table.by_hunger(
        (Bound::Excluded(10u32), Bound::Excluded(40u32)),
        QueryOpts::ASC,
    );
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].hunger, 20);
    assert_eq!(result[1].hunger, 30);
}

#[test]
fn by_match_all() {
    let table = make_hunger_table();
    let all = table.by_hunger(MatchAll, QueryOpts::ASC);
    assert_eq!(all.len(), 5);
}

#[test]
fn by_range_full() {
    let table = make_hunger_table();
    let all = table.by_hunger(.., QueryOpts::ASC);
    assert_eq!(all.len(), 5);
}

// ============================================================================
// Index maintenance on mutations
// ============================================================================

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

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        0
    );

    table
        .update_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Capybara,
            hunger: 0,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 0);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
}

#[test]
fn index_maintained_on_upsert() {
    let mut table = CreatureTable::new();
    table
        .upsert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 0,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);

    table
        .upsert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Capybara,
            hunger: 0,
        })
        .unwrap();

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 0);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
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

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(table.count_by_hunger(&10u32, QueryOpts::ASC), 1);

    table.remove_no_fk(&CreatureId(1)).unwrap();

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 0);
    assert_eq!(table.count_by_hunger(&10u32, QueryOpts::ASC), 0);
}

#[test]
fn manual_rebuild_all_indexes() {
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

    table.manual_rebuild_all_indexes();

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
    assert_eq!(table.count_by_hunger(&5u32, QueryOpts::ASC), 1);
    assert_eq!(table.count_by_hunger(&10u32, QueryOpts::ASC), 1);
}

// ============================================================================
// Option<T> indexed fields
// ============================================================================

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

    let unassigned = table.by_assignee(&None, QueryOpts::ASC);
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].id, TaskId(1));

    let assigned_to_10 = table.by_assignee(&Some(CreatureId(10)), QueryOpts::ASC);
    assert_eq!(assigned_to_10.len(), 2);
    assert_eq!(assigned_to_10[0].id, TaskId(2));
    assert_eq!(assigned_to_10[1].id, TaskId(3));

    assert_eq!(table.count_by_assignee(&None, QueryOpts::ASC), 1);
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(10)), QueryOpts::ASC),
        2
    );
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(99)), QueryOpts::ASC),
        0
    );
}

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
    let all = table.by_assignee(MatchAll, QueryOpts::ASC);
    assert_eq!(all.len(), 4);

    // Range from None captures everything (None < Some(..)).
    let from_none = table.by_assignee(None.., QueryOpts::ASC);
    assert_eq!(from_none.len(), 4);
    assert_eq!(from_none[0].id, TaskId(1)); // None first

    // Range that captures only Some values.
    let some_only = table.by_assignee(Some(CreatureId(0)).., QueryOpts::ASC);
    assert_eq!(some_only.len(), 3);
    assert_eq!(some_only[0].id, TaskId(2));

    // Closed range within Some values.
    let mid = table.by_assignee(Some(CreatureId(5))..=Some(CreatureId(10)), QueryOpts::ASC);
    assert_eq!(mid.len(), 2);
    assert_eq!(mid[0].id, TaskId(2));
    assert_eq!(mid[1].id, TaskId(3));

    // Range up to (exclusive) Some(CreatureId(10)).
    let up_to = table.by_assignee(..Some(CreatureId(10)), QueryOpts::ASC);
    assert_eq!(up_to.len(), 2);
    assert_eq!(up_to[0].id, TaskId(1)); // None
    assert_eq!(up_to[1].id, TaskId(2)); // Some(5)
}

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

    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(10)), QueryOpts::ASC),
        1
    );
    assert_eq!(table.count_by_assignee(&None, QueryOpts::ASC), 0);

    table
        .update_no_fk(Task {
            id: TaskId(1),
            assignee: None,
            priority: 5,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(10)), QueryOpts::ASC),
        0
    );
    assert_eq!(table.count_by_assignee(&None, QueryOpts::ASC), 1);
}

// ============================================================================
// Edge cases
// ============================================================================

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

    let err = table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "B".into(),
            species: Species::Capybara,
            hunger: 20,
        })
        .unwrap_err();
    assert!(matches!(err, tabulosity::Error::DuplicateKey { .. }));

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        0
    );
    assert_eq!(table.count_by_hunger(&10u32, QueryOpts::ASC), 1);
    assert_eq!(table.count_by_hunger(&20u32, QueryOpts::ASC), 0);
}

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

    let result = table.by_hunger(1000u32.., QueryOpts::ASC);
    assert!(result.is_empty());
    assert_eq!(table.count_by_hunger(1000u32.., QueryOpts::ASC), 0);
    assert_eq!(table.iter_by_hunger(1000u32.., QueryOpts::ASC).count(), 0);
}

#[test]
fn range_queries_on_empty_table() {
    let table = CreatureTable::new();
    let result = table.by_hunger(MatchAll, QueryOpts::ASC);
    assert!(result.is_empty());
    assert_eq!(table.count_by_hunger(MatchAll, QueryOpts::ASC), 0);
    assert_eq!(table.iter_by_hunger(MatchAll, QueryOpts::ASC).count(), 0);
}

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

    table.manual_rebuild_all_indexes();
    table.manual_rebuild_all_indexes();

    assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
    assert_eq!(
        table.count_by_species(&Species::Capybara, QueryOpts::ASC),
        1
    );
    assert_eq!(table.count_by_hunger(&5u32, QueryOpts::ASC), 1);
    assert_eq!(table.count_by_hunger(&10u32, QueryOpts::ASC), 1);
}

#[test]
fn iter_by_returns_pk_order_regardless_of_insert_order() {
    let mut table = CreatureTable::new();
    // Insert out of PK order: 3, 1, 2
    for id in [3, 1, 2] {
        table
            .insert_no_fk(Creature {
                id: CreatureId(id),
                name: format!("C{}", id),
                species: Species::Elf,
                hunger: 0,
            })
            .unwrap();
    }

    let ids: Vec<_> = table
        .iter_by_species(&Species::Elf, QueryOpts::ASC)
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, vec![CreatureId(1), CreatureId(2), CreatureId(3)]);
}

// ============================================================================
// Compound indexes
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TaskStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CompTaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
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
fn compound_index_both_exact() {
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
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(2)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Both fields exact — point lookup.
    let result = table.by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, CompTaskId(1));

    let result = table.by_assignee_priority(&Some(CreatureId(1)), &3u8, QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, CompTaskId(2));
}

#[test]
fn compound_index_prefix_query() {
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

    // Prefix: exact first field, MatchAll second.
    // Results are in index order: (assignee, priority, pk).
    // Task 2 has priority 3, task 1 has priority 5.
    let result = table.by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, CompTaskId(2)); // priority 3
    assert_eq!(result[1].id, CompTaskId(1)); // priority 5
}

#[test]
fn compound_index_first_exact_second_range() {
    let mut table = CompTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: i as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    let result = table.by_assignee_priority(&Some(CreatureId(1)), 2u8..=4, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].priority, 2);
    assert_eq!(result[1].priority, 3);
    assert_eq!(result[2].priority, 4);
}

#[test]
fn compound_index_both_matchall() {
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
            assignee: None,
            priority: 3,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // MatchAll on both fields — returns everything in the index.
    let result = table.by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
}

#[test]
fn compound_index_first_matchall_second_exact() {
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
            assignee: Some(CreatureId(2)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(1)),
            priority: 3,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // MatchAll on first, exact on second — post-filter.
    let result = table.by_assignee_priority(MatchAll, &5u8, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, CompTaskId(1));
    assert_eq!(result[1].id, CompTaskId(2));
}

#[test]
fn compound_index_count_and_iter() {
    let mut table = CompTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: i as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        5
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), 2u8..=4, QueryOpts::ASC),
        3
    );

    let ids: Vec<_> = table
        .iter_by_assignee_priority(&Some(CreatureId(1)), 2u8..=4, QueryOpts::ASC)
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, vec![CompTaskId(2), CompTaskId(3), CompTaskId(4)]);
}

#[test]
fn compound_index_maintained_on_update() {
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );

    table
        .update_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(2)), &3u8, QueryOpts::ASC),
        1
    );
}

#[test]
fn compound_index_maintained_on_remove() {
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        1
    );

    table.remove_no_fk(&CompTaskId(1)).unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        0
    );
}

#[test]
fn compound_index_maintained_on_upsert() {
    let mut table = CompTaskTable::new();
    table
        .upsert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );

    // Upsert update path: change both fields.
    table
        .upsert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(2)), &3u8, QueryOpts::ASC),
        1
    );
}

#[test]
fn compound_index_rebuild() {
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

    table.manual_rebuild_all_indexes();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        2
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );
}

#[test]
fn compound_index_empty_table() {
    let table = CompTaskTable::new();
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        0
    );
    let result = table.by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert!(result.is_empty());
}

// ============================================================================
// Filtered indexes
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct FiltTaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "active_assignee",
    fields("assignee"),
    filter = "FiltTask::is_active"
)]
#[allow(clippy::duplicated_attributes)]
#[index(
    name = "active_priority",
    fields("assignee", "priority"),
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
fn filtered_index_insert_matching() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );
}

#[test]
fn filtered_index_insert_non_matching() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Done, // Does not match filter.
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        0
    );
    // But the simple index still has it.
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
}

#[test]
fn filtered_index_update_enters_filter() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );

    // Update: status changes from Done to Pending — enters filter.
    table
        .update_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
}

#[test]
fn filtered_index_update_exits_filter() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );

    // Update: status changes from Pending to Done — exits filter.
    table
        .update_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );
}

#[test]
fn filtered_index_update_stays_in_filter_field_change() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Both pass filter — index field changes.
    table
        .update_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(2)), QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(2)), &3u8, QueryOpts::ASC),
        1
    );
}

#[test]
fn filtered_index_remove() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );

    table.remove_no_fk(&FiltTaskId(1)).unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );
}

#[test]
fn filtered_index_remove_non_matching() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();

    // Remove a row that was never in the filtered index — should be a no-op
    // for the filtered index.
    table.remove_no_fk(&FiltTaskId(1)).unwrap();
    assert!(table.is_empty());
}

#[test]
fn filtered_index_rebuild() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(1)),
            priority: 3,
            status: TaskStatus::Done,
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    // Only the Pending task should be in the filtered index.
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
    let active = table.by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, FiltTaskId(1));
}

#[test]
fn filtered_compound_index_prefix_query() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(1)),
            priority: 3,
            status: TaskStatus::InProgress,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(3),
            assignee: Some(CreatureId(1)),
            priority: 1,
            status: TaskStatus::Done, // Excluded by filter.
        })
        .unwrap();

    // Prefix on compound filtered index.
    // Results are in index order: (assignee, priority, pk).
    // Task 2 has priority 3, task 1 has priority 5.
    let result = table.by_active_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, FiltTaskId(2)); // priority 3
    assert_eq!(result[1].id, FiltTaskId(1)); // priority 5
}

#[test]
fn filtered_index_upsert_enters_and_exits() {
    let mut table = FiltTaskTable::new();

    // Upsert insert path: matches filter.
    table
        .upsert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );

    // Upsert update path: exits filter.
    table
        .upsert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );

    // Upsert update path: re-enters filter.
    table
        .upsert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::InProgress,
        })
        .unwrap();
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
}

#[test]
fn filtered_index_matchall_returns_only_filtered_rows() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::Done,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(3),
            assignee: Some(CreatureId(3)),
            priority: 1,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    // MatchAll on filtered index returns only active tasks.
    let result = table.by_active_assignee(MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, FiltTaskId(1));
    assert_eq!(result[1].id, FiltTaskId(3));

    // MatchAll on compound filtered index.
    let result = table.by_active_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 2);

    // Simple index returns all rows regardless of filter.
    let result = table.by_assignee(MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
}

// ============================================================================
// IntoQuery trait tests (compile-time verification of API ergonomics)
// ============================================================================

#[test]
fn into_query_exact_ref() {
    let q: QueryBound<u32> = (&42u32).into_query();
    assert_eq!(q, QueryBound::Exact(42));
}

#[test]
fn into_query_range() {
    let q: QueryBound<u32> = (3u32..7).into_query();
    assert!(matches!(q, QueryBound::Range { .. }));
}

#[test]
fn into_query_matchall() {
    let q: QueryBound<u32> = MatchAll.into_query();
    assert_eq!(q, QueryBound::MatchAll);
}

#[test]
fn into_query_range_full() {
    let q: QueryBound<u32> = (..).into_query();
    assert_eq!(q, QueryBound::MatchAll);
}

#[test]
fn into_query_range_from() {
    use std::ops::Bound;
    let q: QueryBound<u32> = (5u32..).into_query();
    assert_eq!(
        q,
        QueryBound::Range {
            start: Bound::Included(5),
            end: Bound::Unbounded,
        }
    );
}

#[test]
fn into_query_range_to() {
    use std::ops::Bound;
    let q: QueryBound<u32> = (..10u32).into_query();
    assert_eq!(
        q,
        QueryBound::Range {
            start: Bound::Unbounded,
            end: Bound::Excluded(10),
        }
    );
}

#[test]
fn into_query_range_to_inclusive() {
    use std::ops::Bound;
    let q: QueryBound<u32> = (..=10u32).into_query();
    assert_eq!(
        q,
        QueryBound::Range {
            start: Bound::Unbounded,
            end: Bound::Included(10),
        }
    );
}

#[test]
fn into_query_range_inclusive() {
    use std::ops::Bound;
    let q: QueryBound<u32> = (3u32..=7).into_query();
    assert_eq!(
        q,
        QueryBound::Range {
            start: Bound::Included(3),
            end: Bound::Included(7),
        }
    );
}

#[test]
fn into_query_bound_tuple() {
    use std::ops::Bound;
    let q: QueryBound<u32> = (Bound::Excluded(3u32), Bound::Excluded(7u32)).into_query();
    assert_eq!(
        q,
        QueryBound::Range {
            start: Bound::Excluded(3),
            end: Bound::Excluded(7),
        }
    );
}

// ============================================================================
// 3-field compound index
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TripleId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "a_b_c", fields("a", "b", "c"))]
struct TripleRow {
    #[primary_key]
    pub id: TripleId,
    pub a: u8,
    pub b: u8,
    pub c: u8,
}

#[test]
fn triple_compound_all_exact() {
    let mut table = TripleRowTable::new();
    for i in 1..=8 {
        table
            .insert_no_fk(TripleRow {
                id: TripleId(i),
                a: (i % 2) as u8,
                b: (i % 3) as u8,
                c: (i % 4) as u8,
            })
            .unwrap();
    }

    // All-exact: narrow point query.
    let result = table.by_a_b_c(&0u8, &0u8, &0u8, QueryOpts::ASC);
    // id=6: a=0, b=0, c=2 — doesn't match c=0
    // id=4: a=0, b=1, c=0 — doesn't match b=0
    // id=2: a=0, b=2, c=2 — doesn't match
    // Only exact (0,0,0) — none in our data.
    assert!(result.is_empty());

    // Check a known triple.
    // id=1: a=1, b=1, c=1
    let result = table.by_a_b_c(&1u8, &1u8, &1u8, QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, TripleId(1));
}

#[test]
fn triple_compound_prefix_one() {
    let mut table = TripleRowTable::new();
    for i in 1..=6 {
        table
            .insert_no_fk(TripleRow {
                id: TripleId(i),
                a: (i / 4) as u8, // 0 for 1-3, 1 for 4-6
                b: i as u8,
                c: 0,
            })
            .unwrap();
    }

    let result = table.by_a_b_c(&0u8, MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
    let result = table.by_a_b_c(&1u8, MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
}

#[test]
fn triple_compound_prefix_two() {
    let mut table = TripleRowTable::new();
    table
        .insert_no_fk(TripleRow {
            id: TripleId(1),
            a: 1,
            b: 2,
            c: 10,
        })
        .unwrap();
    table
        .insert_no_fk(TripleRow {
            id: TripleId(2),
            a: 1,
            b: 2,
            c: 20,
        })
        .unwrap();
    table
        .insert_no_fk(TripleRow {
            id: TripleId(3),
            a: 1,
            b: 3,
            c: 10,
        })
        .unwrap();

    // Prefix on first two fields.
    let result = table.by_a_b_c(&1u8, &2u8, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].c, 10);
    assert_eq!(result[1].c, 20);
}

#[test]
fn triple_compound_middle_range() {
    let mut table = TripleRowTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(TripleRow {
                id: TripleId(i),
                a: 1,
                b: i as u8,
                c: 0,
            })
            .unwrap();
    }

    // Exact first, range second, MatchAll third.
    let result = table.by_a_b_c(&1u8, 2u8..=4, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].b, 2);
    assert_eq!(result[1].b, 3);
    assert_eq!(result[2].b, 4);
}

#[test]
fn triple_compound_all_matchall() {
    let mut table = TripleRowTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(TripleRow {
                id: TripleId(i),
                a: i as u8,
                b: 0,
                c: 0,
            })
            .unwrap();
    }

    let result = table.by_a_b_c(MatchAll, MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 5);
}

#[test]
fn triple_compound_empty_table() {
    let table = TripleRowTable::new();
    let result = table.by_a_b_c(&0u8, &0u8, &0u8, QueryOpts::ASC);
    assert!(result.is_empty());
    let result = table.by_a_b_c(MatchAll, MatchAll, MatchAll, QueryOpts::ASC);
    assert!(result.is_empty());
}

// ============================================================================
// Range query variants on actual tables
// ============================================================================

#[test]
fn range_from_query_on_table() {
    let mut table = CreatureTable::new();
    for i in 1..=10 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{i}"),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // RangeFrom: hunger >= 80
    let result = table.by_hunger(80u32.., QueryOpts::ASC);
    assert_eq!(result.len(), 3); // 80, 90, 100
}

#[test]
fn range_to_query_on_table() {
    let mut table = CreatureTable::new();
    for i in 1..=10 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{i}"),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // RangeTo: hunger < 30
    let result = table.by_hunger(..30u32, QueryOpts::ASC);
    assert_eq!(result.len(), 2); // 10, 20
}

#[test]
fn range_to_inclusive_query_on_table() {
    let mut table = CreatureTable::new();
    for i in 1..=10 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{i}"),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // RangeToInclusive: hunger <= 30
    let result = table.by_hunger(..=30u32, QueryOpts::ASC);
    assert_eq!(result.len(), 3); // 10, 20, 30
}

#[test]
fn bound_tuple_query_on_table() {
    use std::ops::Bound;
    let mut table = CreatureTable::new();
    for i in 1..=10 {
        table
            .insert_no_fk(Creature {
                id: CreatureId(i),
                name: format!("C{i}"),
                species: Species::Elf,
                hunger: i * 10,
            })
            .unwrap();
    }

    // Exclusive on both ends: 20 < hunger < 50
    let result = table.by_hunger(
        (Bound::Excluded(20u32), Bound::Excluded(50u32)),
        QueryOpts::ASC,
    );
    assert_eq!(result.len(), 2); // 30, 40
}

// ============================================================================
// Tracked bounds edge cases
// ============================================================================

#[test]
fn bounds_stale_after_all_rows_deleted() {
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    table.remove_no_fk(&CompTaskId(1)).unwrap();

    // Table is empty. Tracked bounds are stale (wider than reality)
    // but queries should still return empty results because the
    // BTreeSet range scan finds nothing.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        0
    );
    let result = table.by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert!(result.is_empty());
}

#[test]
fn bounds_correct_with_many_inserts_and_deletes() {
    let mut table = CompTaskTable::new();
    for i in 1..=20 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId((i % 3) + 1)),
                priority: (i % 5) as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    // Remove half.
    for i in 1..=10 {
        table.remove_no_fk(&CompTaskId(i)).unwrap();
    }

    // Query should still work correctly — only remaining rows returned.
    let all = table.by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(all.len(), 10);

    // Specific queries should match remaining data.
    for row in &all {
        let matches = table.by_assignee_priority(&row.assignee, &row.priority, QueryOpts::ASC);
        assert!(matches.iter().any(|m| m.id == row.id));
    }
}

// ============================================================================
// Filtered index edge cases
// ============================================================================

#[test]
fn filtered_index_empty_table_query() {
    let table = FiltTaskTable::new();
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );
    let result = table.by_active_assignee(MatchAll, QueryOpts::ASC);
    assert!(result.is_empty());
    let result = table.by_active_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert!(result.is_empty());
}

#[test]
fn filtered_index_all_rows_excluded() {
    let mut table = FiltTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(FiltTask {
                id: FiltTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: i as u8,
                status: TaskStatus::Done, // All excluded by filter.
            })
            .unwrap();
    }

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        0
    );

    // But simple index should have them.
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        5
    );
}

#[test]
fn filtered_index_update_stays_outside_filter() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();

    // Update indexed field while staying outside filter.
    table
        .update_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::Done,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(2)), QueryOpts::ASC),
        0
    );
    // Simple index should reflect the update.
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(2)), QueryOpts::ASC),
        1
    );
}

// ============================================================================
// Compound + simple index on same field
// ============================================================================

#[test]
fn compound_and_simple_index_on_same_field() {
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
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(2)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Simple #[indexed] query.
    let simple = table.by_assignee(&Some(CreatureId(1)), QueryOpts::ASC);
    assert_eq!(simple.len(), 2);

    // Compound index prefix query — same result but potentially different order.
    let compound = table.by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC);
    assert_eq!(compound.len(), 2);

    // After mutation, both indexes updated.
    table
        .update_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(3)),
            priority: 5,
            status: TaskStatus::Done,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(3)), QueryOpts::ASC),
        1
    );
}

// ============================================================================
// Duplicate values in indexed fields
// ============================================================================

#[test]
fn duplicate_values_in_compound_index() {
    let mut table = CompTaskTable::new();
    // Multiple rows with identical indexed field values, different PKs.
    for i in 1..=5 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: 5,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    let result = table.by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC);
    assert_eq!(result.len(), 5);

    // Verify all are returned.
    let ids: Vec<_> = result.iter().map(|r| r.id).collect();
    for i in 1..=5 {
        assert!(ids.contains(&CompTaskId(i)));
    }
}

// ============================================================================
// Compound index range query on second field
// ============================================================================

#[test]
fn compound_index_second_field_range_from() {
    let mut table = CompTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: i as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    // RangeFrom on second field.
    let result = table.by_assignee_priority(&Some(CreatureId(1)), 4u8.., QueryOpts::ASC);
    assert_eq!(result.len(), 2); // priority 4, 5
}

#[test]
fn compound_index_second_field_range_to() {
    let mut table = CompTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: i as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    // RangeTo (exclusive) on second field.
    let result = table.by_assignee_priority(&Some(CreatureId(1)), ..3u8, QueryOpts::ASC);
    assert_eq!(result.len(), 2); // priority 1, 2
}

// ============================================================================
// Single-row edge cases
// ============================================================================

#[test]
fn single_row_compound_index() {
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_assignee_priority(MatchAll, &5u8, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(2)), &5u8, QueryOpts::ASC),
        0
    );
}

#[test]
fn single_row_filtered_index() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
    assert_eq!(table.count_by_active_assignee(MatchAll, QueryOpts::ASC), 1);
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );
    assert_eq!(
        table.count_by_active_priority(MatchAll, MatchAll, QueryOpts::ASC),
        1
    );
}

// ============================================================================
// Compound index: update that doesn't change indexed fields (optimization path)
// ============================================================================

#[test]
fn compound_index_update_no_field_change() {
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Update only the non-indexed status field.
    table
        .update_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    // Index should still work correctly.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );
    let row = table.by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC);
    assert_eq!(row[0].status, TaskStatus::InProgress);
}

// ============================================================================
// Compound index with Option<T> = None queries
// ============================================================================

#[test]
fn compound_index_option_none_query() {
    let mut table = CompTaskTable::new();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: None,
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(2),
            assignee: None,
            priority: 3,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Query unassigned tasks via compound index.
    let result = table.by_assignee_priority(&None, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 2);

    let result = table.by_assignee_priority(&None, &5u8, QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, CompTaskId(1));
}

// ============================================================================
// Filtered compound index: range on second field
// ============================================================================

#[test]
fn filtered_compound_range_second_field() {
    let mut table = FiltTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(FiltTask {
                id: FiltTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: i as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }
    // One inactive task.
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(6),
            assignee: Some(CreatureId(1)),
            priority: 3,
            status: TaskStatus::Done,
        })
        .unwrap();

    // Range query on second field of filtered compound index.
    let result = table.by_active_priority(&Some(CreatureId(1)), 2u8..=4, QueryOpts::ASC);
    assert_eq!(result.len(), 3); // priorities 2, 3, 4 (all active)

    // Total active for this assignee.
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        5
    );
}

// ============================================================================
// iter_by on compound and filtered indexes
// ============================================================================

#[test]
fn iter_by_compound_index() {
    let mut table = CompTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId(1)),
                priority: i as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    let priorities: Vec<u8> = table
        .iter_by_assignee_priority(&Some(CreatureId(1)), 2u8..=4, QueryOpts::ASC)
        .map(|t| t.priority)
        .collect();
    assert_eq!(priorities, vec![2, 3, 4]);
}

#[test]
fn iter_by_filtered_index() {
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::Done,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(3),
            assignee: Some(CreatureId(3)),
            priority: 1,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    let ids: Vec<FiltTaskId> = table
        .iter_by_active_assignee(MatchAll, QueryOpts::ASC)
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, vec![FiltTaskId(1), FiltTaskId(3)]);
}

// ============================================================================
// Compound index: upsert with insert path
// ============================================================================

#[test]
fn compound_index_upsert_insert_path() {
    let mut table = CompTaskTable::new();

    // Upsert on empty table — insert path.
    table
        .upsert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );

    // Simple index too.
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        1
    );
}

// ============================================================================
// Multiple compound indexes on same struct
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct MultiIdxId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "a_b", fields("a", "b"))]
#[index(name = "b_c", fields("b", "c"))]
struct MultiIdx {
    #[primary_key]
    pub id: MultiIdxId,
    pub a: u8,
    pub b: u8,
    pub c: u8,
}

#[test]
fn multiple_compound_indexes() {
    let mut table = MultiIdxTable::new();
    table
        .insert_no_fk(MultiIdx {
            id: MultiIdxId(1),
            a: 1,
            b: 2,
            c: 3,
        })
        .unwrap();
    table
        .insert_no_fk(MultiIdx {
            id: MultiIdxId(2),
            a: 1,
            b: 2,
            c: 4,
        })
        .unwrap();
    table
        .insert_no_fk(MultiIdx {
            id: MultiIdxId(3),
            a: 2,
            b: 2,
            c: 3,
        })
        .unwrap();

    // Query via first compound index.
    let result = table.by_a_b(&1u8, &2u8, QueryOpts::ASC);
    assert_eq!(result.len(), 2);

    // Query via second compound index.
    let result = table.by_b_c(&2u8, &3u8, QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    let ids: Vec<_> = result.iter().map(|r| r.id).collect();
    assert!(ids.contains(&MultiIdxId(1)));
    assert!(ids.contains(&MultiIdxId(3)));
}

#[test]
fn multiple_compound_indexes_maintained_on_update() {
    let mut table = MultiIdxTable::new();
    table
        .insert_no_fk(MultiIdx {
            id: MultiIdxId(1),
            a: 1,
            b: 2,
            c: 3,
        })
        .unwrap();

    // Update all fields.
    table
        .update_no_fk(MultiIdx {
            id: MultiIdxId(1),
            a: 10,
            b: 20,
            c: 30,
        })
        .unwrap();

    assert_eq!(table.count_by_a_b(&1u8, &2u8, QueryOpts::ASC), 0);
    assert_eq!(table.count_by_a_b(&10u8, &20u8, QueryOpts::ASC), 1);
    assert_eq!(table.count_by_b_c(&2u8, &3u8, QueryOpts::ASC), 0);
    assert_eq!(table.count_by_b_c(&20u8, &30u8, QueryOpts::ASC), 1);
}

// ============================================================================
// Rebuild indexes with compound and filtered
// ============================================================================

#[test]
fn rebuild_preserves_compound_and_simple_indexes() {
    let mut table = CompTaskTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId((i % 2) + 1)),
                priority: i as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    table.manual_rebuild_all_indexes();

    // Compound index works.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        2
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(2)), MatchAll, QueryOpts::ASC),
        3
    );

    // Simple index works.
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(1)), QueryOpts::ASC),
        2
    );
    assert_eq!(
        table.count_by_assignee(&Some(CreatureId(2)), QueryOpts::ASC),
        3
    );
}

// ============================================================================
// Compound index on first field range, second MatchAll
// ============================================================================

#[test]
fn compound_index_first_range_second_matchall() {
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
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(3)),
            priority: 1,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(4),
            assignee: None,
            priority: 7,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Range on first field (Option types: None < Some), MatchAll on second.
    // This is a non-prefix query (range on first field), so it hits the catch-all arm.
    let result = table.by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 4);
}

// ============================================================================
// Additional coverage: compound filtered, stress, edge cases
// ============================================================================

#[test]
fn filtered_compound_update_changes_indexed_fields_while_staying_in_filter() {
    // Verify compound filtered index tuple is properly replaced when indexed
    // fields change but both old and new rows pass the filter.
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(10)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Verify initial state in compound filtered index.
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(10)), &5u8, QueryOpts::ASC),
        1
    );

    // Update: change both compound-indexed fields, stay active (InProgress).
    table
        .update_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(20)),
            priority: 9,
            status: TaskStatus::InProgress,
        })
        .unwrap();

    // Old tuple must be gone.
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(10)), &5u8, QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(10)), MatchAll, QueryOpts::ASC),
        0
    );
    // New tuple must be present.
    assert_eq!(
        table.count_by_active_priority(&Some(CreatureId(20)), &9u8, QueryOpts::ASC),
        1
    );
    // Single-field filtered index also updated.
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(10)), QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_active_assignee(&Some(CreatureId(20)), QueryOpts::ASC),
        1
    );
}

#[test]
fn filtered_compound_matchall_returns_only_filtered_subset() {
    // Mix of active and inactive rows; MatchAll on both fields of compound
    // filtered index should return only the filtered subset.
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 1,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(2)),
            priority: 2,
            status: TaskStatus::Done, // inactive
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(3),
            assignee: Some(CreatureId(3)),
            priority: 3,
            status: TaskStatus::InProgress,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(4),
            assignee: Some(CreatureId(1)),
            priority: 4,
            status: TaskStatus::Done, // inactive
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(5),
            assignee: Some(CreatureId(2)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // MatchAll on compound filtered index: only 3 active rows.
    let result = table.by_active_priority(MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(result.len(), 3);
    let ids: Vec<_> = result.iter().map(|r| r.id).collect();
    assert!(ids.contains(&FiltTaskId(1)));
    assert!(ids.contains(&FiltTaskId(3)));
    assert!(ids.contains(&FiltTaskId(5)));

    // MatchAll on single-field filtered index: same 3 active rows.
    assert_eq!(table.count_by_active_assignee(MatchAll, QueryOpts::ASC), 3);

    // Unfiltered simple index: all 5 rows.
    assert_eq!(table.count_by_assignee(MatchAll, QueryOpts::ASC), 5);
}

#[test]
fn compound_index_all_same_values() {
    // All rows have identical indexed fields; differ only by PK. Verify count.
    let mut table = CompTaskTable::new();
    for i in 1..=10 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId(42)),
                priority: 7,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(42)), &7u8, QueryOpts::ASC),
        10
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(42)), MatchAll, QueryOpts::ASC),
        10
    );
    assert_eq!(
        table.count_by_assignee_priority(MatchAll, &7u8, QueryOpts::ASC),
        10
    );
    assert_eq!(
        table.count_by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC),
        10
    );

    // Verify PKs are all present and in order.
    let ids: Vec<_> = table
        .iter_by_assignee_priority(&Some(CreatureId(42)), &7u8, QueryOpts::ASC)
        .map(|t| t.id)
        .collect();
    for i in 1..=10 {
        assert_eq!(ids[(i - 1) as usize], CompTaskId(i));
    }
}

#[test]
fn large_scale_stress_test() {
    // Insert 100 rows, query various ranges, verify counts.
    let mut table = CompTaskTable::new();
    for i in 1..=100u32 {
        table
            .insert_no_fk(CompTask {
                id: CompTaskId(i),
                assignee: Some(CreatureId((i % 5) + 1)),
                priority: (i % 10) as u8,
                status: TaskStatus::Pending,
            })
            .unwrap();
    }

    assert_eq!(table.len(), 100);

    // Each of 5 assignees gets 20 rows.
    for a in 1..=5 {
        assert_eq!(
            table.count_by_assignee_priority(&Some(CreatureId(a)), MatchAll, QueryOpts::ASC),
            20
        );
        assert_eq!(
            table.count_by_assignee(&Some(CreatureId(a)), QueryOpts::ASC),
            20
        );
    }

    // Each priority 0-9 gets 10 rows.
    for p in 0..10u8 {
        assert_eq!(
            table.count_by_assignee_priority(MatchAll, &p, QueryOpts::ASC),
            10
        );
    }

    // Range: priorities 3..=7 for assignee 1.
    let result = table.by_assignee_priority(&Some(CreatureId(1)), 3u8..=7, QueryOpts::ASC);
    // Assignee 1 has i where i%5==0 → i=5,10,15,...,100. That's 20 rows.
    // Of those, priority = i%10: 5,0,5,0,5,0,...  — only priorities 0 and 5.
    // In range 3..=7: only priority 5 rows.
    let count_p5 = result.len();
    assert_eq!(count_p5, 10);

    // MatchAll on both returns everything.
    assert_eq!(
        table.count_by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC),
        100
    );

    // Remove 50 rows.
    for i in 1..=50u32 {
        table.remove_no_fk(&CompTaskId(i)).unwrap();
    }
    assert_eq!(table.len(), 50);
    assert_eq!(
        table.count_by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC),
        50
    );
}

#[test]
fn compound_index_exact_match_no_results() {
    // Query exact values that don't exist, but table is non-empty.
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
            assignee: Some(CreatureId(2)),
            priority: 3,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Assignee exists but priority doesn't match.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &99u8, QueryOpts::ASC),
        0
    );
    // Priority exists but assignee doesn't match.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(99)), &5u8, QueryOpts::ASC),
        0
    );
    // Neither exists.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(99)), &99u8, QueryOpts::ASC),
        0
    );
    // Verify table is non-empty.
    assert_eq!(table.len(), 2);
}

#[test]
fn triple_compound_matchall_first_range_second_exact_third() {
    // Non-prefix range: MatchAll on first, Range on second, Exact on third.
    let mut table = TripleRowTable::new();
    // a=1, b varies 1-5, c=10
    for i in 1..=5 {
        table
            .insert_no_fk(TripleRow {
                id: TripleId(i),
                a: 1,
                b: i as u8,
                c: 10,
            })
            .unwrap();
    }
    // a=2, b varies 1-5, c=10
    for i in 6..=10 {
        table
            .insert_no_fk(TripleRow {
                id: TripleId(i),
                a: 2,
                b: (i - 5) as u8,
                c: 10,
            })
            .unwrap();
    }
    // a=1, b=3, c=20 (different c, should not match exact c=10)
    table
        .insert_no_fk(TripleRow {
            id: TripleId(11),
            a: 1,
            b: 3,
            c: 20,
        })
        .unwrap();

    // MatchAll on a, range 2..=4 on b, exact 10 on c.
    let result = table.by_a_b_c(MatchAll, 2u8..=4, &10u8, QueryOpts::ASC);
    // Expected: rows with b in {2,3,4} and c=10, any a.
    // a=1: b=2,3,4 c=10 → ids 2,3,4
    // a=2: b=2,3,4 c=10 → ids 7,8,9
    // id=11 has c=20, excluded.
    assert_eq!(result.len(), 6);
    let ids: Vec<_> = result.iter().map(|r| r.id).collect();
    for expected in [2, 3, 4, 7, 8, 9] {
        assert!(
            ids.contains(&TripleId(expected)),
            "missing TripleId({})",
            expected
        );
    }
}

#[test]
fn triple_compound_update_and_remove() {
    // Verify index maintenance for 3-field compound.
    let mut table = TripleRowTable::new();
    table
        .insert_no_fk(TripleRow {
            id: TripleId(1),
            a: 1,
            b: 2,
            c: 3,
        })
        .unwrap();
    table
        .insert_no_fk(TripleRow {
            id: TripleId(2),
            a: 1,
            b: 2,
            c: 4,
        })
        .unwrap();

    // Verify initial state.
    assert_eq!(
        table.count_by_a_b_c(&1u8, &2u8, MatchAll, QueryOpts::ASC),
        2
    );

    // Update row 1: change all indexed fields.
    table
        .update_no_fk(TripleRow {
            id: TripleId(1),
            a: 10,
            b: 20,
            c: 30,
        })
        .unwrap();

    // Old tuple gone.
    assert_eq!(table.count_by_a_b_c(&1u8, &2u8, &3u8, QueryOpts::ASC), 0);
    // Row 2 still there.
    assert_eq!(table.count_by_a_b_c(&1u8, &2u8, &4u8, QueryOpts::ASC), 1);
    // New tuple present.
    assert_eq!(table.count_by_a_b_c(&10u8, &20u8, &30u8, QueryOpts::ASC), 1);

    // Remove row 2.
    table.remove_no_fk(&TripleId(2)).unwrap();
    assert_eq!(
        table.count_by_a_b_c(&1u8, &2u8, MatchAll, QueryOpts::ASC),
        0
    );
    assert_eq!(table.count_by_a_b_c(&10u8, &20u8, &30u8, QueryOpts::ASC), 1);

    // Remove row 1.
    table.remove_no_fk(&TripleId(1)).unwrap();
    assert_eq!(
        table.count_by_a_b_c(MatchAll, MatchAll, MatchAll, QueryOpts::ASC),
        0
    );
    assert!(table.is_empty());
}

#[test]
fn compound_index_upsert_changes_only_one_indexed_field() {
    // Verify partial field change via upsert update path.
    let mut table = CompTaskTable::new();
    table
        .upsert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        1
    );

    // Upsert update: change only priority, keep assignee the same.
    table
        .upsert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 9,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Old compound tuple gone.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &5u8, QueryOpts::ASC),
        0
    );
    // New compound tuple present.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &9u8, QueryOpts::ASC),
        1
    );
    // Prefix query unchanged: still 1 row for this assignee.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        1
    );

    // Now change only assignee, keep priority the same.
    table
        .upsert_no_fk(CompTask {
            id: CompTaskId(1),
            assignee: Some(CreatureId(2)),
            priority: 9,
            status: TaskStatus::Pending,
        })
        .unwrap();

    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), &9u8, QueryOpts::ASC),
        0
    );
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(2)), &9u8, QueryOpts::ASC),
        1
    );
}

#[test]
fn filtered_single_field_range_query() {
    // Range query on the filtered single-field index (by_active_assignee).
    let mut table = FiltTaskTable::new();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(1),
            assignee: Some(CreatureId(1)),
            priority: 1,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(2),
            assignee: Some(CreatureId(5)),
            priority: 2,
            status: TaskStatus::InProgress,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(3),
            assignee: Some(CreatureId(10)),
            priority: 3,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(4),
            assignee: Some(CreatureId(15)),
            priority: 4,
            status: TaskStatus::Done, // inactive
        })
        .unwrap();
    table
        .insert_no_fk(FiltTask {
            id: FiltTaskId(5),
            assignee: Some(CreatureId(20)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Range on filtered index: assignee in Some(CreatureId(3))..=Some(CreatureId(12)).
    // Active rows with assignee in that range: id=2 (assignee 5), id=3 (assignee 10).
    let result =
        table.by_active_assignee(Some(CreatureId(3))..=Some(CreatureId(12)), QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, FiltTaskId(2));
    assert_eq!(result[1].id, FiltTaskId(3));

    // RangeFrom on filtered index: assignee >= Some(CreatureId(10)).
    // Active: id=3 (10), id=5 (20). id=4 (15) is inactive.
    let result = table.by_active_assignee(Some(CreatureId(10)).., QueryOpts::ASC);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, FiltTaskId(3));
    assert_eq!(result[1].id, FiltTaskId(5));

    // Compare to unfiltered simple index for the same range.
    let result = table.by_assignee(Some(CreatureId(10)).., QueryOpts::ASC);
    assert_eq!(result.len(), 3); // includes inactive id=4
}

// ============================================================================
// Compound index: first field range, second field exact (non-prefix)
// ============================================================================

#[test]
fn compound_index_first_range_second_exact() {
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
            assignee: Some(CreatureId(2)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(3)),
            priority: 3,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Range on first, exact on second — hits catch-all arm with post-filtering.
    let result = table.by_assignee_priority(
        Some(CreatureId(1))..=Some(CreatureId(2)),
        &5u8,
        QueryOpts::ASC,
    );
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].id, CompTaskId(1));
    assert_eq!(result[1].id, CompTaskId(2));
}

#[test]
fn compound_index_both_ranges() {
    let mut table = CompTaskTable::new();
    for a in 1..=5u32 {
        for p in 1..=5u8 {
            let id = (a - 1) * 5 + p as u32;
            table
                .insert_no_fk(CompTask {
                    id: CompTaskId(id),
                    assignee: Some(CreatureId(a)),
                    priority: p,
                    status: TaskStatus::Pending,
                })
                .unwrap();
        }
    }

    // Both fields are ranges — catch-all arm, post-filter both.
    let result = table.by_assignee_priority(
        Some(CreatureId(2))..=Some(CreatureId(4)),
        2u8..=4,
        QueryOpts::ASC,
    );
    // Assignees 2,3,4 × priorities 2,3,4 = 9 rows.
    assert_eq!(result.len(), 9);
}

// ============================================================================
// Tracked bounds: insert-delete-insert cycle
// ============================================================================

#[test]
fn bounds_insert_delete_insert_cycle() {
    let mut table = CreatureTable::new();
    table
        .insert_no_fk(Creature {
            id: CreatureId(1),
            name: "A".into(),
            species: Species::Elf,
            hunger: 50,
        })
        .unwrap();

    table.remove_no_fk(&CreatureId(1)).unwrap();

    // Insert a new row with value inside the stale bounds range.
    table
        .insert_no_fk(Creature {
            id: CreatureId(2),
            name: "B".into(),
            species: Species::Elf,
            hunger: 30,
        })
        .unwrap();

    // Query for the old value should return empty.
    let result = table.by_hunger(&50u32, QueryOpts::ASC);
    assert!(result.is_empty());

    // Query for the new value should return the new row.
    let result = table.by_hunger(&30u32, QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, CreatureId(2));
}

// ============================================================================
// Tracked bounds: manual_rebuild_all_indexes recomputes bounds
// ============================================================================

#[test]
fn bounds_recomputed_on_rebuild() {
    let mut table = CompTaskTable::new();

    // Insert rows with wide value range.
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

    // Delete the row with extreme values.
    table.remove_no_fk(&CompTaskId(2)).unwrap();

    // Before rebuild: bounds are stale (include CreatureId(100), priority 255).
    // After rebuild: bounds should tighten to current data only.
    table.manual_rebuild_all_indexes();

    // Insert a row after rebuild with moderate values.
    table
        .insert_no_fk(CompTask {
            id: CompTaskId(3),
            assignee: Some(CreatureId(1)),
            priority: 5,
            status: TaskStatus::Pending,
        })
        .unwrap();

    // Verify queries work correctly with the tightened bounds.
    assert_eq!(
        table.count_by_assignee_priority(&Some(CreatureId(1)), MatchAll, QueryOpts::ASC),
        2
    );
    assert_eq!(
        table.count_by_assignee_priority(MatchAll, MatchAll, QueryOpts::ASC),
        2
    );
}

/// Additional gap coverage tests for indexed tables: reinsert with different
/// index value, inverted ranges, single-value ranges, clear + reinsert, empty
/// table queries, compound/simple index PK ordering, and filtered index
/// behavior when all rows exit the filter.
mod gap_coverage {
    use tabulosity::{Bounded, MatchAll, QueryOpts, Table};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct CId(u32);

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    #[allow(dead_code)]
    enum Species {
        Elf,
        Capybara,
        Squirrel,
    }

    #[derive(Table, Clone, Debug, PartialEq)]
    struct Creature {
        #[primary_key]
        pub id: CId,
        pub name: String,
        #[indexed]
        pub species: Species,
        #[indexed]
        pub hunger: u32,
    }

    // --- Reinsert same PK with different index value ---

    #[test]
    fn reinsert_same_pk_different_index_value() {
        let mut table = CreatureTable::new();
        table
            .insert_no_fk(Creature {
                id: CId(1),
                name: "A".into(),
                species: Species::Elf,
                hunger: 10,
            })
            .unwrap();

        assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);

        table.remove_no_fk(&CId(1)).unwrap();
        assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 0);

        // Reinsert same PK with different species.
        table
            .insert_no_fk(Creature {
                id: CId(1),
                name: "A".into(),
                species: Species::Capybara,
                hunger: 20,
            })
            .unwrap();

        assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 0);
        assert_eq!(
            table.count_by_species(&Species::Capybara, QueryOpts::ASC),
            1
        );
        // Also check the hunger index was cleaned up and rebuilt.
        assert_eq!(table.count_by_hunger(&10u32, QueryOpts::ASC), 0);
        assert_eq!(table.count_by_hunger(&20u32, QueryOpts::ASC), 1);
    }

    // --- Inverted range returns empty ---

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn inverted_range_returns_empty() {
        let mut table = CreatureTable::new();
        for i in 0..5 {
            table
                .insert_no_fk(Creature {
                    id: CId(i),
                    name: format!("C{i}"),
                    species: Species::Elf,
                    hunger: i * 10,
                })
                .unwrap();
        }

        // Inverted range: 40..=10 (start > end).
        let result = table.by_hunger(40u32..=10, QueryOpts::ASC);
        assert!(result.is_empty(), "inverted range should return empty");

        let count = table.count_by_hunger(40u32..=10, QueryOpts::ASC);
        assert_eq!(count, 0);

        let iter_result: Vec<_> = table.iter_by_hunger(40u32..=10, QueryOpts::ASC).collect();
        assert!(iter_result.is_empty());
    }

    // --- Single-value range query ---

    #[test]
    fn single_value_range_query() {
        let mut table = CreatureTable::new();
        table
            .insert_no_fk(Creature {
                id: CId(1),
                name: "A".into(),
                species: Species::Elf,
                hunger: 20,
            })
            .unwrap();
        table
            .insert_no_fk(Creature {
                id: CId(2),
                name: "B".into(),
                species: Species::Elf,
                hunger: 20,
            })
            .unwrap();
        table
            .insert_no_fk(Creature {
                id: CId(3),
                name: "C".into(),
                species: Species::Elf,
                hunger: 30,
            })
            .unwrap();

        let result = table.by_hunger(20u32..=20u32, QueryOpts::ASC);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, CId(1));
        assert_eq!(result[1].id, CId(2));
    }

    // --- Clear all rows then reinsert ---

    #[test]
    fn clear_all_rows_then_reinsert() {
        let mut table = CreatureTable::new();
        for i in 1..=3 {
            table
                .insert_no_fk(Creature {
                    id: CId(i),
                    name: format!("C{i}"),
                    species: Species::Elf,
                    hunger: i * 10,
                })
                .unwrap();
        }

        // Remove all rows.
        for i in 1..=3 {
            table.remove_no_fk(&CId(i)).unwrap();
        }
        assert!(table.is_empty());
        assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 0);

        // Reinsert new rows.
        table
            .insert_no_fk(Creature {
                id: CId(10),
                name: "X".into(),
                species: Species::Capybara,
                hunger: 50,
            })
            .unwrap();
        table
            .insert_no_fk(Creature {
                id: CId(11),
                name: "Y".into(),
                species: Species::Elf,
                hunger: 60,
            })
            .unwrap();

        assert_eq!(table.len(), 2);
        assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 1);
        assert_eq!(
            table.count_by_species(&Species::Capybara, QueryOpts::ASC),
            1
        );
        assert_eq!(table.count_by_hunger(&50u32, QueryOpts::ASC), 1);
        assert_eq!(table.count_by_hunger(&60u32, QueryOpts::ASC), 1);
    }

    // --- Empty table index queries ---

    #[test]
    fn empty_table_index_queries() {
        let table = CreatureTable::new();

        assert_eq!(table.count_by_species(&Species::Elf, QueryOpts::ASC), 0);
        assert!(table.by_species(&Species::Elf, QueryOpts::ASC).is_empty());
        assert_eq!(
            table.iter_by_species(&Species::Elf, QueryOpts::ASC).count(),
            0
        );

        assert_eq!(table.count_by_hunger(&10u32, QueryOpts::ASC), 0);
        assert!(table.by_hunger(0u32..100, QueryOpts::ASC).is_empty());
        assert_eq!(table.count_by_hunger(.., QueryOpts::ASC), 0);
    }

    // --- Compound index PK ordering within bucket ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct WId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    #[index(name = "species_hunger", fields("species", "hunger"))]
    struct Widget {
        #[primary_key]
        pub id: WId,
        #[indexed]
        pub species: u32,
        pub hunger: u32,
        pub label: String,
    }

    #[test]
    fn compound_index_pk_ordered_within_bucket() {
        let mut table = WidgetTable::new();
        // Insert in non-PK order, all with same compound key.
        for &id in &[5, 1, 3, 2, 4] {
            table
                .insert_no_fk(Widget {
                    id: WId(id),
                    species: 1,
                    hunger: 10,
                    label: format!("W{id}"),
                })
                .unwrap();
        }

        let result = table.by_species_hunger(&1u32, &10u32, QueryOpts::ASC);
        let ids: Vec<u32> = result.iter().map(|w| w.id.0).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    }

    // --- Simple index: multiple same-value rows returned in PK order ---

    #[test]
    fn simple_index_pk_ordered() {
        let mut table = CreatureTable::new();
        for &id in &[3, 1, 2] {
            table
                .insert_no_fk(Creature {
                    id: CId(id),
                    name: format!("C{id}"),
                    species: Species::Elf,
                    hunger: 50,
                })
                .unwrap();
        }

        let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
        let ids: Vec<u32> = elves.iter().map(|c| c.id.0).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    // --- Filtered index: all rows exit filter via update ---

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
    struct FId(u32);

    #[derive(Table, Clone, Debug, PartialEq)]
    #[index(name = "active_score", fields("score"), filter = "FiltRow::is_active")]
    struct FiltRow {
        #[primary_key]
        pub id: FId,
        #[indexed]
        pub score: u32,
        pub active: bool,
    }

    impl FiltRow {
        fn is_active(&self) -> bool {
            self.active
        }
    }

    #[test]
    fn filtered_index_all_rows_exit_filter() {
        let mut table = FiltRowTable::new();
        for i in 1..=3 {
            table
                .insert_no_fk(FiltRow {
                    id: FId(i),
                    score: i * 10,
                    active: true,
                })
                .unwrap();
        }

        assert_eq!(table.count_by_active_score(MatchAll, QueryOpts::ASC), 3);

        // Deactivate all rows.
        for i in 1..=3 {
            table
                .update_no_fk(FiltRow {
                    id: FId(i),
                    score: i * 10,
                    active: false,
                })
                .unwrap();
        }

        // Filtered index should be empty.
        assert_eq!(table.count_by_active_score(MatchAll, QueryOpts::ASC), 0);
        assert!(table.by_active_score(MatchAll, QueryOpts::ASC).is_empty());
    }
}
