//! Integration tests for parent-PK-as-child-PK (1:1 relations).
//!
//! Tests the `pk` keyword in FK declarations, which marks a foreign key field
//! as also being the child table's primary key. This enables 1:1 parent-child
//! relationships where the child row's PK is simultaneously an FK to the parent.

use tabulosity::{Bounded, Database, Error, Table};

// --- Row types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CreatureId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
}

/// 1:1 extension table — its PK is also an FK to creatures.
#[derive(Table, Clone, Debug, PartialEq)]
struct CreatureStats {
    #[primary_key]
    pub id: CreatureId,
    pub health: u32,
    pub mana: u32,
}

/// Another 1:1 extension table with cascade delete.
#[derive(Table, Clone, Debug, PartialEq)]
struct CreaturePosition {
    #[primary_key]
    pub id: CreatureId,
    pub x: i32,
    pub y: i32,
}

#[derive(Database)]
struct TestDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk))]
    pub creature_stats: CreatureStatsTable,

    #[table(singular = "creature_position", fks(id = "creatures" pk on_delete cascade))]
    pub creature_positions: CreaturePositionTable,
}

// --- Helpers ---

fn make_creature(id: u32, name: &str) -> Creature {
    Creature {
        id: CreatureId(id),
        name: name.to_string(),
    }
}

fn make_stats(id: u32, health: u32, mana: u32) -> CreatureStats {
    CreatureStats {
        id: CreatureId(id),
        health,
        mana,
    }
}

fn make_position(id: u32, x: i32, y: i32) -> CreaturePosition {
    CreaturePosition {
        id: CreatureId(id),
        x,
        y,
    }
}

// --- Insert tests ---

#[test]
fn insert_child_with_existing_parent_succeeds() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    assert_eq!(db.creature_stats.len(), 1);
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().health, 100);
}

#[test]
fn insert_child_without_parent_fails() {
    let mut db = TestDb::new();
    let err = db
        .insert_creature_stats(make_stats(1, 100, 50))
        .unwrap_err();
    assert!(
        matches!(err, Error::FkTargetNotFound { .. }),
        "expected FkTargetNotFound, got: {err:?}"
    );
}

#[test]
fn insert_multiple_children_for_different_parents() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_stats(make_stats(2, 80, 30)).unwrap();
    assert_eq!(db.creature_stats.len(), 2);
}

#[test]
fn insert_duplicate_child_pk_fails() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    let err = db.insert_creature_stats(make_stats(1, 80, 30)).unwrap_err();
    assert!(
        matches!(err, Error::DuplicateKey { .. }),
        "expected DuplicateKey, got: {err:?}"
    );
}

// --- Update tests ---

#[test]
fn update_child_succeeds() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.update_creature_stats(make_stats(1, 80, 60)).unwrap();
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().health, 80);
}

#[test]
fn update_child_nonexistent_fails() {
    let mut db = TestDb::new();
    let err = db
        .update_creature_stats(make_stats(1, 100, 50))
        .unwrap_err();
    assert!(
        matches!(err, Error::FkTargetNotFound { .. } | Error::NotFound { .. }),
        "expected FkTargetNotFound or NotFound, got: {err:?}"
    );
}

// --- Upsert tests ---

#[test]
fn upsert_child_insert_path() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.upsert_creature_stats(make_stats(1, 100, 50)).unwrap();
    assert_eq!(db.creature_stats.len(), 1);
}

#[test]
fn upsert_child_update_path() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.upsert_creature_stats(make_stats(1, 80, 60)).unwrap();
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().health, 80);
    assert_eq!(db.creature_stats.len(), 1);
}

#[test]
fn upsert_child_without_parent_fails() {
    let mut db = TestDb::new();
    let err = db
        .upsert_creature_stats(make_stats(1, 100, 50))
        .unwrap_err();
    assert!(
        matches!(err, Error::FkTargetNotFound { .. }),
        "expected FkTargetNotFound, got: {err:?}"
    );
}

// --- Delete with restrict (default) ---

#[test]
fn remove_parent_with_restrict_child_fails() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert!(
        matches!(err, Error::FkViolation { .. }),
        "expected FkViolation, got: {err:?}"
    );
    // Parent should still exist.
    assert!(db.creatures.contains(&CreatureId(1)));
    // Child should still exist.
    assert!(db.creature_stats.contains(&CreatureId(1)));
}

#[test]
fn remove_parent_without_child_succeeds() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
}

#[test]
fn remove_child_directly_succeeds() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.remove_creature_stats(&CreatureId(1)).unwrap();
    assert_eq!(db.creature_stats.len(), 0);
    // Parent should still exist.
    assert!(db.creatures.contains(&CreatureId(1)));
}

// --- Delete with cascade ---

#[test]
fn remove_parent_cascades_to_child() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_positions.len(), 0);
}

#[test]
fn remove_parent_cascades_position_but_blocks_on_stats() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();
    // Should fail because creature_stats has restrict.
    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert!(
        matches!(err, Error::FkViolation { .. }),
        "expected FkViolation, got: {err:?}"
    );
    // Everything should still exist (restrict blocks before cascade runs).
    assert!(db.creatures.contains(&CreatureId(1)));
    assert!(db.creature_stats.contains(&CreatureId(1)));
    assert!(db.creature_positions.contains(&CreatureId(1)));
}

#[test]
fn remove_parent_cascade_when_no_child_exists() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    // No position inserted — cascade should be a no-op.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
}

// --- Mixed: restrict child removed, then parent removal succeeds ---

#[test]
fn remove_restrict_child_then_parent_succeeds() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.remove_creature_stats(&CreatureId(1)).unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_stats.len(), 0);
}

// --- Multiple parents, selective operations ---

#[test]
fn cascade_only_affects_matching_child() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();
    db.insert_creature_position(make_position(2, 30, 40))
        .unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creature_positions.len(), 1);
    assert!(db.creature_positions.contains(&CreatureId(2)));
    assert!(!db.creature_positions.contains(&CreatureId(1)));
}

#[test]
fn restrict_only_affects_matching_child() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    // Creature 2 has no stats — should be removable.
    db.remove_creature(&CreatureId(2)).unwrap();
    assert_eq!(db.creatures.len(), 1);
    // Creature 1 should be blocked by stats.
    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert!(matches!(err, Error::FkViolation { .. }));
}

// --- Both pk-FK and regular FK on the same child table ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct EquipmentId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Equipment {
    #[primary_key]
    pub id: EquipmentId,
    pub name: String,
}

/// Child table whose PK is FK to creatures, with an additional regular FK.
#[derive(Table, Clone, Debug, PartialEq)]
struct CreatureEquipment {
    #[primary_key]
    pub id: CreatureId,
    #[indexed]
    pub weapon: EquipmentId,
}

#[derive(Database)]
struct MixedFkDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "equipment")]
    pub equipment: EquipmentTable,

    #[table(singular = "creature_equipment", fks(id = "creatures" pk on_delete cascade, weapon = "equipment"))]
    pub creature_equipment: CreatureEquipmentTable,
}

#[test]
fn mixed_fk_insert_both_parents_exist() {
    let mut db = MixedFkDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_equipment(Equipment {
        id: EquipmentId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    db.insert_creature_equipment(CreatureEquipment {
        id: CreatureId(1),
        weapon: EquipmentId(10),
    })
    .unwrap();
    assert_eq!(db.creature_equipment.len(), 1);
}

#[test]
fn mixed_fk_insert_pk_parent_missing_fails() {
    let mut db = MixedFkDb::new();
    db.insert_equipment(Equipment {
        id: EquipmentId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    let err = db
        .insert_creature_equipment(CreatureEquipment {
            id: CreatureId(1),
            weapon: EquipmentId(10),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn mixed_fk_insert_regular_parent_missing_fails() {
    let mut db = MixedFkDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    let err = db
        .insert_creature_equipment(CreatureEquipment {
            id: CreatureId(1),
            weapon: EquipmentId(10),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn mixed_fk_cascade_on_pk_parent_delete() {
    let mut db = MixedFkDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_equipment(Equipment {
        id: EquipmentId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    db.insert_creature_equipment(CreatureEquipment {
        id: CreatureId(1),
        weapon: EquipmentId(10),
    })
    .unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creature_equipment.len(), 0);
}

#[test]
fn mixed_fk_restrict_on_regular_parent_delete() {
    let mut db = MixedFkDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_equipment(Equipment {
        id: EquipmentId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    db.insert_creature_equipment(CreatureEquipment {
        id: CreatureId(1),
        weapon: EquipmentId(10),
    })
    .unwrap();
    let err = db.remove_equipment(&EquipmentId(10)).unwrap_err();
    assert!(matches!(err, Error::FkViolation { .. }));
}

// =========================================================================
// Cascade chains: grandparent → parent (pk FK) → grandchild (pk FK)
// =========================================================================

#[derive(Table, Clone, Debug, PartialEq)]
struct CreatureAura {
    #[primary_key]
    pub id: CreatureId,
    pub color: String,
}

/// 3-level cascade chain: Creature → CreatureStats → CreatureAura
/// (all pk FKs with cascade).
#[derive(Database)]
struct ChainDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk on_delete cascade))]
    pub creature_stats: CreatureStatsTable,

    #[table(singular = "creature_aura", fks(id = "creature_stats" pk on_delete cascade))]
    pub creature_auras: CreatureAuraTable,
}

#[test]
fn cascade_chain_three_levels() {
    let mut db = ChainDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_aura(CreatureAura {
        id: CreatureId(1),
        color: "blue".to_string(),
    })
    .unwrap();

    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_stats.len(), 0);
    assert_eq!(db.creature_auras.len(), 0);
}

#[test]
fn cascade_chain_middle_level_only() {
    let mut db = ChainDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_aura(CreatureAura {
        id: CreatureId(1),
        color: "blue".to_string(),
    })
    .unwrap();

    // Remove just stats — should cascade to aura but leave creature.
    db.remove_creature_stats(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creature_stats.len(), 0);
    assert_eq!(db.creature_auras.len(), 0);
}

#[test]
fn cascade_chain_leaf_only() {
    let mut db = ChainDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_aura(CreatureAura {
        id: CreatureId(1),
        color: "blue".to_string(),
    })
    .unwrap();

    // Remove just aura — should leave stats and creature.
    db.remove_creature_aura(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creature_stats.len(), 1);
    assert_eq!(db.creature_auras.len(), 0);
}

#[test]
fn cascade_chain_partial_population() {
    let mut db = ChainDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    // No aura inserted — cascade should still work.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_stats.len(), 0);
}

#[test]
fn cascade_chain_insert_grandchild_without_middle_fails() {
    let mut db = ChainDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    // No stats — aura can't reference stats.
    let err = db
        .insert_creature_aura(CreatureAura {
            id: CreatureId(1),
            color: "blue".to_string(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

// =========================================================================
// Multiple pk FKs from different child tables to the same parent
// =========================================================================

#[derive(Table, Clone, Debug, PartialEq)]
struct CreatureAppearance {
    #[primary_key]
    pub id: CreatureId,
    pub hair: String,
}

#[derive(Database)]
struct MultiChildDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk))]
    pub creature_stats: CreatureStatsTable,

    #[table(singular = "creature_position", fks(id = "creatures" pk on_delete cascade))]
    pub creature_positions: CreaturePositionTable,

    #[table(singular = "creature_appearance", fks(id = "creatures" pk on_delete cascade))]
    pub creature_appearances: CreatureAppearanceTable,
}

#[test]
fn multi_child_restrict_blocks_cascade() {
    let mut db = MultiChildDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();
    db.insert_creature_appearance(CreatureAppearance {
        id: CreatureId(1),
        hair: "silver".to_string(),
    })
    .unwrap();

    // Restrict on stats should block, even though position and appearance
    // are cascade.
    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert!(matches!(err, Error::FkViolation { .. }));

    // Everything should still be intact.
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.creature_stats.len(), 1);
    assert_eq!(db.creature_positions.len(), 1);
    assert_eq!(db.creature_appearances.len(), 1);
}

#[test]
fn multi_child_all_cascade() {
    let mut db = MultiChildDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();
    db.insert_creature_appearance(CreatureAppearance {
        id: CreatureId(1),
        hair: "silver".to_string(),
    })
    .unwrap();
    // No stats (restrict) — so cascade should clean up.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_positions.len(), 0);
    assert_eq!(db.creature_appearances.len(), 0);
}

// =========================================================================
// modify_unchecked on parent-pk child tables
// =========================================================================

#[test]
fn modify_unchecked_child() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.modify_unchecked_creature_stats(&CreatureId(1), |row| {
        row.health = 200;
    })
    .unwrap();
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().health, 200);
}

#[test]
fn modify_unchecked_all_children() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_stats(make_stats(2, 80, 30)).unwrap();
    let count = db.modify_unchecked_all_creature_stats(|_pk, row| {
        row.mana += 10;
    });
    assert_eq!(count, 2);
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().mana, 60);
    assert_eq!(db.creature_stats.get(&CreatureId(2)).unwrap().mana, 40);
}

// =========================================================================
// Error message content
// =========================================================================

#[test]
fn restrict_error_mentions_child_table() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    match err {
        Error::FkViolation {
            table,
            referenced_by,
            ..
        } => {
            assert_eq!(table, "creatures");
            assert_eq!(referenced_by.len(), 1);
            assert_eq!(referenced_by[0].0, "creature_stats");
            assert_eq!(referenced_by[0].1, "id");
            assert_eq!(referenced_by[0].2, 1); // count is always 1 for pk FK
        }
        other => panic!("expected FkViolation, got: {other:?}"),
    }
}

#[test]
fn fk_target_not_found_error_fields() {
    let mut db = TestDb::new();
    let err = db
        .insert_creature_stats(make_stats(42, 100, 50))
        .unwrap_err();
    match err {
        Error::FkTargetNotFound {
            table,
            field,
            referenced_table,
            ..
        } => {
            assert_eq!(table, "creature_statss");
            assert_eq!(field, "id");
            assert_eq!(referenced_table, "creatures");
        }
        other => panic!("expected FkTargetNotFound, got: {other:?}"),
    }
}

// =========================================================================
// Auto-increment parent with pk FK child
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct AutoId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct AutoParent {
    #[primary_key(auto_increment)]
    pub id: AutoId,
    pub name: String,
}

#[derive(Table, Clone, Debug, PartialEq)]
struct AutoChild {
    #[primary_key]
    pub id: AutoId,
    pub value: u32,
}

#[derive(Database)]
struct AutoDb {
    #[table(singular = "auto_parent", auto)]
    pub auto_parents: AutoParentTable,

    #[table(singular = "auto_child", fks(id = "auto_parents" pk on_delete cascade))]
    pub auto_children: AutoChildTable,
}

#[test]
fn auto_increment_parent_with_pk_child() {
    let mut db = AutoDb::new();
    let pk = db
        .insert_auto_parent_auto(|id| AutoParent {
            id,
            name: "first".to_string(),
        })
        .unwrap();
    assert_eq!(pk, AutoId(0));

    db.insert_auto_child(AutoChild {
        id: AutoId(0),
        value: 42,
    })
    .unwrap();
    assert_eq!(db.auto_children.len(), 1);

    // Delete parent — should cascade.
    db.remove_auto_parent(&AutoId(0)).unwrap();
    assert_eq!(db.auto_parents.len(), 0);
    assert_eq!(db.auto_children.len(), 0);
}

#[test]
fn auto_increment_child_fk_to_nonexistent_parent() {
    let mut db = AutoDb::new();
    let err = db
        .insert_auto_child(AutoChild {
            id: AutoId(99),
            value: 1,
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

// =========================================================================
// Re-insert after cascade delete
// =========================================================================

#[test]
fn reinsert_parent_and_child_after_cascade() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();

    // Re-insert same PK.
    db.insert_creature(make_creature(1, "Reborn Elf")).unwrap();
    db.insert_creature_position(make_position(1, 30, 40))
        .unwrap();
    assert_eq!(db.creatures.get(&CreatureId(1)).unwrap().name, "Reborn Elf");
    assert_eq!(db.creature_positions.get(&CreatureId(1)).unwrap().x, 30);
}

// =========================================================================
// modify_unchecked_range on child table
// =========================================================================

#[test]
fn modify_unchecked_range_children() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature(make_creature(3, "Troll")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_stats(make_stats(2, 80, 30)).unwrap();
    db.insert_creature_stats(make_stats(3, 60, 10)).unwrap();

    let count =
        db.modify_unchecked_range_creature_stats(CreatureId(1)..=CreatureId(2), |_pk, row| {
            row.health += 10;
        });
    assert_eq!(count, 2);
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().health, 110);
    assert_eq!(db.creature_stats.get(&CreatureId(2)).unwrap().health, 90);
    assert_eq!(db.creature_stats.get(&CreatureId(3)).unwrap().health, 60); // unchanged
}

// =========================================================================
// Cascade chain with mixed pk and regular FKs
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct LogId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct CreatureLog {
    #[primary_key]
    pub id: LogId,
    #[indexed]
    pub creature_id: CreatureId,
    pub message: String,
}

/// Parent (creature) has:
/// - pk FK child (CreatureStats) with cascade
/// - regular FK child (CreatureLog) with cascade
///
/// If parent is deleted, both should cascade.
#[derive(Database)]
struct MixedCascadeDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk on_delete cascade))]
    pub creature_stats: CreatureStatsTable,

    #[table(singular = "creature_log", fks(creature_id = "creatures" on_delete cascade))]
    pub creature_logs: CreatureLogTable,
}

#[test]
fn mixed_cascade_pk_and_regular() {
    let mut db = MixedCascadeDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_log(CreatureLog {
        id: LogId(1),
        creature_id: CreatureId(1),
        message: "born".to_string(),
    })
    .unwrap();
    db.insert_creature_log(CreatureLog {
        id: LogId(2),
        creature_id: CreatureId(1),
        message: "grew up".to_string(),
    })
    .unwrap();

    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_stats.len(), 0);
    assert_eq!(db.creature_logs.len(), 0);
}

#[test]
fn mixed_cascade_only_pk_child_exists() {
    let mut db = MixedCascadeDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    // No logs.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_stats.len(), 0);
}

#[test]
fn mixed_cascade_only_regular_child_exists() {
    let mut db = MixedCascadeDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    // No stats.
    db.insert_creature_log(CreatureLog {
        id: LogId(1),
        creature_id: CreatureId(1),
        message: "born".to_string(),
    })
    .unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_logs.len(), 0);
}

// =========================================================================
// Child table access methods (get, get_ref, contains, len, keys, all)
// =========================================================================

#[test]
fn child_table_read_methods() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_stats(make_stats(2, 80, 30)).unwrap();

    // get
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().health, 100);

    // get_ref
    assert_eq!(db.creature_stats.get_ref(&CreatureId(1)).unwrap().mana, 50);

    // contains
    assert!(db.creature_stats.contains(&CreatureId(1)));
    assert!(!db.creature_stats.contains(&CreatureId(99)));

    // len
    assert_eq!(db.creature_stats.len(), 2);

    // keys
    let keys = db.creature_stats.keys();
    assert_eq!(keys, vec![CreatureId(1), CreatureId(2)]);

    // all (iter_all)
    let all: Vec<_> = db.creature_stats.iter_all().collect();
    assert_eq!(all.len(), 2);
}

// =========================================================================
// Edge case: insert child, remove child, re-insert child
// =========================================================================

#[test]
fn remove_child_then_reinsert() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.remove_creature_stats(&CreatureId(1)).unwrap();
    // Re-insert with different values.
    db.insert_creature_stats(make_stats(1, 200, 100)).unwrap();
    assert_eq!(db.creature_stats.get(&CreatureId(1)).unwrap().health, 200);
}

// =========================================================================
// Edge case: parent with no children, various operations
// =========================================================================

#[test]
fn operations_on_empty_child_tables() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();

    // No stats or positions inserted.
    assert_eq!(db.creature_stats.len(), 0);
    assert!(!db.creature_stats.contains(&CreatureId(1)));

    // Remove parent should succeed (no children to block).
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
}

// =========================================================================
// Leaf restrict blocks entire cascade chain
// =========================================================================

/// A -> B (pk cascade) -> C (pk restrict to B).
/// Deleting A cascades to B, but B's removal is blocked by C's restrict.
/// Error should propagate back to A's remove call.
#[derive(Database)]
struct LeafRestrictDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk on_delete cascade))]
    pub creature_stats: CreatureStatsTable,

    #[table(singular = "creature_position", fks(id = "creature_stats" pk))]
    pub creature_positions: CreaturePositionTable,
}

#[test]
fn leaf_restrict_blocks_cascade_chain() {
    let mut db = LeafRestrictDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();

    // Deleting creature should cascade to stats, but stats removal is
    // blocked by position's restrict.
    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert!(
        matches!(err, Error::FkViolation { .. }),
        "expected FkViolation, got: {err:?}"
    );
}

// =========================================================================
// Two FKs (pk + regular) to the same parent table
// =========================================================================

/// A creature with a PK that is FK to creatures AND a separate mentor field
/// that is also FK to creatures.
#[derive(Table, Clone, Debug, PartialEq)]
struct CreatureMentorship {
    #[primary_key]
    pub id: CreatureId,
    #[indexed]
    pub mentor: CreatureId,
}

#[derive(Database)]
struct DualFkToSameParentDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(
        singular = "creature_mentorship",
        fks(id = "creatures" pk on_delete cascade, mentor = "creatures")
    )]
    pub creature_mentorships: CreatureMentorshipTable,
}

#[test]
fn dual_fk_same_parent_insert_ok() {
    let mut db = DualFkToSameParentDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Mentor")).unwrap();
    db.insert_creature_mentorship(CreatureMentorship {
        id: CreatureId(1),
        mentor: CreatureId(2),
    })
    .unwrap();
    assert_eq!(db.creature_mentorships.len(), 1);
}

#[test]
fn dual_fk_same_parent_insert_pk_missing_fails() {
    let mut db = DualFkToSameParentDb::new();
    db.insert_creature(make_creature(2, "Mentor")).unwrap();
    let err = db
        .insert_creature_mentorship(CreatureMentorship {
            id: CreatureId(1),
            mentor: CreatureId(2),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn dual_fk_same_parent_insert_mentor_missing_fails() {
    let mut db = DualFkToSameParentDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    let err = db
        .insert_creature_mentorship(CreatureMentorship {
            id: CreatureId(1),
            mentor: CreatureId(99),
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn dual_fk_same_parent_cascade_on_pk_parent() {
    let mut db = DualFkToSameParentDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Mentor")).unwrap();
    db.insert_creature_mentorship(CreatureMentorship {
        id: CreatureId(1),
        mentor: CreatureId(2),
    })
    .unwrap();

    // Delete creature 1 (the pk parent) — should cascade.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creature_mentorships.len(), 0);
    // Creature 2 (mentor) should still exist.
    assert!(db.creatures.contains(&CreatureId(2)));
}

#[test]
fn dual_fk_same_parent_restrict_on_mentor() {
    let mut db = DualFkToSameParentDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Mentor")).unwrap();
    db.insert_creature_mentorship(CreatureMentorship {
        id: CreatureId(1),
        mentor: CreatureId(2),
    })
    .unwrap();

    // Delete creature 2 (the mentor) — should be blocked by restrict on mentor FK.
    let err = db.remove_creature(&CreatureId(2)).unwrap_err();
    assert!(
        matches!(err, Error::FkViolation { .. }),
        "expected FkViolation, got: {err:?}"
    );
}

// =========================================================================
// Cascade chain: pk FK → regular FK
// =========================================================================

/// A -> B (pk FK cascade) -> C (regular FK cascade from B).
/// Deleting A should cascade to B, which cascades to C.
#[derive(Database)]
struct PkThenRegularCascadeDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_stats", fks(id = "creatures" pk on_delete cascade))]
    pub creature_stats: CreatureStatsTable,

    #[table(singular = "creature_log", fks(creature_id = "creature_stats" on_delete cascade))]
    pub creature_logs: CreatureLogTable,
}

#[test]
fn cascade_pk_then_regular_fk() {
    let mut db = PkThenRegularCascadeDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    db.insert_creature_log(CreatureLog {
        id: LogId(1),
        creature_id: CreatureId(1),
        message: "born".to_string(),
    })
    .unwrap();
    db.insert_creature_log(CreatureLog {
        id: LogId(2),
        creature_id: CreatureId(1),
        message: "grew".to_string(),
    })
    .unwrap();

    // Delete creature → cascades to stats → cascades to logs.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_stats.len(), 0);
    assert_eq!(db.creature_logs.len(), 0);
}

// =========================================================================
// Index maintenance through cascade
// =========================================================================

/// pk-FK child with an indexed non-PK field. After cascade, the index
/// should no longer contain entries for the deleted row.
#[derive(Table, Clone, Debug, PartialEq)]
struct IndexedChild {
    #[primary_key]
    pub id: CreatureId,
    #[indexed]
    pub category: u32,
}

#[derive(Database)]
struct IndexedChildDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "indexed_child", fks(id = "creatures" pk on_delete cascade))]
    pub indexed_children: IndexedChildTable,
}

#[test]
fn cascade_updates_secondary_index() {
    use tabulosity::QueryOpts;

    let mut db = IndexedChildDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_indexed_child(IndexedChild {
        id: CreatureId(1),
        category: 5,
    })
    .unwrap();
    db.insert_indexed_child(IndexedChild {
        id: CreatureId(2),
        category: 5,
    })
    .unwrap();

    // Both have category 5.
    assert_eq!(db.indexed_children.count_by_category(&5, QueryOpts::ASC), 2);

    // Cascade-delete creature 1.
    db.remove_creature(&CreatureId(1)).unwrap();

    // Category index should reflect the deletion.
    assert_eq!(db.indexed_children.count_by_category(&5, QueryOpts::ASC), 1);
    let remaining: Vec<_> = db
        .indexed_children
        .iter_by_category(&5, QueryOpts::ASC)
        .collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, CreatureId(2));
}

#[test]
fn cascade_then_reinsert_with_different_index_value() {
    use tabulosity::QueryOpts;

    let mut db = IndexedChildDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_indexed_child(IndexedChild {
        id: CreatureId(1),
        category: 5,
    })
    .unwrap();

    // Cascade-delete.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.indexed_children.count_by_category(&5, QueryOpts::ASC), 0);

    // Re-insert parent and child with different category.
    db.insert_creature(make_creature(1, "Elf Reborn")).unwrap();
    db.insert_indexed_child(IndexedChild {
        id: CreatureId(1),
        category: 10,
    })
    .unwrap();

    assert_eq!(db.indexed_children.count_by_category(&5, QueryOpts::ASC), 0);
    assert_eq!(
        db.indexed_children.count_by_category(&10, QueryOpts::ASC),
        1
    );
}

// =========================================================================
// Both-restrict error lists multiple pk-FK children
// =========================================================================

#[test]
fn restrict_error_lists_both_pk_children() {
    let mut db = MultiChildDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature_stats(make_stats(1, 100, 50)).unwrap();
    // Don't insert position/appearance (cascade) — only stats (restrict).

    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    match err {
        Error::FkViolation { referenced_by, .. } => {
            // Only stats has restrict; position and appearance are cascade.
            assert_eq!(referenced_by.len(), 1);
            assert_eq!(referenced_by[0].0, "creature_stats");
            assert_eq!(referenced_by[0].2, 1); // pk FK always reports count=1
        }
        other => panic!("expected FkViolation, got: {other:?}"),
    }
}

// =========================================================================
// Consistency checks after cascade
// =========================================================================

#[test]
fn consistency_after_cascade() {
    let mut db = TestDb::new();
    db.insert_creature(make_creature(1, "Elf")).unwrap();
    db.insert_creature(make_creature(2, "Orc")).unwrap();
    db.insert_creature_position(make_position(1, 10, 20))
        .unwrap();
    db.insert_creature_position(make_position(2, 30, 40))
        .unwrap();

    db.remove_creature(&CreatureId(1)).unwrap();

    // len
    assert_eq!(db.creature_positions.len(), 1);
    // contains
    assert!(!db.creature_positions.contains(&CreatureId(1)));
    assert!(db.creature_positions.contains(&CreatureId(2)));
    // get
    assert!(db.creature_positions.get(&CreatureId(1)).is_none());
    assert_eq!(db.creature_positions.get(&CreatureId(2)).unwrap().x, 30);
    // iter_all
    let all: Vec<_> = db.creature_positions.iter_all().collect();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, CreatureId(2));
}

// =========================================================================
// Self-referential pk FK (creature's pk FK to itself — should it work?)
// This is a degenerate case but should compile and work if the child
// table IS the parent table. Actually, this is a cycle — we'd need cascade
// cycle detection to handle it.
// =========================================================================

// NOTE: Not tested because self-referential pk FK cascade would form a cycle
// which is correctly rejected at compile time by cascade cycle detection.

// =========================================================================
// Cascade partial failure documentation
// =========================================================================
// The following scenario documents non-atomic cascade behavior:
// A -> B (regular FK cascade), B -> C (pk FK restrict)
// If multiple B rows reference A and some have C rows (restrict blocks)
// while others don't, the cascade may partially succeed.
// This is known behavior, not a bug — tabulosity does not support
// transactional rollback. Users should structure their cascade chains
// to avoid mixed restrict/cascade at the same level.
