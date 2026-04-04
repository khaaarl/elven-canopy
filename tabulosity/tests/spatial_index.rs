//! Integration tests for spatial indexes (`kind = "spatial"`) and compound
//! spatial indexes (`fields("prefix", "pos" spatial)`).
//!
//! Tests the derive-macro-generated spatial index support: insert, update,
//! remove, intersection queries, Option<T> handling, filter support, rebuild,
//! and deterministic PK-sorted results. Compound spatial tests cover partitioned
//! R-trees keyed by prefix fields, with btree/hash prefix kinds, multi-prefix
//! fields, filter interactions, and partition cleanup. Serde roundtrip tests
//! for compound spatial live in `serde.rs`.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded)]
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

// =============================================================================
// Compound spatial indexes
// =============================================================================

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "zone_pos", fields("zone_id", "pos" spatial))]
struct ZonedEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub pos: Box3d,
}

#[test]
fn compound_spatial_basic_insert_and_query() {
    let mut table = ZonedEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(2),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    // Query zone 1 near origin — should find only entity 0.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    // Query zone 1 with large envelope — should find both zone 1 entities.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [20, 20, 20]));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, 0);
    assert_eq!(hits[1].id, 2);

    // Query zone 2 — should find only entity 1.
    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [20, 20, 20]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);

    // Query nonexistent zone — empty results.
    let hits = table.intersecting_by_zone_pos(&ZoneId(99), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    // iter_ and count_ variants.
    let count = table.count_intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(count, 1);
    let iter_hits: Vec<_> = table
        .iter_intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [20, 20, 20]))
        .collect();
    assert_eq!(iter_hits.len(), 2);
}

#[test]
fn compound_spatial_remove_with_partition_cleanup() {
    let mut table = ZonedEntityTable::new();

    let id0 = table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    let id1 = table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(2),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Remove the only entity in zone 2 — partition should be cleaned up.
    table.remove_no_fk(&id1).unwrap();

    // Zone 2 query returns empty (partition gone).
    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    // Zone 1 still has its entity.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id0);
}

#[test]
fn compound_spatial_update_prefix_change() {
    let mut table = ZonedEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Move entity from zone 1 to zone 2.
    table
        .update_no_fk(ZonedEntity {
            id: 0,
            zone_id: ZoneId(2),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Gone from zone 1.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    // Present in zone 2.
    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

#[test]
fn compound_spatial_update_spatial_change() {
    let mut table = ZonedEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Move within zone 1.
    table
        .update_no_fk(ZonedEntity {
            id: 0,
            zone_id: ZoneId(1),
            pos: Box3d::new([50, 50, 50], [55, 55, 55]),
        })
        .unwrap();

    // Old location empty.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [5, 5, 5]));
    assert!(hits.is_empty());

    // New location found.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([50, 50, 50], [55, 55, 55]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

#[test]
fn compound_spatial_update_both_prefix_and_spatial() {
    let mut table = ZonedEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Change both zone and position.
    table
        .update_no_fk(ZonedEntity {
            id: 0,
            zone_id: ZoneId(3),
            pos: Box3d::new([100, 100, 100], [105, 105, 105]),
        })
        .unwrap();

    // Gone from old zone+location.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [200, 200, 200]));
    assert!(hits.is_empty());

    // Present in new zone+location.
    let hits =
        table.intersecting_by_zone_pos(&ZoneId(3), &Box3d::new([100, 100, 100], [105, 105, 105]));
    assert_eq!(hits.len(), 1);
}

#[test]
fn compound_spatial_update_no_change() {
    let mut table = ZonedEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Update with identical values — should be a no-op.
    table
        .update_no_fk(ZonedEntity {
            id: 0,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [5, 5, 5]));
    assert_eq!(hits.len(), 1);
}

#[test]
fn compound_spatial_rebuild() {
    let mut table = ZonedEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(2),
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);
}

// --- Compound spatial with Optional spatial field ---

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "zone_pos", fields("zone_id", "pos" spatial))]
struct ZonedOptEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub pos: Option<Box3d>,
}

#[test]
fn compound_spatial_optional_spatial_field() {
    let mut table = ZonedOptEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: None,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(2),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();

    // Zone 1: only the Some entity matches.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    // Update None -> Some: should now appear in queries.
    table
        .update_no_fk(ZonedOptEntity {
            id: 1,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([10, 10, 10], [15, 15, 15])),
        })
        .unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 2);

    // Update Some -> None: should disappear from queries.
    table
        .update_no_fk(ZonedOptEntity {
            id: 0,
            zone_id: ZoneId(1),
            pos: None,
        })
        .unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);
}

// --- Compound spatial with filter ---

fn is_zoned_active(e: &ZonedFilterEntity) -> bool {
    e.active
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "zone_pos",
    fields("zone_id", "pos" spatial),
    filter = "is_zoned_active"
)]
struct ZonedFilterEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub pos: Box3d,
    pub active: bool,
}

#[test]
fn compound_spatial_filter_basic() {
    let mut table = ZonedFilterEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();

    // Only active entity is in the index.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

#[test]
fn compound_spatial_filter_update_transitions() {
    let mut table = ZonedFilterEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();

    // Deactivate (true → false): should leave the index.
    table
        .update_no_fk(ZonedFilterEntity {
            id: 0,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    // Reactivate (false → true): should re-enter the index.
    table
        .update_no_fk(ZonedFilterEntity {
            id: 0,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
}

#[test]
fn compound_spatial_filter_deactivate_cleans_up_partition() {
    let mut table = ZonedFilterEntityTable::new();

    // Only entity in zone 3 — deactivating it should clean up the partition.
    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(3),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();

    table
        .update_no_fk(ZonedFilterEntity {
            id: 0,
            zone_id: ZoneId(3),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: false,
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(&ZoneId(3), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());
}

// --- Compound spatial with hash prefix ---

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "zone_pos", fields("zone_id" hash, "pos" spatial))]
struct HashPrefixEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub pos: Box3d,
}

#[test]
fn compound_spatial_hash_prefix() {
    let mut table = HashPrefixEntityTable::new();

    table
        .insert_auto_no_fk(|id| HashPrefixEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashPrefixEntity {
            id,
            zone_id: ZoneId(2),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    // Remove and verify partition cleanup.
    table.remove_no_fk(&0).unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());
}

// --- Multi-prefix compound spatial ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded)]
struct CivId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "zone_civ_pos", fields("zone_id", "civ_id", "pos" spatial))]
struct MultiPrefixEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub civ_id: CivId,
    pub pos: Box3d,
}

#[test]
fn compound_spatial_multi_prefix() {
    let mut table = MultiPrefixEntityTable::new();

    table
        .insert_auto_no_fk(|id| MultiPrefixEntity {
            id,
            zone_id: ZoneId(1),
            civ_id: CivId(10),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| MultiPrefixEntity {
            id,
            zone_id: ZoneId(1),
            civ_id: CivId(20),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| MultiPrefixEntity {
            id,
            zone_id: ZoneId(2),
            civ_id: CivId(10),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Query zone 1 + civ 10.
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(10),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    // Query zone 1 + civ 20.
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(20),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);

    // Nonexistent combination.
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(2),
        &CivId(20),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert!(hits.is_empty());
}

// --- Hash primary storage + compound spatial ---

#[derive(Table, Clone, Debug, PartialEq)]
#[table(primary_storage = "hash")]
#[index(name = "zone_pos", fields("zone_id", "pos" spatial))]
struct HashStorageZonedEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub pos: Box3d,
}

#[test]
fn compound_spatial_hash_primary_storage() {
    let mut table = HashStorageZonedEntityTable::new();

    table
        .insert_auto_no_fk(|id| HashStorageZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashStorageZonedEntity {
            id,
            zone_id: ZoneId(2),
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    // Update and verify.
    table
        .update_no_fk(HashStorageZonedEntity {
            id: 0,
            zone_id: ZoneId(2),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());
    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 2);
}

// --- Upsert with compound spatial ---

#[test]
fn compound_spatial_upsert() {
    let mut table = ZonedEntityTable::new();

    // Upsert insert path.
    table.upsert_no_fk(ZonedEntity {
        id: 0,
        zone_id: ZoneId(1),
        pos: Box3d::new([0, 0, 0], [5, 5, 5]),
    });

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);

    // Upsert update path — change zone.
    table.upsert_no_fk(ZonedEntity {
        id: 0,
        zone_id: ZoneId(2),
        pos: Box3d::new([0, 0, 0], [5, 5, 5]),
    });

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());
    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
}

// --- modify_unchecked includes compound spatial fields ---

#[test]
#[cfg(debug_assertions)]
fn compound_spatial_modify_unchecked_asserts_prefix_field() {
    let mut table = ZonedEntityTable::new();
    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.modify_unchecked(&0, |row| {
            row.zone_id = ZoneId(99);
        })
    }));
    assert!(
        result.is_err(),
        "should panic when modifying indexed prefix field"
    );
}

#[test]
#[cfg(debug_assertions)]
fn compound_spatial_modify_unchecked_asserts_spatial_field() {
    let mut table = ZonedEntityTable::new();
    table
        .insert_auto_no_fk(|id| ZonedEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        table.modify_unchecked(&0, |row| {
            row.pos = Box3d::new([99, 99, 99], [100, 100, 100]);
        })
    }));
    assert!(
        result.is_err(),
        "should panic when modifying indexed spatial field"
    );
}

// --- Compound spatial with 2D spatial key ---

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "zone_pos", fields("zone_id", "pos" spatial))]
struct ZonedEntity2d {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub pos: Box2d,
}

#[test]
fn compound_spatial_2d() {
    let mut table = ZonedEntity2dTable::new();

    table
        .insert_auto_no_fk(|id| ZonedEntity2d {
            id,
            zone_id: ZoneId(1),
            pos: Box2d {
                min: [0, 0],
                max: [5, 5],
            },
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(
        &ZoneId(1),
        &Box2d {
            min: [0, 0],
            max: [10, 10],
        },
    );
    assert_eq!(hits.len(), 1);
}

// --- Equivalent single-field spatial via per-field annotation ---

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "pos_idx", fields("bounds" spatial))]
struct PerFieldAnnotatedEntity {
    #[primary_key]
    pub id: EntityId,
    pub bounds: Box3d,
}

#[test]
fn per_field_spatial_annotation_single_field() {
    // Verify that `fields("pos" spatial)` on a single field produces the same
    // query API as `kind = "spatial"` (not compound spatial).
    let mut table = PerFieldAnnotatedEntityTable::new();
    table
        .insert_no_fk(PerFieldAnnotatedEntity {
            id: EntityId(1),
            bounds: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Method name is `intersecting_pos_idx` (no "by_" prefix — single-field spatial).
    let hits = table.intersecting_pos_idx(&Box3d::new([0, 0, 0], [10, 10, 10]));
    assert_eq!(hits.len(), 1);
}

// --- Filter + prefix change (true,true branch with partition migration) ---

#[test]
fn compound_spatial_filter_with_prefix_change() {
    let mut table = ZonedFilterEntityTable::new();

    // Insert two active entities in zone 1.
    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
            active: true,
        })
        .unwrap();

    // Move entity 0 to zone 2 (prefix change while filter stays true).
    table
        .update_no_fk(ZonedFilterEntity {
            id: 0,
            zone_id: ZoneId(2),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();

    // Zone 1 should have only entity 1.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);

    // Zone 2 should have entity 0.
    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

// --- Rebuild with filter ---

#[test]
fn compound_spatial_rebuild_with_filter() {
    let mut table = ZonedFilterEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
            active: true,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedFilterEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
            active: false,
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    // Only active entity should be in the index after rebuild.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

// --- Rebuild with optional None ---

#[test]
fn compound_spatial_rebuild_with_optional_none() {
    let mut table = ZonedOptEntityTable::new();

    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: None,
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    // Only the Some entity should appear in spatial queries.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

// --- Multi-prefix partial update ---

#[test]
fn compound_spatial_multi_prefix_update_partial() {
    let mut table = MultiPrefixEntityTable::new();

    table
        .insert_auto_no_fk(|id| MultiPrefixEntity {
            id,
            zone_id: ZoneId(1),
            civ_id: CivId(10),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Change only civ_id (partial prefix change).
    table
        .update_no_fk(MultiPrefixEntity {
            id: 0,
            zone_id: ZoneId(1),
            civ_id: CivId(20),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Old partition (1, 10) should be empty.
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(10),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert!(hits.is_empty());

    // New partition (1, 20) should have the entity.
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(20),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

// --- Hash prefix update with prefix change ---

#[test]
fn compound_spatial_hash_prefix_update() {
    let mut table = HashPrefixEntityTable::new();

    table
        .insert_auto_no_fk(|id| HashPrefixEntity {
            id,
            zone_id: ZoneId(1),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Move to different zone (prefix change via hash-prefix codepath).
    table
        .update_no_fk(HashPrefixEntity {
            id: 0,
            zone_id: ZoneId(2),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    let hits = table.intersecting_by_zone_pos(&ZoneId(2), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);
}

// --- Option prefix field ---

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "zone_pos", fields("zone_id", "pos" spatial))]
struct OptPrefixEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: Option<ZoneId>,
    pub pos: Box3d,
}

#[test]
fn compound_spatial_option_prefix_field() {
    let mut table = OptPrefixEntityTable::new();

    // Insert with Some prefix.
    table
        .insert_auto_no_fk(|id| OptPrefixEntity {
            id,
            zone_id: Some(ZoneId(1)),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    // Insert with None prefix.
    table
        .insert_auto_no_fk(|id| OptPrefixEntity {
            id,
            zone_id: None,
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    // Another Some(1).
    table
        .insert_auto_no_fk(|id| OptPrefixEntity {
            id,
            zone_id: Some(ZoneId(1)),
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    // Query Some(1) partition.
    let hits =
        table.intersecting_by_zone_pos(&Some(ZoneId(1)), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, 0);
    assert_eq!(hits[1].id, 2);

    // Query None partition.
    let hits = table.intersecting_by_zone_pos(&None, &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);

    // Query nonexistent Some(99).
    let hits =
        table.intersecting_by_zone_pos(&Some(ZoneId(99)), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    // Update: move entity from None to Some(2).
    table
        .update_no_fk(OptPrefixEntity {
            id: 1,
            zone_id: Some(ZoneId(2)),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let hits = table.intersecting_by_zone_pos(&None, &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    let hits =
        table.intersecting_by_zone_pos(&Some(ZoneId(2)), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);
}

// --- Multi-prefix with hash kind ---

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "zone_civ_pos",
    fields("zone_id", "civ_id", "pos" spatial),
    kind = "hash"
)]
struct MultiPrefixHashEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub civ_id: CivId,
    pub pos: Box3d,
}

#[test]
fn compound_spatial_multi_prefix_hash() {
    let mut table = MultiPrefixHashEntityTable::new();

    table
        .insert_auto_no_fk(|id| MultiPrefixHashEntity {
            id,
            zone_id: ZoneId(1),
            civ_id: CivId(10),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| MultiPrefixHashEntity {
            id,
            zone_id: ZoneId(1),
            civ_id: CivId(20),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| MultiPrefixHashEntity {
            id,
            zone_id: ZoneId(2),
            civ_id: CivId(10),
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();

    // Query (zone=1, civ=10).
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(10),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    // Query (zone=1, civ=20).
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(20),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);

    // Update: change one prefix field.
    table
        .update_no_fk(MultiPrefixHashEntity {
            id: 0,
            zone_id: ZoneId(1),
            civ_id: CivId(20),
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Old partition (1,10) empty.
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(10),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert!(hits.is_empty());

    // New partition (1,20) has both.
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(20),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert_eq!(hits.len(), 2);

    // Remove and verify partition cleanup.
    table.remove_no_fk(&0).unwrap();
    table.remove_no_fk(&1).unwrap();
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(1),
        &CivId(20),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert!(hits.is_empty());

    // Rebuild and verify.
    table.manual_rebuild_all_indexes();
    let hits = table.intersecting_by_zone_civ_pos(
        &ZoneId(2),
        &CivId(10),
        &Box3d::new([0, 0, 0], [100, 100, 100]),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 2);
}

// --- Filter + optional spatial field ---

fn is_zoned_opt_active(e: &ZonedFilterOptEntity) -> bool {
    e.active
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "zone_pos",
    fields("zone_id", "pos" spatial),
    filter = "is_zoned_opt_active"
)]
struct ZonedFilterOptEntity {
    #[primary_key(auto_increment)]
    pub id: u32,
    pub zone_id: ZoneId,
    pub pos: Option<Box3d>,
    pub active: bool,
}

#[test]
fn compound_spatial_filter_with_optional_spatial() {
    let mut table = ZonedFilterOptEntityTable::new();

    // active + Some — enters R-tree partition.
    table
        .insert_auto_no_fk(|id| ZonedFilterOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
            active: true,
        })
        .unwrap();
    // active + None — enters none-set partition.
    table
        .insert_auto_no_fk(|id| ZonedFilterOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: None,
            active: true,
        })
        .unwrap();
    // inactive + Some — excluded by filter.
    table
        .insert_auto_no_fk(|id| ZonedFilterOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
            active: false,
        })
        .unwrap();

    // Only entity 0 (active + Some) in spatial queries.
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 0);

    // Deactivate entity 0 (true→false, Some spatial).
    table
        .update_no_fk(ZonedFilterOptEntity {
            id: 0,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
            active: false,
        })
        .unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());

    // Activate entity 2 (false→true, Some spatial).
    table
        .update_no_fk(ZonedFilterOptEntity {
            id: 2,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
            active: true,
        })
        .unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 2);
}

// --- Remove last none-entry cleans partition ---

#[test]
fn compound_spatial_remove_last_none_entry_cleans_partition() {
    let mut table = ZonedOptEntityTable::new();

    // Insert a single entity with pos: None in zone 1.
    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: None,
        })
        .unwrap();

    // Remove it — the partition (R-tree empty, none-set now empty) should be cleaned up.
    table.remove_no_fk(&0).unwrap();

    // Insert a new entity in zone 1 — partition should be recreated cleanly.
    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: Some(Box3d::new([0, 0, 0], [5, 5, 5])),
        })
        .unwrap();
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);
}

// --- Optional spatial prefix change with None value ---

#[test]
fn compound_spatial_update_prefix_change_with_none_spatial() {
    let mut table = ZonedOptEntityTable::new();

    // Insert with pos: None in zone 1.
    table
        .insert_auto_no_fk(|id| ZonedOptEntity {
            id,
            zone_id: ZoneId(1),
            pos: None,
        })
        .unwrap();

    // Change prefix (zone 1 → zone 2) while spatial stays None.
    // This migrates the entry between none-set partitions.
    table
        .update_no_fk(ZonedOptEntity {
            id: 0,
            zone_id: ZoneId(2),
            pos: None,
        })
        .unwrap();

    // Zone 1 should be empty (partition cleaned up).
    let hits = table.intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert!(hits.is_empty());
    let count =
        table.count_intersecting_by_zone_pos(&ZoneId(1), &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(count, 0);

    // Entity still exists (just not in spatial queries since pos is None).
    assert!(table.contains(&0));
}

// --- Compound PK + compound spatial ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded)]
struct RegionId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("region", "slot")]
#[index(name = "cat_pos", fields("category", "pos" spatial))]
struct CompoundPkZonedEntity {
    pub region: RegionId,
    pub slot: SlotId,
    pub category: u32,
    pub pos: Box3d,
}

#[test]
fn compound_spatial_with_compound_pk() {
    let mut table = CompoundPkZonedEntityTable::new();

    table
        .insert_no_fk(CompoundPkZonedEntity {
            region: RegionId(1),
            slot: SlotId(1),
            category: 10,
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();
    table
        .insert_no_fk(CompoundPkZonedEntity {
            region: RegionId(1),
            slot: SlotId(2),
            category: 10,
            pos: Box3d::new([10, 10, 10], [15, 15, 15]),
        })
        .unwrap();
    table
        .insert_no_fk(CompoundPkZonedEntity {
            region: RegionId(2),
            slot: SlotId(1),
            category: 20,
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    // Query category 10.
    let hits = table.intersecting_by_cat_pos(&10, &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].region, RegionId(1));
    assert_eq!(hits[0].slot, SlotId(1));
    assert_eq!(hits[1].region, RegionId(1));
    assert_eq!(hits[1].slot, SlotId(2));

    // Query category 20.
    let hits = table.intersecting_by_cat_pos(&20, &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);

    // Update: change category (prefix change with compound PK).
    table
        .update_no_fk(CompoundPkZonedEntity {
            region: RegionId(1),
            slot: SlotId(1),
            category: 20,
            pos: Box3d::new([0, 0, 0], [5, 5, 5]),
        })
        .unwrap();

    let hits = table.intersecting_by_cat_pos(&10, &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
    let hits = table.intersecting_by_cat_pos(&20, &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 2);

    // Remove with compound PK.
    table.remove_no_fk(&(RegionId(1), SlotId(1))).unwrap();
    let hits = table.intersecting_by_cat_pos(&20, &Box3d::new([0, 0, 0], [100, 100, 100]));
    assert_eq!(hits.len(), 1);
}
