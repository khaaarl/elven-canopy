//! Integration tests for auto-increment primary keys.
//!
//! Tests both table-level (`insert_auto_no_fk`, `next_id`) and database-level
//! (`insert_*_auto`) auto-increment behavior.

use tabulosity::{Bounded, Database, Error, MatchAll, Table};

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

// =============================================================================
// Auto-increment + secondary indexes
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct WidgetId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Color {
    Red,
    Blue,
    Green,
}

#[derive(Table, Clone, Debug, PartialEq)]
struct Widget {
    #[primary_key(auto_increment)]
    pub id: WidgetId,
    #[indexed]
    pub color: Color,
    pub label: String,
}

#[test]
fn auto_insert_maintains_simple_index() {
    let mut table = WidgetTable::new();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Blue,
            label: "B".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "C".into(),
        })
        .unwrap();

    assert_eq!(table.count_by_color(&Color::Red), 2);
    assert_eq!(table.count_by_color(&Color::Blue), 1);
    assert_eq!(table.count_by_color(&Color::Green), 0);

    let reds = table.by_color(&Color::Red);
    assert_eq!(reds.len(), 2);
    assert_eq!(reds[0].label, "A");
    assert_eq!(reds[1].label, "C");
}

#[test]
fn auto_insert_then_remove_updates_index() {
    let mut table = WidgetTable::new();
    let id0 = table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "B".into(),
        })
        .unwrap();

    assert_eq!(table.count_by_color(&Color::Red), 2);

    table.remove_no_fk(&id0).unwrap();
    assert_eq!(table.count_by_color(&Color::Red), 1);
    assert_eq!(table.by_color(&Color::Red)[0].label, "B");

    // next_id should not have changed after remove.
    assert_eq!(table.next_id(), WidgetId(2));
}

#[test]
fn auto_insert_then_update_updates_index() {
    let mut table = WidgetTable::new();
    let id0 = table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "A".into(),
        })
        .unwrap();

    assert_eq!(table.count_by_color(&Color::Red), 1);
    assert_eq!(table.count_by_color(&Color::Blue), 0);

    table
        .update_no_fk(Widget {
            id: id0,
            color: Color::Blue,
            label: "A-updated".into(),
        })
        .unwrap();

    assert_eq!(table.count_by_color(&Color::Red), 0);
    assert_eq!(table.count_by_color(&Color::Blue), 1);
}

#[test]
fn update_does_not_bump_next_id() {
    let mut table = WidgetTable::new();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "A".into(),
        })
        .unwrap();
    assert_eq!(table.next_id(), WidgetId(1));

    table
        .update_no_fk(Widget {
            id: WidgetId(0),
            color: Color::Blue,
            label: "A-updated".into(),
        })
        .unwrap();
    // Update should not change next_id.
    assert_eq!(table.next_id(), WidgetId(1));
}

#[test]
fn upsert_update_path_does_not_bump_next_id_when_pk_below() {
    let mut table = WidgetTable::new();
    // Insert two items via auto: next_id = 2.
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Blue,
            label: "B".into(),
        })
        .unwrap();
    assert_eq!(table.next_id(), WidgetId(2));

    // Upsert on existing key 0 — should not change next_id.
    table.upsert_no_fk(Widget {
        id: WidgetId(0),
        color: Color::Green,
        label: "A-upserted".into(),
    });
    assert_eq!(table.next_id(), WidgetId(2));
    assert_eq!(table.count_by_color(&Color::Green), 1);
    assert_eq!(table.count_by_color(&Color::Red), 0);
}

#[test]
fn upsert_insert_path_with_high_pk_bumps_next_id() {
    let mut table = WidgetTable::new();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "A".into(),
        })
        .unwrap();
    assert_eq!(table.next_id(), WidgetId(1));

    // Upsert with new key above next_id.
    table.upsert_no_fk(Widget {
        id: WidgetId(10),
        color: Color::Blue,
        label: "Z".into(),
    });
    assert_eq!(table.next_id(), WidgetId(11));
    assert_eq!(table.count_by_color(&Color::Blue), 1);
}

// =============================================================================
// Auto-increment + compound indexes
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct EntryId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    Low,
    High,
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "color_priority", fields("color", "priority"))]
struct Entry {
    #[primary_key(auto_increment)]
    pub id: EntryId,
    #[indexed]
    pub color: Color,
    pub priority: Priority,
    pub label: String,
}

#[test]
fn auto_insert_maintains_compound_index() {
    let mut table = EntryTable::new();
    table
        .insert_auto_no_fk(|pk| Entry {
            id: pk,
            color: Color::Red,
            priority: Priority::High,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Entry {
            id: pk,
            color: Color::Red,
            priority: Priority::Low,
            label: "B".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Entry {
            id: pk,
            color: Color::Blue,
            priority: Priority::High,
            label: "C".into(),
        })
        .unwrap();

    // Simple index queries.
    assert_eq!(table.count_by_color(&Color::Red), 2);
    assert_eq!(table.count_by_color(&Color::Blue), 1);

    // Compound index queries.
    assert_eq!(
        table.count_by_color_priority(&Color::Red, &Priority::High),
        1
    );
    assert_eq!(
        table.count_by_color_priority(&Color::Red, &Priority::Low),
        1
    );
    assert_eq!(table.count_by_color_priority(&Color::Red, MatchAll), 2);
    assert_eq!(
        table.count_by_color_priority(&Color::Blue, &Priority::High),
        1
    );
    assert_eq!(
        table.count_by_color_priority(&Color::Blue, &Priority::Low),
        0
    );
}

#[test]
fn auto_insert_compound_index_after_remove() {
    let mut table = EntryTable::new();
    let id0 = table
        .insert_auto_no_fk(|pk| Entry {
            id: pk,
            color: Color::Red,
            priority: Priority::High,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Entry {
            id: pk,
            color: Color::Red,
            priority: Priority::High,
            label: "B".into(),
        })
        .unwrap();

    assert_eq!(
        table.count_by_color_priority(&Color::Red, &Priority::High),
        2
    );

    table.remove_no_fk(&id0).unwrap();
    assert_eq!(
        table.count_by_color_priority(&Color::Red, &Priority::High),
        1
    );
    assert_eq!(table.next_id(), EntryId(2));
}

// =============================================================================
// Auto-increment + filtered indexes
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct FiltId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "active_color",
    fields("color"),
    filter = "FiltEntry::is_active"
)]
struct FiltEntry {
    #[primary_key(auto_increment)]
    pub id: FiltId,
    #[indexed]
    pub color: Color,
    pub active: bool,
    pub label: String,
}

impl FiltEntry {
    fn is_active(&self) -> bool {
        self.active
    }
}

#[test]
fn auto_insert_maintains_filtered_index() {
    let mut table = FiltEntryTable::new();
    table
        .insert_auto_no_fk(|pk| FiltEntry {
            id: pk,
            color: Color::Red,
            active: true,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| FiltEntry {
            id: pk,
            color: Color::Red,
            active: false,
            label: "B".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| FiltEntry {
            id: pk,
            color: Color::Blue,
            active: true,
            label: "C".into(),
        })
        .unwrap();

    // Simple index includes all rows.
    assert_eq!(table.count_by_color(&Color::Red), 2);

    // Filtered index only includes active rows.
    assert_eq!(table.count_by_active_color(&Color::Red), 1);
    assert_eq!(table.count_by_active_color(&Color::Blue), 1);
    assert_eq!(table.count_by_active_color(MatchAll), 2);
}

#[test]
fn auto_insert_filtered_index_update_changes_membership() {
    let mut table = FiltEntryTable::new();
    let id0 = table
        .insert_auto_no_fk(|pk| FiltEntry {
            id: pk,
            color: Color::Red,
            active: true,
            label: "A".into(),
        })
        .unwrap();

    assert_eq!(table.count_by_active_color(&Color::Red), 1);

    // Deactivate: should leave the filtered index.
    table
        .update_no_fk(FiltEntry {
            id: id0,
            color: Color::Red,
            active: false,
            label: "A".into(),
        })
        .unwrap();
    assert_eq!(table.count_by_active_color(&Color::Red), 0);

    // Re-activate: should re-enter the filtered index.
    table
        .update_no_fk(FiltEntry {
            id: id0,
            color: Color::Red,
            active: true,
            label: "A".into(),
        })
        .unwrap();
    assert_eq!(table.count_by_active_color(&Color::Red), 1);
}

// =============================================================================
// Auto-increment + rebuild_indexes
// =============================================================================

#[test]
fn rebuild_indexes_on_auto_table() {
    let mut table = WidgetTable::new();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Blue,
            label: "B".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|pk| Widget {
            id: pk,
            color: Color::Red,
            label: "C".into(),
        })
        .unwrap();

    // rebuild_indexes should not affect next_id or data, just indexes.
    let next_before = table.next_id();
    table.rebuild_indexes();
    assert_eq!(table.next_id(), next_before);
    assert_eq!(table.len(), 3);
    assert_eq!(table.count_by_color(&Color::Red), 2);
    assert_eq!(table.count_by_color(&Color::Blue), 1);
}

// =============================================================================
// Auto-increment + FK cascade/nullify interactions
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CatId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Category {
    #[primary_key(auto_increment)]
    pub id: CatId,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ArticleId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Article {
    #[primary_key(auto_increment)]
    pub id: ArticleId,
    #[indexed]
    pub category: CatId,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TagId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Tag {
    #[primary_key(auto_increment)]
    pub id: TagId,
    #[indexed]
    pub category: Option<CatId>,
    pub name: String,
}

#[derive(Database)]
struct CascadeAutoDb {
    #[table(singular = "category", auto)]
    pub categories: CategoryTable,

    #[table(singular = "article", auto, fks(category = "categories" on_delete cascade))]
    pub articles: ArticleTable,

    #[table(singular = "tag", auto, fks(category? = "categories" on_delete nullify))]
    pub tags: TagTable,
}

#[test]
fn auto_cascade_removes_dependent_rows() {
    let mut db = CascadeAutoDb::new();
    let cat = db
        .insert_category_auto(|pk| Category {
            id: pk,
            name: "Tech".into(),
        })
        .unwrap();
    db.insert_article_auto(|pk| Article {
        id: pk,
        category: cat,
        title: "Rust".into(),
    })
    .unwrap();
    db.insert_article_auto(|pk| Article {
        id: pk,
        category: cat,
        title: "Go".into(),
    })
    .unwrap();

    db.remove_category(&cat).unwrap();
    assert!(db.categories.is_empty());
    assert!(db.articles.is_empty());
    // next_id should still be advanced (IDs are not reused).
    assert_eq!(db.articles.next_id(), ArticleId(2));
}

#[test]
fn auto_nullify_sets_fk_to_none() {
    let mut db = CascadeAutoDb::new();
    let cat = db
        .insert_category_auto(|pk| Category {
            id: pk,
            name: "Tech".into(),
        })
        .unwrap();
    let tag_id = db
        .insert_tag_auto(|pk| Tag {
            id: pk,
            category: Some(cat),
            name: "rust".into(),
        })
        .unwrap();

    db.remove_category(&cat).unwrap();
    assert!(db.categories.is_empty());
    assert_eq!(db.tags.len(), 1);
    assert_eq!(db.tags.get(&tag_id).unwrap().category, None);
}

#[test]
fn auto_cascade_and_nullify_together() {
    let mut db = CascadeAutoDb::new();
    let cat = db
        .insert_category_auto(|pk| Category {
            id: pk,
            name: "Tech".into(),
        })
        .unwrap();
    db.insert_article_auto(|pk| Article {
        id: pk,
        category: cat,
        title: "Rust".into(),
    })
    .unwrap();
    let tag_id = db
        .insert_tag_auto(|pk| Tag {
            id: pk,
            category: Some(cat),
            name: "rust".into(),
        })
        .unwrap();

    db.remove_category(&cat).unwrap();
    assert!(db.categories.is_empty());
    assert!(db.articles.is_empty());
    assert_eq!(db.tags.len(), 1);
    assert_eq!(db.tags.get(&tag_id).unwrap().category, None);
}

#[test]
fn auto_insert_after_cascade_uses_correct_next_id() {
    let mut db = CascadeAutoDb::new();
    let cat = db
        .insert_category_auto(|pk| Category {
            id: pk,
            name: "Tech".into(),
        })
        .unwrap();
    db.insert_article_auto(|pk| Article {
        id: pk,
        category: cat,
        title: "Rust".into(),
    })
    .unwrap();

    // Cascade-delete the category and its articles.
    db.remove_category(&cat).unwrap();

    // Insert new category and article — IDs should continue from where they left off.
    let cat2 = db
        .insert_category_auto(|pk| Category {
            id: pk,
            name: "Science".into(),
        })
        .unwrap();
    assert_eq!(cat2, CatId(1));

    let art = db
        .insert_article_auto(|pk| Article {
            id: pk,
            category: cat2,
            title: "Physics".into(),
        })
        .unwrap();
    assert_eq!(art, ArticleId(1));
}

// =============================================================================
// Database with mix of auto and non-auto tables
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ManualId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct ManualRow {
    #[primary_key]
    pub id: ManualId,
    pub value: String,
}

#[derive(Database)]
struct MixedAutoDb {
    #[table(singular = "manual_row")]
    pub manual_rows: ManualRowTable,

    #[table(singular = "project", auto)]
    pub projects: ProjectTable,
}

#[test]
fn mixed_auto_and_non_auto_db() {
    let mut db = MixedAutoDb::new();

    // Manual table: must provide PK.
    db.insert_manual_row(ManualRow {
        id: ManualId(42),
        value: "hello".into(),
    })
    .unwrap();

    // Auto table: PK is generated.
    let pid = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "Alpha".into(),
        })
        .unwrap();
    assert_eq!(pid, ProjectId(0));

    assert_eq!(db.manual_rows.len(), 1);
    assert_eq!(db.projects.len(), 1);
}

// =============================================================================
// Auto-increment + optional FK fields on the auto table
// =============================================================================

#[test]
fn auto_insert_with_optional_fk_none() {
    let mut db = CascadeAutoDb::new();
    let tag_id = db
        .insert_tag_auto(|pk| Tag {
            id: pk,
            category: None,
            name: "untagged".into(),
        })
        .unwrap();
    assert_eq!(tag_id, TagId(0));
    assert_eq!(db.tags.get(&tag_id).unwrap().category, None);
}

#[test]
fn auto_insert_with_optional_fk_invalid_target() {
    let mut db = CascadeAutoDb::new();
    let err = db
        .insert_tag_auto(|pk| Tag {
            id: pk,
            category: Some(CatId(99)),
            name: "bad".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
    // next_id should NOT have advanced because insert was rejected.
    assert_eq!(db.tags.next_id(), TagId(0));
}

#[test]
fn auto_insert_fk_failure_does_not_advance_next_id() {
    // This is a critical edge case: if the FK check fails, the auto-increment
    // counter should not advance.
    let mut db = AutoDb::new();
    let err = db
        .insert_task_auto(|pk| Task {
            id: pk,
            project: ProjectId(99),
            label: "orphan".into(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
    assert_eq!(db.tasks.next_id(), TaskId(0));

    // Now insert a valid project and task.
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
    assert_eq!(tid, TaskId(0)); // Should start at 0, not 1.
}

// =============================================================================
// Auto-increment index queries after DB operations
// =============================================================================

#[test]
fn db_auto_insert_secondary_index_queries() {
    let mut db = AutoDb::new();
    let p0 = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "Alpha".into(),
        })
        .unwrap();
    let p1 = db
        .insert_project_auto(|pk| Project {
            id: pk,
            name: "Beta".into(),
        })
        .unwrap();

    db.insert_task_auto(|pk| Task {
        id: pk,
        project: p0,
        label: "build".into(),
    })
    .unwrap();
    db.insert_task_auto(|pk| Task {
        id: pk,
        project: p0,
        label: "test".into(),
    })
    .unwrap();
    db.insert_task_auto(|pk| Task {
        id: pk,
        project: p1,
        label: "deploy".into(),
    })
    .unwrap();

    // Query tasks by project index.
    assert_eq!(db.tasks.count_by_project(&p0), 2);
    assert_eq!(db.tasks.count_by_project(&p1), 1);

    let p0_tasks = db.tasks.by_project(&p0);
    assert_eq!(p0_tasks.len(), 2);
    assert_eq!(p0_tasks[0].label, "build");
    assert_eq!(p0_tasks[1].label, "test");
}
