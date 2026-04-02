//! Integration tests for spatial indexes (`kind = "spatial"`).
//!
//! Tests the derive-macro-generated spatial index support: insert, update,
//! remove, intersection queries, Option<T> handling, filter support, rebuild,
//! and deterministic PK-sorted results.

use tabulosity::{Bounded, SpatialKey, Table};

// =============================================================================
// Test spatial key type
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
struct Box3d {
    min: [i32; 3],
    max: [i32; 3],
}

impl Box3d {
    fn new(min: [i32; 3], max: [i32; 3]) -> Self {
        Self { min, max }
    }
}

impl SpatialKey for Box3d {
    type Point = [i32; 3];
    fn spatial_min(&self) -> [i32; 3] {
        self.min
    }
    fn spatial_max(&self) -> [i32; 3] {
        self.max
    }
}

// =============================================================================
// Basic spatial index on a non-optional field
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded)]
struct EntityId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Entity {
    #[primary_key]
    pub id: EntityId,
    #[indexed(spatial)]
    pub bounds: Box3d,
}

#[test]
fn spatial_insert_and_intersecting_query() {
    let mut table = EntityTable::new();
    table
        .insert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_no_fk(Entity {
            id: EntityId(2),
            bounds: Box3d::new([3, 3, 3], [8, 8, 8]),
        })
        .unwrap();
    table
        .insert_no_fk(Entity {
            id: EntityId(3),
            bounds: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    // Query that hits first two entities
    let hits = table.intersecting_bounds(&Box3d::new([4, 4, 4], [6, 6, 6]));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, EntityId(1));
    assert_eq!(hits[1].id, EntityId(2));

    // Query that hits only the third
    let hits = table.intersecting_bounds(&Box3d::new([12, 12, 12], [13, 13, 13]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(3));

    // Query that hits nothing
    let hits = table.intersecting_bounds(&Box3d::new([20, 20, 20], [25, 25, 25]));
    assert!(hits.is_empty());
}

#[test]
fn spatial_remove() {
    let mut table = EntityTable::new();
    table
        .insert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [10, 10, 10]),
        })
        .unwrap();
    table
        .insert_no_fk(Entity {
            id: EntityId(2),
            bounds: Box3d::new([0, 0, 0], [10, 10, 10]),
        })
        .unwrap();

    table.remove_no_fk(&EntityId(1)).unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [10, 10, 10]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(2));
}

#[test]
fn spatial_update_moves_entry() {
    let mut table = EntityTable::new();
    table
        .insert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Move entity to a different location
    table
        .update_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([20, 20, 20], [25, 25, 25]),
        })
        .unwrap();

    // Old location — should not find it
    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert!(hits.is_empty());

    // New location — should find it
    let hits = table.intersecting_bounds(&Box3d::new([20, 20, 20], [25, 25, 25]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

#[test]
fn spatial_deterministic_pk_ordering() {
    let mut table = EntityTable::new();
    // Insert in reverse PK order, all overlapping
    for i in (1..=10).rev() {
        table
            .insert_no_fk(Entity {
                id: EntityId(i),
                bounds: Box3d::new([0, 0, 0], [10, 10, 10]),
            })
            .unwrap();
    }
    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [10, 10, 10]));
    let ids: Vec<u32> = hits.iter().map(|e| e.id.0).collect();
    assert_eq!(ids, (1..=10).collect::<Vec<u32>>());
}

// =============================================================================
// Optional spatial field
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
struct OptEntity {
    #[primary_key]
    pub id: EntityId,
    #[indexed(spatial)]
    pub bounds: Option<Box3d>,
}

#[test]
fn spatial_optional_some_entries_indexed() {
    let mut table = OptEntityTable::new();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(2),
            bounds: None,
        })
        .unwrap();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(3),
            bounds: Some(Box3d::new([3, 3, 3], [8, 8, 8])),
        })
        .unwrap();

    // Only entities with Some bounds should appear
    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [10, 10, 10]));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, EntityId(1));
    assert_eq!(hits[1].id, EntityId(3));
}

#[test]
fn spatial_optional_none_to_some_update() {
    let mut table = OptEntityTable::new();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(1),
            bounds: None,
        })
        .unwrap();

    // Update from None to Some
    table
        .update_no_fk(OptEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

#[test]
fn spatial_optional_some_to_none_update() {
    let mut table = OptEntityTable::new();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();

    // Update from Some to None
    table
        .update_no_fk(OptEntity {
            id: EntityId(1),
            bounds: None,
        })
        .unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert!(hits.is_empty());
}

// =============================================================================
// Filtered spatial index
// =============================================================================

fn is_active(entity: &FilterEntity) -> bool {
    entity.active
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "position",
    fields("bounds"),
    kind = "spatial",
    filter = "is_active"
)]
struct FilterEntity {
    #[primary_key]
    pub id: EntityId,
    pub bounds: Box3d,
    pub active: bool,
}

#[test]
fn spatial_filter_excludes_inactive() {
    let mut table = FilterEntityTable::new();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(2),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();

    let hits = table.intersecting_position(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

#[test]
fn spatial_filter_update_becomes_active() {
    let mut table = FilterEntityTable::new();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();

    // Becomes active
    table
        .update_no_fk(FilterEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();

    let hits = table.intersecting_position(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
}

#[test]
fn spatial_filter_update_becomes_inactive() {
    let mut table = FilterEntityTable::new();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();

    // Becomes inactive
    table
        .update_no_fk(FilterEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();

    let hits = table.intersecting_position(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert!(hits.is_empty());
}

// =============================================================================
// Rebuild (manual_rebuild_all_indexes)
// =============================================================================

#[test]
fn spatial_rebuild_all_indexes() {
    let mut table = EntityTable::new();
    table
        .insert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_no_fk(Entity {
            id: EntityId(2),
            bounds: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    // Rebuild and verify queries still work
    table.manual_rebuild_all_indexes();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

// =============================================================================
// Upsert
// =============================================================================

#[test]
fn spatial_upsert_insert() {
    let mut table = EntityTable::new();
    table
        .upsert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
}

#[test]
fn spatial_upsert_update() {
    let mut table = EntityTable::new();
    table
        .insert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Upsert moves the entity
    table
        .upsert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([20, 20, 20], [25, 25, 25]),
        })
        .unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert!(hits.is_empty());
    let hits = table.intersecting_bounds(&Box3d::new([20, 20, 20], [25, 25, 25]));
    assert_eq!(hits.len(), 1);
}

// =============================================================================
// iter_intersecting and count_intersecting
// =============================================================================

#[test]
fn spatial_iter_intersecting() {
    let mut table = EntityTable::new();
    table
        .insert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_no_fk(Entity {
            id: EntityId(2),
            bounds: Box3d::new([3, 3, 3], [8, 8, 8]),
        })
        .unwrap();

    let ids: Vec<EntityId> = table
        .iter_intersecting_bounds(&Box3d::new([4, 4, 4], [6, 6, 6]))
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, vec![EntityId(1), EntityId(2)]);
}

#[test]
fn spatial_count_intersecting() {
    let mut table = EntityTable::new();
    table
        .insert_no_fk(Entity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_no_fk(Entity {
            id: EntityId(2),
            bounds: Box3d::new([3, 3, 3], [8, 8, 8]),
        })
        .unwrap();

    assert_eq!(
        table.count_intersecting_bounds(&Box3d::new([4, 4, 4], [6, 6, 6])),
        2
    );
    assert_eq!(
        table.count_intersecting_bounds(&Box3d::new([20, 20, 20], [25, 25, 25])),
        0
    );
}

// =============================================================================
// 2D spatial index
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
struct Box2d {
    min: [i32; 2],
    max: [i32; 2],
}

impl Box2d {
    fn new(min: [i32; 2], max: [i32; 2]) -> Self {
        Self { min, max }
    }
}

impl SpatialKey for Box2d {
    type Point = [i32; 2];
    fn spatial_min(&self) -> [i32; 2] {
        self.min
    }
    fn spatial_max(&self) -> [i32; 2] {
        self.max
    }
}

#[derive(Table, Clone, Debug, PartialEq)]
struct Tile {
    #[primary_key]
    pub id: EntityId,
    #[indexed(spatial)]
    pub area: Box2d,
}

#[test]
fn spatial_2d_index() {
    let mut table = TileTable::new();
    table
        .insert_no_fk(Tile {
            id: EntityId(1),
            area: Box2d::new([0, 0], [5, 5]),
        })
        .unwrap();
    table
        .insert_no_fk(Tile {
            id: EntityId(2),
            area: Box2d::new([10, 10], [15, 15]),
        })
        .unwrap();

    let hits = table.intersecting_area(&Box2d::new([3, 3], [6, 6]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));

    let hits = table.intersecting_area(&Box2d::new([0, 0], [20, 20]));
    assert_eq!(hits.len(), 2);
}

// =============================================================================
// Struct-level #[index(kind = "spatial", ...)] syntax
// =============================================================================

fn is_visible(entity: &StructLevelEntity) -> bool {
    entity.visible
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "pos",
    fields("bounds"),
    kind = "spatial",
    filter = "is_visible"
)]
struct StructLevelEntity {
    #[primary_key]
    pub id: EntityId,
    pub bounds: Box3d,
    pub visible: bool,
}

#[test]
fn spatial_struct_level_index() {
    let mut table = StructLevelEntityTable::new();
    table
        .insert_no_fk(StructLevelEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            visible: true,
        })
        .unwrap();
    table
        .insert_no_fk(StructLevelEntity {
            id: EntityId(2),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            visible: false,
        })
        .unwrap();

    let hits = table.intersecting_pos(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

// =============================================================================
// modify_unchecked should include spatial fields in debug checks
// =============================================================================

#[test]
fn spatial_modify_unchecked_non_spatial_field() {
    // modify_unchecked on a non-indexed field should succeed
    let mut table = StructLevelEntityTable::new();
    table
        .insert_no_fk(StructLevelEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            visible: true,
        })
        .unwrap();

    table.modify_unchecked(&EntityId(1), |row| {
        row.visible = false;
    });

    let row = table.get(&EntityId(1)).unwrap();
    assert!(!row.visible);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "indexed field")]
fn spatial_modify_unchecked_spatial_field_panics_debug() {
    // modify_unchecked on a spatial-indexed field should panic in debug
    let mut table = StructLevelEntityTable::new();
    table
        .insert_no_fk(StructLevelEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            visible: true,
        })
        .unwrap();

    table.modify_unchecked(&EntityId(1), |row| {
        row.bounds = Box3d::new([10, 10, 10], [15, 15, 15]);
    });
}

// =============================================================================
// Hash primary storage + spatial index
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
#[table(primary_storage = "hash")]
struct HashEntity {
    #[primary_key]
    pub id: EntityId,
    #[indexed(spatial)]
    pub bounds: Option<Box3d>,
}

#[test]
fn spatial_hash_primary_storage_insert_query() {
    let mut table = HashEntityTable::new();
    table
        .insert_no_fk(HashEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();
    table
        .insert_no_fk(HashEntity {
            id: EntityId(2),
            bounds: None,
        })
        .unwrap();
    table
        .insert_no_fk(HashEntity {
            id: EntityId(3),
            bounds: Some(Box3d::new([3, 3, 3], [8, 8, 8])),
        })
        .unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([4, 4, 4], [6, 6, 6]));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, EntityId(1));
    assert_eq!(hits[1].id, EntityId(3));
}

#[test]
fn spatial_hash_primary_storage_remove() {
    let mut table = HashEntityTable::new();
    table
        .insert_no_fk(HashEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();
    table
        .insert_no_fk(HashEntity {
            id: EntityId(2),
            bounds: None,
        })
        .unwrap();

    table.remove_no_fk(&EntityId(1)).unwrap();
    table.remove_no_fk(&EntityId(2)).unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [10, 10, 10]));
    assert!(hits.is_empty());
    assert!(table.is_empty());
}

// =============================================================================
// Filter + Option<T> combined
// =============================================================================

fn is_opt_active(entity: &FilterOptEntity) -> bool {
    entity.active
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "pos",
    fields("bounds"),
    kind = "spatial",
    filter = "is_opt_active"
)]
struct FilterOptEntity {
    #[primary_key]
    pub id: EntityId,
    pub bounds: Option<Box3d>,
    pub active: bool,
}

#[test]
fn spatial_filter_plus_optional_excludes_both() {
    let mut table = FilterOptEntityTable::new();
    // Active + Some — should appear
    table
        .insert_no_fk(FilterOptEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
            active: true,
        })
        .unwrap();
    // Active + None — excluded (no geometry)
    table
        .insert_no_fk(FilterOptEntity {
            id: EntityId(2),
            bounds: None,
            active: true,
        })
        .unwrap();
    // Inactive + Some — excluded by filter
    table
        .insert_no_fk(FilterOptEntity {
            id: EntityId(3),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
            active: false,
        })
        .unwrap();
    // Inactive + None — excluded by both
    table
        .insert_no_fk(FilterOptEntity {
            id: EntityId(4),
            bounds: None,
            active: false,
        })
        .unwrap();

    let hits = table.intersecting_pos(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

// =============================================================================
// Optional Some -> different Some update
// =============================================================================

#[test]
fn spatial_optional_some_to_different_some_update() {
    let mut table = OptEntityTable::new();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();

    // Update from Some(A) to Some(B) — different AABB
    table
        .update_no_fk(OptEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([20, 20, 20], [25, 25, 25])),
        })
        .unwrap();

    // Old location empty
    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert!(hits.is_empty());

    // New location found
    let hits = table.intersecting_bounds(&Box3d::new([20, 20, 20], [25, 25, 25]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

// =============================================================================
// Compound PK + spatial index
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ZoneId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct SlotId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("zone", "slot")]
struct CompoundEntity {
    pub zone: ZoneId,
    pub slot: SlotId,
    #[indexed(spatial)]
    pub bounds: Box3d,
}

#[test]
fn spatial_compound_pk() {
    let mut table = CompoundEntityTable::new();
    table
        .insert_no_fk(CompoundEntity {
            zone: ZoneId(1),
            slot: SlotId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_no_fk(CompoundEntity {
            zone: ZoneId(1),
            slot: SlotId(2),
            bounds: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].zone, ZoneId(1));
    assert_eq!(hits[0].slot, SlotId(1));

    // Update via compound PK
    table
        .update_no_fk(CompoundEntity {
            zone: ZoneId(1),
            slot: SlotId(1),
            bounds: Box3d::new([20, 20, 20], [25, 25, 25]),
        })
        .unwrap();
    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert!(hits.is_empty());

    // Remove via compound PK
    table.remove_no_fk(&(ZoneId(1), SlotId(2))).unwrap();
    assert_eq!(table.len(), 1);
}

// =============================================================================
// Rebuild with filter and with optional None
// =============================================================================

#[test]
fn spatial_rebuild_with_filter() {
    let mut table = FilterEntityTable::new();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(2),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    let hits = table.intersecting_position(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

#[test]
fn spatial_rebuild_with_optional_none() {
    let mut table = OptEntityTable::new();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(1),
            bounds: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();
    table
        .insert_no_fk(OptEntity {
            id: EntityId(2),
            bounds: None,
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [10, 10, 10]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(1));
}

// =============================================================================
// count_intersecting on filtered table
// =============================================================================

#[test]
fn spatial_count_intersecting_respects_filter() {
    let mut table = FilterEntityTable::new();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();
    table
        .insert_no_fk(FilterEntity {
            id: EntityId(2),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();

    // count should match intersecting().len()
    let count = table.count_intersecting_position(&Box3d::new([0, 0, 0], [5, 5, 5]));
    let hits = table.intersecting_position(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(count, 1);
    assert_eq!(count, hits.len());
}

// =============================================================================
// Spatial coexists with btree index
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
struct DualIndexEntity {
    #[primary_key]
    pub id: EntityId,
    #[indexed(spatial)]
    pub bounds: Box3d,
    #[indexed]
    pub category: u32,
}

#[test]
fn spatial_coexists_with_btree_index() {
    let mut table = DualIndexEntityTable::new();
    table
        .insert_no_fk(DualIndexEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            category: 1,
        })
        .unwrap();
    table
        .insert_no_fk(DualIndexEntity {
            id: EntityId(2),
            bounds: Box3d::new([10, 10, 10], [15, 15, 15]),
            category: 1,
        })
        .unwrap();
    table
        .insert_no_fk(DualIndexEntity {
            id: EntityId(3),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
            category: 2,
        })
        .unwrap();

    // Spatial query
    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, EntityId(1));
    assert_eq!(hits[1].id, EntityId(3));

    // BTree query
    let cat1 = table.by_category(&1, tabulosity::QueryOpts::ASC);
    assert_eq!(cat1.len(), 2);
    assert_eq!(cat1[0].id, EntityId(1));
    assert_eq!(cat1[1].id, EntityId(2));

    // Update — both indexes should reflect the change
    table
        .update_no_fk(DualIndexEntity {
            id: EntityId(1),
            bounds: Box3d::new([20, 20, 20], [25, 25, 25]),
            category: 2,
        })
        .unwrap();

    let hits = table.intersecting_bounds(&Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, EntityId(3));

    let cat1 = table.by_category(&1, tabulosity::QueryOpts::ASC);
    assert_eq!(cat1.len(), 1);
    assert_eq!(cat1[0].id, EntityId(2));
}
