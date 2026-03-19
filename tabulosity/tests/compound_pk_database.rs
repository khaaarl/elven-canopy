//! Database-level tests for compound PKs with foreign keys.
//!
//! Tests that zero, one, or multiple columns of a compound PK can be FKs,
//! with proper FK validation and cascade/restrict behavior.

use tabulosity::{Bounded, Database, Error, QueryOpts, Table};

// --- Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CreatureId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct TraitKind(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ItemId(u32);

// --- Parent tables ---

#[derive(Table, Clone, Debug, PartialEq)]
struct Creature {
    #[primary_key]
    pub id: CreatureId,
    pub name: String,
}

#[derive(Table, Clone, Debug, PartialEq)]
struct Item {
    #[primary_key]
    pub id: ItemId,
    pub name: String,
}

// --- Compound PK child with zero FKs ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("x", "y")]
struct Coord {
    pub x: i32,
    pub y: i32,
    pub label: String,
}

#[derive(Database)]
struct ZeroFkDb {
    #[table(singular = "coord")]
    pub coords: CoordTable,
}

#[test]
fn zero_fk_compound_pk_insert() {
    let mut db = ZeroFkDb::new();
    db.insert_coord(Coord {
        x: 1,
        y: 2,
        label: "origin".to_string(),
    })
    .unwrap();
    assert_eq!(db.coords.len(), 1);
    assert_eq!(db.coords.get(&(1, 2)).unwrap().label, "origin");
}

#[test]
fn zero_fk_compound_pk_duplicate_fails() {
    let mut db = ZeroFkDb::new();
    db.insert_coord(Coord {
        x: 1,
        y: 2,
        label: "first".to_string(),
    })
    .unwrap();
    let err = db
        .insert_coord(Coord {
            x: 1,
            y: 2,
            label: "second".to_string(),
        })
        .unwrap_err();
    assert!(matches!(err, Error::DuplicateKey { .. }));
}

// --- Compound PK child with one FK column ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind")]
struct CreatureTrait {
    #[indexed]
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
}

#[derive(Database)]
struct OneFkDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "creature_trait", fks(creature_id = "creatures" on_delete cascade))]
    pub creature_traits: CreatureTraitTable,
}

#[test]
fn one_fk_insert_with_parent() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();
    assert_eq!(db.creature_traits.len(), 1);
}

#[test]
fn one_fk_insert_without_parent_fails() {
    let mut db = OneFkDb::new();
    let err = db
        .insert_creature_trait(CreatureTrait {
            creature_id: CreatureId(99),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn one_fk_cascade_deletes_compound_pk_children() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(20),
        value: 43,
    })
    .unwrap();

    // Delete the parent — should cascade to both trait rows.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
    assert_eq!(db.creature_traits.len(), 0);
}

#[test]
fn one_fk_cascade_only_affects_matching_parent() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature(Creature {
        id: CreatureId(2),
        name: "Orc".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(2),
        trait_kind: TraitKind(10),
        value: 43,
    })
    .unwrap();

    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creature_traits.len(), 1);
    assert!(db.creature_traits.contains(&(CreatureId(2), TraitKind(10))));
}

// --- Compound PK child with restrict ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "trait_kind")]
struct RestrictTrait {
    #[indexed]
    pub creature_id: CreatureId,
    pub trait_kind: TraitKind,
    pub value: i32,
}

#[derive(Database)]
struct RestrictFkDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "restrict_trait", fks(creature_id = "creatures"))]
    pub restrict_traits: RestrictTraitTable,
}

#[test]
fn restrict_blocks_parent_delete() {
    let mut db = RestrictFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_restrict_trait(RestrictTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();

    let err = db.remove_creature(&CreatureId(1)).unwrap_err();
    assert!(matches!(err, Error::FkViolation { .. }));
    // Both should still exist.
    assert_eq!(db.creatures.len(), 1);
    assert_eq!(db.restrict_traits.len(), 1);
}

#[test]
fn restrict_allows_delete_after_child_removed() {
    let mut db = RestrictFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_restrict_trait(RestrictTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();

    db.remove_restrict_trait(&(CreatureId(1), TraitKind(10)))
        .unwrap();
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creatures.len(), 0);
}

// --- Compound PK child with two FK columns ---

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "item_id")]
struct CreatureItem {
    #[indexed]
    pub creature_id: CreatureId,
    #[indexed]
    pub item_id: ItemId,
    pub quantity: u32,
}

#[derive(Database)]
struct TwoFkDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "item")]
    pub items: ItemTable,

    #[table(
        singular = "creature_item",
        fks(creature_id = "creatures" on_delete cascade, item_id = "items")
    )]
    pub creature_items: CreatureItemTable,
}

#[test]
fn two_fk_insert_both_parents_exist() {
    let mut db = TwoFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_item(Item {
        id: ItemId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    db.insert_creature_item(CreatureItem {
        creature_id: CreatureId(1),
        item_id: ItemId(10),
        quantity: 1,
    })
    .unwrap();
    assert_eq!(db.creature_items.len(), 1);
}

#[test]
fn two_fk_insert_first_parent_missing_fails() {
    let mut db = TwoFkDb::new();
    db.insert_item(Item {
        id: ItemId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    let err = db
        .insert_creature_item(CreatureItem {
            creature_id: CreatureId(99),
            item_id: ItemId(10),
            quantity: 1,
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn two_fk_insert_second_parent_missing_fails() {
    let mut db = TwoFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    let err = db
        .insert_creature_item(CreatureItem {
            creature_id: CreatureId(1),
            item_id: ItemId(99),
            quantity: 1,
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

#[test]
fn two_fk_cascade_on_first_parent() {
    let mut db = TwoFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_item(Item {
        id: ItemId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    db.insert_creature_item(CreatureItem {
        creature_id: CreatureId(1),
        item_id: ItemId(10),
        quantity: 1,
    })
    .unwrap();

    // Cascade delete on creatures.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creature_items.len(), 0);
    // Item should still exist.
    assert!(db.items.contains(&ItemId(10)));
}

#[test]
fn two_fk_restrict_on_second_parent() {
    let mut db = TwoFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_item(Item {
        id: ItemId(10),
        name: "Sword".to_string(),
    })
    .unwrap();
    db.insert_creature_item(CreatureItem {
        creature_id: CreatureId(1),
        item_id: ItemId(10),
        quantity: 1,
    })
    .unwrap();

    // Restrict on items — should block deletion.
    let err = db.remove_item(&ItemId(10)).unwrap_err();
    assert!(matches!(err, Error::FkViolation { .. }));
}

// --- Update FK validation with compound PK ---

#[test]
fn update_fk_still_validated() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();

    // Update value — should succeed (FK column unchanged).
    db.update_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 99,
    })
    .unwrap();
    assert_eq!(
        db.creature_traits
            .get(&(CreatureId(1), TraitKind(10)))
            .unwrap()
            .value,
        99
    );
}

// --- modify_unchecked through database ---

#[test]
fn modify_unchecked_compound_pk_through_database() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();

    db.modify_unchecked_creature_trait(&(CreatureId(1), TraitKind(10)), |row| {
        row.value = 99;
    })
    .unwrap();
    assert_eq!(
        db.creature_traits
            .get(&(CreatureId(1), TraitKind(10)))
            .unwrap()
            .value,
        99
    );
}

#[test]
fn modify_unchecked_all_compound_pk_through_database() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(20),
        value: 43,
    })
    .unwrap();

    let count = db.modify_unchecked_all_creature_trait(|_pk, row| {
        row.value += 100;
    });
    assert_eq!(count, 2);
    assert_eq!(
        db.creature_traits
            .get(&(CreatureId(1), TraitKind(10)))
            .unwrap()
            .value,
        142
    );
}

#[test]
fn modify_unchecked_range_compound_pk_through_database() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature(Creature {
        id: CreatureId(2),
        name: "Orc".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 1,
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(2),
        trait_kind: TraitKind(10),
        value: 2,
    })
    .unwrap();

    let count = db.modify_unchecked_range_creature_trait(
        (CreatureId(1), TraitKind(10))..=(CreatureId(1), TraitKind(10)),
        |_pk, row| {
            row.value += 100;
        },
    );
    assert_eq!(count, 1);
    assert_eq!(
        db.creature_traits
            .get(&(CreatureId(1), TraitKind(10)))
            .unwrap()
            .value,
        101
    );
    // Other row unchanged.
    assert_eq!(
        db.creature_traits
            .get(&(CreatureId(2), TraitKind(10)))
            .unwrap()
            .value,
        2
    );
}

// --- Querying indexed FK column on compound PK table ---

#[test]
fn query_by_fk_column() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature(Creature {
        id: CreatureId(2),
        name: "Orc".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(20),
        value: 43,
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(2),
        trait_kind: TraitKind(10),
        value: 44,
    })
    .unwrap();

    let c1_traits = db
        .creature_traits
        .by_creature_id(&CreatureId(1), QueryOpts::ASC);
    assert_eq!(c1_traits.len(), 2);
    assert_eq!(c1_traits[0].value, 42);
    assert_eq!(c1_traits[1].value, 43);
}

// --- Upsert through database with FK validation ---

#[test]
fn upsert_insert_path_with_fk_validation() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.upsert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();
    assert_eq!(db.creature_traits.len(), 1);
}

#[test]
fn upsert_update_path_with_fk_validation() {
    let mut db = OneFkDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 42,
    })
    .unwrap();
    db.upsert_creature_trait(CreatureTrait {
        creature_id: CreatureId(1),
        trait_kind: TraitKind(10),
        value: 99,
    })
    .unwrap();
    assert_eq!(db.creature_traits.len(), 1);
    assert_eq!(
        db.creature_traits
            .get(&(CreatureId(1), TraitKind(10)))
            .unwrap()
            .value,
        99
    );
}

#[test]
fn upsert_without_parent_fails() {
    let mut db = OneFkDb::new();
    let err = db
        .upsert_creature_trait(CreatureTrait {
            creature_id: CreatureId(99),
            trait_kind: TraitKind(10),
            value: 42,
        })
        .unwrap_err();
    assert!(matches!(err, Error::FkTargetNotFound { .. }));
}

// --- Nullify on delete with compound PK child ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct EffectKind(u32);

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("creature_id", "effect_kind")]
struct CreatureEffect {
    #[indexed]
    pub creature_id: CreatureId,
    pub effect_kind: EffectKind,
    #[indexed]
    pub source: Option<ItemId>,
}

#[derive(Database)]
struct NullifyDb {
    #[table(singular = "creature")]
    pub creatures: CreatureTable,

    #[table(singular = "item")]
    pub items: ItemTable,

    #[table(
        singular = "creature_effect",
        fks(creature_id = "creatures" on_delete cascade, source? = "items" on_delete nullify)
    )]
    pub creature_effects: CreatureEffectTable,
}

#[test]
fn nullify_on_delete_sets_fk_to_none() {
    let mut db = NullifyDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_item(Item {
        id: ItemId(10),
        name: "Potion".to_string(),
    })
    .unwrap();
    db.insert_creature_effect(CreatureEffect {
        creature_id: CreatureId(1),
        effect_kind: EffectKind(1),
        source: Some(ItemId(10)),
    })
    .unwrap();

    // Delete the item — should nullify the source FK.
    db.remove_item(&ItemId(10)).unwrap();
    let effect = db
        .creature_effects
        .get(&(CreatureId(1), EffectKind(1)))
        .unwrap();
    assert_eq!(effect.source, None);
}

#[test]
fn cascade_on_creature_with_nullified_effect() {
    let mut db = NullifyDb::new();
    db.insert_creature(Creature {
        id: CreatureId(1),
        name: "Elf".to_string(),
    })
    .unwrap();
    db.insert_creature_effect(CreatureEffect {
        creature_id: CreatureId(1),
        effect_kind: EffectKind(1),
        source: None,
    })
    .unwrap();

    // Delete creature — should cascade to effects.
    db.remove_creature(&CreatureId(1)).unwrap();
    assert_eq!(db.creature_effects.len(), 0);
}
