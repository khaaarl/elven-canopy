//! Tests for `QueryOpts` — ordering (asc/desc), offset, and
//! `modify_each_by_*` methods.

use tabulosity::{Bounded, MatchAll, QueryOpts, Table};

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

fn make_table() -> CreatureTable {
    let mut t = CreatureTable::new();
    t.insert_no_fk(Creature {
        id: CreatureId(1),
        name: "Aelindra".into(),
        species: Species::Elf,
        hunger: 10,
    })
    .unwrap();
    t.insert_no_fk(Creature {
        id: CreatureId(2),
        name: "Thorn".into(),
        species: Species::Capybara,
        hunger: 20,
    })
    .unwrap();
    t.insert_no_fk(Creature {
        id: CreatureId(3),
        name: "Whisper".into(),
        species: Species::Elf,
        hunger: 5,
    })
    .unwrap();
    t.insert_no_fk(Creature {
        id: CreatureId(4),
        name: "Glade".into(),
        species: Species::Elf,
        hunger: 15,
    })
    .unwrap();
    t
}

// ============================================================================
// Ordering tests
// ============================================================================

#[test]
fn desc_ordering_simple() {
    let table = make_table();
    let elves = table.by_species(&Species::Elf, QueryOpts::DESC);
    let ids: Vec<CreatureId> = elves.iter().map(|c| c.id).collect();
    // Desc should reverse the PK order within species=Elf.
    assert_eq!(ids, vec![CreatureId(4), CreatureId(3), CreatureId(1)]);
}

#[test]
fn asc_ordering_simple() {
    let table = make_table();
    let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
    let ids: Vec<CreatureId> = elves.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![CreatureId(1), CreatureId(3), CreatureId(4)]);
}

#[test]
fn desc_ordering_iter() {
    let table = make_table();
    let ids: Vec<CreatureId> = table
        .iter_by_species(&Species::Elf, QueryOpts::DESC)
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, vec![CreatureId(4), CreatureId(3), CreatureId(1)]);
}

// ============================================================================
// Offset tests
// ============================================================================

#[test]
fn offset_skips_first_n() {
    let table = make_table();
    let elves = table.by_species(&Species::Elf, QueryOpts::offset(1));
    let ids: Vec<CreatureId> = elves.iter().map(|c| c.id).collect();
    // Skips first elf (id=1), returns remaining two.
    assert_eq!(ids, vec![CreatureId(3), CreatureId(4)]);
}

#[test]
fn desc_with_offset() {
    let table = make_table();
    let elves = table.by_species(&Species::Elf, QueryOpts::DESC.with_offset(1));
    let ids: Vec<CreatureId> = elves.iter().map(|c| c.id).collect();
    // Desc order: 4, 3, 1 → skip 1 → 3, 1
    assert_eq!(ids, vec![CreatureId(3), CreatureId(1)]);
}

#[test]
fn count_respects_offset() {
    let table = make_table();
    let total = table.count_by_species(&Species::Elf, QueryOpts::ASC);
    assert_eq!(total, 3);
    let after_offset = table.count_by_species(&Species::Elf, QueryOpts::offset(1));
    assert_eq!(after_offset, 2);
}

#[test]
fn offset_exceeds_count_returns_empty() {
    let table = make_table();
    let elves = table.by_species(&Species::Elf, QueryOpts::offset(100));
    assert!(elves.is_empty());
    assert_eq!(
        table.count_by_species(&Species::Elf, QueryOpts::offset(100)),
        0
    );
}

// ============================================================================
// Compound index with ordering
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TaskId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "assignee_priority", fields("assignee", "priority"))]
struct Task {
    #[primary_key]
    pub id: TaskId,
    #[indexed]
    pub assignee: u32,
    pub priority: u32,
    pub label: String,
}

#[test]
fn desc_ordering_compound_index() {
    let mut table = TaskTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Task {
                id: TaskId(i),
                assignee: 1,
                priority: i * 10,
                label: format!("task{i}"),
            })
            .unwrap();
    }

    let desc = table.by_assignee_priority(&1u32, MatchAll, QueryOpts::DESC);
    let ids: Vec<TaskId> = desc.iter().map(|t| t.id).collect();
    assert_eq!(
        ids,
        vec![TaskId(4), TaskId(3), TaskId(2), TaskId(1), TaskId(0)]
    );

    let asc = table.by_assignee_priority(&1u32, MatchAll, QueryOpts::ASC);
    let ids: Vec<TaskId> = asc.iter().map(|t| t.id).collect();
    assert_eq!(
        ids,
        vec![TaskId(0), TaskId(1), TaskId(2), TaskId(3), TaskId(4)]
    );
}

#[test]
fn compound_index_desc_with_offset() {
    let mut table = TaskTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Task {
                id: TaskId(i),
                assignee: 1,
                priority: i * 10,
                label: format!("task{i}"),
            })
            .unwrap();
    }

    let result = table.by_assignee_priority(&1u32, MatchAll, QueryOpts::DESC.with_offset(2));
    let ids: Vec<TaskId> = result.iter().map(|t| t.id).collect();
    // Desc: 4, 3, 2, 1, 0 → skip 2 → 2, 1, 0
    assert_eq!(ids, vec![TaskId(2), TaskId(1), TaskId(0)]);
}

// ============================================================================
// modify_each_by_* tests
// ============================================================================

#[test]
fn modify_each_by_basic() {
    let mut table = make_table();
    let count = table.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, row| {
        row.name = format!("{}-modified", row.name);
    });
    assert_eq!(count, 3);

    // Verify all elves were modified.
    for elf in table.by_species(&Species::Elf, QueryOpts::ASC) {
        assert!(elf.name.ends_with("-modified"));
    }

    // Verify capybara was not modified.
    let capy = table.get(&CreatureId(2)).unwrap();
    assert_eq!(capy.name, "Thorn");
}

#[test]
fn modify_each_by_empty_result() {
    let mut table = make_table();
    let count = table.modify_each_by_species(&Species::Squirrel, QueryOpts::ASC, |_pk, row| {
        row.name = "should not happen".into();
    });
    assert_eq!(count, 0);
}

#[test]
fn modify_each_by_with_desc_and_offset() {
    let mut table = make_table();
    // Desc order for elves: id=4, id=3, id=1. With offset 1: id=3, id=1.
    let count =
        table.modify_each_by_species(&Species::Elf, QueryOpts::DESC.with_offset(1), |_pk, row| {
            row.name = "modified".into();
        });
    assert_eq!(count, 2);

    // id=4 (Glade) was NOT modified (skipped by offset).
    assert_eq!(table.get(&CreatureId(4)).unwrap().name, "Glade");
    // id=3 (Whisper) was modified.
    assert_eq!(table.get(&CreatureId(3)).unwrap().name, "modified");
    // id=1 (Aelindra) was modified.
    assert_eq!(table.get(&CreatureId(1)).unwrap().name, "modified");
}

#[test]
fn modify_each_by_returns_count() {
    let mut table = make_table();
    let count = table.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, _row| {});
    assert_eq!(count, 3);
    let count = table.modify_each_by_species(&Species::Capybara, QueryOpts::ASC, |_pk, _row| {});
    assert_eq!(count, 1);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "indexed field")]
fn modify_each_by_debug_catches_indexed_field_change() {
    let mut table = make_table();
    table.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, row| {
        // Changing an indexed field should panic in debug builds.
        row.species = Species::Squirrel;
    });
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "primary key")]
fn modify_each_by_debug_catches_pk_change() {
    let mut table = make_table();
    table.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, row| {
        row.id = CreatureId(999);
    });
}

// ============================================================================
// QueryOpts constructors
// ============================================================================

#[test]
fn query_opts_default_is_asc_zero_offset() {
    let opts = QueryOpts::default();
    assert_eq!(opts, QueryOpts::ASC);
}

#[test]
fn query_opts_desc_helper() {
    let opts = QueryOpts::desc();
    assert_eq!(opts, QueryOpts::DESC);
}

#[test]
fn query_opts_with_offset_chaining() {
    let opts = QueryOpts::desc().with_offset(5);
    assert_eq!(opts.order, tabulosity::QueryOrder::Desc);
    assert_eq!(opts.offset, 5);
}

// ============================================================================
// DESC / offset with range queries (HIGH priority)
// ============================================================================

#[test]
fn desc_with_range_query() {
    let table = make_table();
    // Hunger values: Whisper=5, Aelindra=10, Glade=15, Thorn=20
    // Range 10..=20 should match: Aelindra(10), Glade(15), Thorn(20)
    let desc = table.by_hunger(10u32..=20u32, QueryOpts::DESC);
    let ids: Vec<CreatureId> = desc.iter().map(|c| c.id).collect();
    // BTreeSet stores (hunger, pk). DESC reverses: (20,2), (15,4), (10,1)
    assert_eq!(ids, vec![CreatureId(2), CreatureId(4), CreatureId(1)]);

    let asc = table.by_hunger(10u32..=20u32, QueryOpts::ASC);
    let ids_asc: Vec<CreatureId> = asc.iter().map(|c| c.id).collect();
    // ASC: (10,1), (15,4), (20,2)
    assert_eq!(ids_asc, vec![CreatureId(1), CreatureId(4), CreatureId(2)]);
}

#[test]
fn offset_with_range_query() {
    let table = make_table();
    // Range 5..=20 matches all 4 rows. With offset 2, skip first 2.
    let result = table.by_hunger(5u32..=20u32, QueryOpts::offset(2));
    let ids: Vec<CreatureId> = result.iter().map(|c| c.id).collect();
    // ASC order by (hunger, pk): (5,3), (10,1), (15,4), (20,2) → skip 2 → (15,4), (20,2)
    assert_eq!(ids, vec![CreatureId(4), CreatureId(2)]);
}

#[test]
fn desc_with_offset_range_query() {
    let table = make_table();
    // Range 5..=20 matches all 4. DESC + offset 1.
    let result = table.by_hunger(5u32..=20u32, QueryOpts::DESC.with_offset(1));
    let ids: Vec<CreatureId> = result.iter().map(|c| c.id).collect();
    // DESC order: (20,2), (15,4), (10,1), (5,3) → skip 1 → (15,4), (10,1), (5,3)
    assert_eq!(ids, vec![CreatureId(4), CreatureId(1), CreatureId(3)]);
}

// ============================================================================
// modify_each on compound index (HIGH priority)
// ============================================================================

#[test]
fn modify_each_by_compound_index() {
    let mut table = TaskTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Task {
                id: TaskId(i),
                assignee: 1,
                priority: i * 10,
                label: format!("task{i}"),
            })
            .unwrap();
    }
    // Also add a task for a different assignee.
    table
        .insert_no_fk(Task {
            id: TaskId(10),
            assignee: 2,
            priority: 50,
            label: "other".into(),
        })
        .unwrap();

    let count =
        table.modify_each_by_assignee_priority(&1u32, MatchAll, QueryOpts::ASC, |_pk, row| {
            row.label = format!("{}-done", row.label);
        });
    assert_eq!(count, 5);

    // Verify assignee=1 tasks were modified.
    for task in table.by_assignee_priority(&1u32, MatchAll, QueryOpts::ASC) {
        assert!(task.label.ends_with("-done"), "got: {}", task.label);
    }

    // Verify assignee=2 task was NOT modified.
    assert_eq!(table.get(&TaskId(10)).unwrap().label, "other");
}

// ============================================================================
// modify_each debug-assert for cross-index field change (HIGH priority)
// ============================================================================

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "indexed field")]
fn modify_each_by_debug_catches_cross_index_field_change() {
    let mut table = make_table();
    // Query by species index, but change hunger (indexed by a DIFFERENT index).
    // The debug assertion should catch this because ALL indexed fields are snapshotted.
    table.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, row| {
        row.hunger = 999;
    });
}

// ============================================================================
// DESC on filtered index (MEDIUM priority)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ItemId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "active_category",
    fields("category"),
    filter = "Item::is_active"
)]
struct Item {
    #[primary_key]
    pub id: ItemId,
    #[indexed]
    pub category: u32,
    pub status: Status,
}

impl Item {
    fn is_active(&self) -> bool {
        self.status == Status::Active
    }
}

fn make_item_table() -> ItemTable {
    let mut t = ItemTable::new();
    // Active items in category 1.
    for i in 1..=4 {
        t.insert_no_fk(Item {
            id: ItemId(i),
            category: 1,
            status: Status::Active,
        })
        .unwrap();
    }
    // Inactive item in category 1 (should be excluded from filtered index).
    t.insert_no_fk(Item {
        id: ItemId(5),
        category: 1,
        status: Status::Inactive,
    })
    .unwrap();
    // Active item in category 2.
    t.insert_no_fk(Item {
        id: ItemId(6),
        category: 2,
        status: Status::Active,
    })
    .unwrap();
    t
}

#[test]
fn desc_on_filtered_index() {
    let table = make_item_table();
    let desc = table.by_active_category(&1u32, QueryOpts::DESC);
    let ids: Vec<ItemId> = desc.iter().map(|i| i.id).collect();
    // Only active items in category 1: ids 1,2,3,4. DESC → 4,3,2,1.
    assert_eq!(ids, vec![ItemId(4), ItemId(3), ItemId(2), ItemId(1)]);
}

#[test]
fn offset_on_filtered_index() {
    let table = make_item_table();
    let result = table.by_active_category(&1u32, QueryOpts::offset(2));
    let ids: Vec<ItemId> = result.iter().map(|i| i.id).collect();
    // ASC: 1,2,3,4 → skip 2 → 3,4
    assert_eq!(ids, vec![ItemId(3), ItemId(4)]);
}

#[test]
fn modify_each_on_filtered_index() {
    let mut table = make_item_table();
    let count = table.modify_each_by_active_category(&1u32, QueryOpts::ASC, |_pk, row| {
        // Mutate a non-indexed field.
        row.status = Status::Active; // no-op but exercises the path
    });
    // 4 active items in category 1.
    assert_eq!(count, 4);
}

// ============================================================================
// DESC on unique index (MEDIUM priority)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct EmployeeId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Employee {
    #[primary_key]
    pub id: EmployeeId,
    #[indexed(unique)]
    pub badge: u32,
    pub name: String,
}

#[test]
fn desc_on_unique_index() {
    let mut table = EmployeeTable::new();
    for i in 1..=5 {
        table
            .insert_no_fk(Employee {
                id: EmployeeId(i),
                badge: i * 100,
                name: format!("emp{i}"),
            })
            .unwrap();
    }

    // Range query on unique index.
    let desc = table.by_badge(200u32..=400u32, QueryOpts::DESC);
    let ids: Vec<EmployeeId> = desc.iter().map(|e| e.id).collect();
    // Badges 200,300,400 → employees 2,3,4 → DESC: 4,3,2
    assert_eq!(ids, vec![EmployeeId(4), EmployeeId(3), EmployeeId(2)]);
}

// ============================================================================
// iter_by with offset (MEDIUM priority)
// ============================================================================

#[test]
fn iter_by_with_offset() {
    let table = make_table();
    let ids: Vec<CreatureId> = table
        .iter_by_species(&Species::Elf, QueryOpts::offset(1))
        .map(|c| c.id)
        .collect();
    // Elves: 1, 3, 4. Skip 1 → 3, 4.
    assert_eq!(ids, vec![CreatureId(3), CreatureId(4)]);
}

#[test]
fn iter_by_desc_with_offset() {
    let table = make_table();
    let ids: Vec<CreatureId> = table
        .iter_by_species(&Species::Elf, QueryOpts::DESC.with_offset(1))
        .map(|c| c.id)
        .collect();
    // DESC: 4, 3, 1. Skip 1 → 3, 1.
    assert_eq!(ids, vec![CreatureId(3), CreatureId(1)]);
}

// ============================================================================
// Offset exactly equal to result count → empty (MEDIUM priority)
// ============================================================================

#[test]
fn offset_equal_to_count_returns_empty() {
    let table = make_table();
    let elves = table.by_species(&Species::Elf, QueryOpts::offset(3));
    assert!(elves.is_empty());
}

// ============================================================================
// DESC on empty result (MEDIUM priority)
// ============================================================================

#[test]
fn desc_on_empty_result() {
    let table = make_table();
    let result = table.by_species(&Species::Squirrel, QueryOpts::DESC);
    assert!(result.is_empty());
}

#[test]
fn desc_on_empty_table() {
    let table = CreatureTable::new();
    let result = table.by_species(&Species::Elf, QueryOpts::DESC);
    assert!(result.is_empty());
}

// ============================================================================
// LOW priority tests
// ============================================================================

#[test]
fn offset_zero_same_as_default() {
    let table = make_table();
    let default_result = table.by_species(&Species::Elf, QueryOpts::ASC);
    let zero_offset = table.by_species(&Species::Elf, QueryOpts::offset(0));
    assert_eq!(default_result, zero_offset);
}

#[test]
fn modify_each_then_index_query_still_correct() {
    let mut table = make_table();
    // Modify non-indexed fields via species index.
    table.modify_each_by_species(&Species::Elf, QueryOpts::ASC, |_pk, row| {
        row.name = "elf".into();
    });
    // Re-query via the SAME index — should still return the same rows.
    let elves = table.by_species(&Species::Elf, QueryOpts::ASC);
    assert_eq!(elves.len(), 3);
    for elf in &elves {
        assert_eq!(elf.name, "elf");
    }
    // Re-query via a DIFFERENT index — should also be correct.
    let hungry = table.by_hunger(&10u32, QueryOpts::ASC);
    assert_eq!(hungry.len(), 1);
    assert_eq!(hungry[0].name, "elf"); // was Aelindra, now "elf"
}

// ============================================================================
// Compound index: DESC with range on trailing field (MEDIUM priority)
// ============================================================================

#[test]
fn compound_desc_with_range_trailing_field() {
    let mut table = TaskTable::new();
    for i in 0..5 {
        table
            .insert_no_fk(Task {
                id: TaskId(i),
                assignee: 1,
                priority: i * 10,
                label: format!("task{i}"),
            })
            .unwrap();
    }

    // Range on trailing field (priority 10..=30) with DESC.
    let desc = table.by_assignee_priority(&1u32, 10u32..=30u32, QueryOpts::DESC);
    let ids: Vec<TaskId> = desc.iter().map(|t| t.id).collect();
    // Priorities 10,20,30 → tasks 1,2,3 → DESC: 3,2,1
    assert_eq!(ids, vec![TaskId(3), TaskId(2), TaskId(1)]);
}
