//! Integration tests for hash-based indexes (`#[indexed(hash)]` and
//! `#[index(..., kind = "hash")]`).
//!
//! Covers: CRUD with hash indexes, unique hash indexes, compound hash indexes,
//! query dispatch (exact, MatchAll, partial match), OneOrMany promotion/demotion,
//! modify_each_by on hash indexes, rebuild, and interaction with BTree indexes.

use tabulosity::*;

// ---------------------------------------------------------------------------
// Basic hash-indexed table
// ---------------------------------------------------------------------------

#[derive(Table, Clone, Debug, PartialEq)]
struct Creature {
    #[primary_key(auto_increment)]
    id: u32,
    #[indexed(hash)]
    species: String,
    name: String,
    age: u32,
}

#[test]
fn insert_and_query_by_hash_index() {
    let mut table = CreatureTable::new();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "Aelara".into(),
            age: 100,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "dwarf".into(),
            name: "Thorin".into(),
            age: 200,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "Legolas".into(),
            age: 150,
        })
        .unwrap();

    // Exact match query on hash index.
    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 2);
    assert_eq!(elves[0].name, "Aelara");
    assert_eq!(elves[1].name, "Legolas");

    // MatchAll query.
    let all = table.by_species(MatchAll, QueryOpts::ASC);
    assert_eq!(all.len(), 3);

    // Count.
    let elf_count = table.count_by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elf_count, 2);

    // No match.
    let orcs = table.by_species(&"orc".to_string(), QueryOpts::ASC);
    assert!(orcs.is_empty());
}

#[test]
fn update_hash_index_maintenance() {
    let mut table = CreatureTable::new();
    let id = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "Aelara".into(),
            age: 100,
        })
        .unwrap();

    // Update species — should update hash index.
    table
        .update_no_fk(Creature {
            id,
            species: "human".into(),
            name: "Aelara".into(),
            age: 100,
        })
        .unwrap();

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert!(elves.is_empty());

    let humans = table.by_species(&"human".to_string(), QueryOpts::ASC);
    assert_eq!(humans.len(), 1);
    assert_eq!(humans[0].name, "Aelara");
}

#[test]
fn remove_hash_index_maintenance() {
    let mut table = CreatureTable::new();
    let id = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "Aelara".into(),
            age: 100,
        })
        .unwrap();

    table.remove_no_fk(&id).unwrap();

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert!(elves.is_empty());
}

#[test]
fn hash_index_one_or_many_promotion_demotion() {
    let mut table = CreatureTable::new();

    // Insert first elf — One(pk).
    let id1 = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();

    // Insert second elf — promotes to Many.
    let _id2 = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "B".into(),
            age: 2,
        })
        .unwrap();

    assert_eq!(
        table.count_by_species(&"elf".to_string(), QueryOpts::ASC),
        2
    );

    // Remove first — demotes Many back to One.
    table.remove_no_fk(&id1).unwrap();
    assert_eq!(
        table.count_by_species(&"elf".to_string(), QueryOpts::ASC),
        1
    );

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves[0].name, "B");
}

// ---------------------------------------------------------------------------
// Unique hash index
// ---------------------------------------------------------------------------

#[derive(Table, Clone, Debug, PartialEq)]
struct User {
    #[primary_key(auto_increment)]
    id: u32,
    #[indexed(hash, unique)]
    email: String,
    name: String,
}

#[test]
fn unique_hash_index_insert_and_query() {
    let mut table = UserTable::new();
    table
        .insert_auto_no_fk(|id| User {
            id,
            email: "alice@example.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let result = table.by_email(&"alice@example.com".to_string(), QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "Alice");
}

#[test]
fn unique_hash_index_duplicate_rejected() {
    let mut table = UserTable::new();
    table
        .insert_auto_no_fk(|id| User {
            id,
            email: "alice@example.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    let result = table.insert_auto_no_fk(|id| User {
        id,
        email: "alice@example.com".into(),
        name: "Bob".into(),
    });

    assert!(matches!(result, Err(Error::DuplicateIndex { .. })));
}

#[test]
fn unique_hash_index_update_same_value_ok() {
    let mut table = UserTable::new();
    let id = table
        .insert_auto_no_fk(|id| User {
            id,
            email: "alice@example.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    // Update name but keep email — should work.
    table
        .update_no_fk(User {
            id,
            email: "alice@example.com".into(),
            name: "Alice Updated".into(),
        })
        .unwrap();

    let result = table.by_email(&"alice@example.com".to_string(), QueryOpts::ASC);
    assert_eq!(result[0].name, "Alice Updated");
}

#[test]
fn unique_hash_index_update_to_existing_rejected() {
    let mut table = UserTable::new();
    let _id1 = table
        .insert_auto_no_fk(|id| User {
            id,
            email: "alice@example.com".into(),
            name: "Alice".into(),
        })
        .unwrap();
    let id2 = table
        .insert_auto_no_fk(|id| User {
            id,
            email: "bob@example.com".into(),
            name: "Bob".into(),
        })
        .unwrap();

    let result = table.update_no_fk(User {
        id: id2,
        email: "alice@example.com".into(),
        name: "Bob".into(),
    });

    assert!(matches!(result, Err(Error::DuplicateIndex { .. })));
}

// ---------------------------------------------------------------------------
// Compound hash index
// ---------------------------------------------------------------------------

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "by_species_age", fields("species", "age"), kind = "hash")]
struct Animal {
    #[primary_key(auto_increment)]
    id: u32,
    species: String,
    age: u32,
    name: String,
}

#[test]
fn compound_hash_index_exact_match() {
    let mut table = AnimalTable::new();
    table
        .insert_auto_no_fk(|id| Animal {
            id,
            species: "cat".into(),
            age: 3,
            name: "Whiskers".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Animal {
            id,
            species: "cat".into(),
            age: 5,
            name: "Mittens".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Animal {
            id,
            species: "dog".into(),
            age: 3,
            name: "Rex".into(),
        })
        .unwrap();

    let result = table.by_by_species_age(&"cat".to_string(), &3u32, QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "Whiskers");
}

#[test]
fn compound_hash_index_partial_match_scans() {
    let mut table = AnimalTable::new();
    table
        .insert_auto_no_fk(|id| Animal {
            id,
            species: "cat".into(),
            age: 3,
            name: "Whiskers".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Animal {
            id,
            species: "cat".into(),
            age: 5,
            name: "Mittens".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Animal {
            id,
            species: "dog".into(),
            age: 3,
            name: "Rex".into(),
        })
        .unwrap();

    // Partial match: exact species, MatchAll age.
    let cats = table.by_by_species_age(&"cat".to_string(), MatchAll, QueryOpts::ASC);
    assert_eq!(cats.len(), 2);

    // Partial match: MatchAll species, exact age.
    let age3 = table.by_by_species_age(MatchAll, &3u32, QueryOpts::ASC);
    assert_eq!(age3.len(), 2);

    // All MatchAll.
    let all = table.by_by_species_age(MatchAll, MatchAll, QueryOpts::ASC);
    assert_eq!(all.len(), 3);
}

#[test]
#[should_panic(expected = "range queries are not supported on hash index")]
fn compound_hash_index_range_panics() {
    let table = AnimalTable::new();
    let _ = table.by_by_species_age(&"cat".to_string(), 1u32..5u32, QueryOpts::ASC);
}

// ---------------------------------------------------------------------------
// Hash index range query panics
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "range queries are not supported on hash index")]
fn simple_hash_index_range_panics() {
    let table = CreatureTable::new();
    let _ = table.by_species("a".to_string().."z".to_string(), QueryOpts::ASC);
}

// ---------------------------------------------------------------------------
// Offset on hash queries
// ---------------------------------------------------------------------------

#[test]
fn hash_index_offset() {
    let mut table = CreatureTable::new();
    for i in 0..5 {
        table
            .insert_auto_no_fk(|id| Creature {
                id,
                species: "elf".into(),
                name: format!("Elf{}", i),
                age: i * 10,
            })
            .unwrap();
    }

    let with_offset = table.by_species(&"elf".to_string(), QueryOpts::ASC.with_offset(2));
    assert_eq!(with_offset.len(), 3);
}

// ---------------------------------------------------------------------------
// manual_rebuild_all_indexes
// ---------------------------------------------------------------------------

#[test]
fn manual_rebuild_all_indexes_hash() {
    let mut table = CreatureTable::new();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "dwarf".into(),
            name: "B".into(),
            age: 2,
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 1);
    assert_eq!(elves[0].name, "A");

    let dwarves = table.by_species(&"dwarf".to_string(), QueryOpts::ASC);
    assert_eq!(dwarves.len(), 1);
}

// ---------------------------------------------------------------------------
// modify_each_by on hash index
// ---------------------------------------------------------------------------

#[test]
fn modify_each_by_hash_index() {
    let mut table = CreatureTable::new();
    for i in 0..3 {
        table
            .insert_auto_no_fk(|id| Creature {
                id,
                species: "elf".into(),
                name: format!("Elf{}", i),
                age: i * 10,
            })
            .unwrap();
    }

    // Modify age of all elves.
    let count = table.modify_each_by_species(&"elf".to_string(), QueryOpts::ASC, |_pk, row| {
        row.age += 100;
    });
    assert_eq!(count, 3);

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert!(elves.iter().all(|e| e.age >= 100));
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "indexed field")]
fn modify_each_by_hash_panics_on_indexed_field_change() {
    let mut table = CreatureTable::new();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();

    // Modifying the indexed field `species` should panic in debug builds.
    table.modify_each_by_species(&"elf".to_string(), QueryOpts::ASC, |_pk, row| {
        row.species = "orc".into();
    });
}

// ---------------------------------------------------------------------------
// Upsert with hash index
// ---------------------------------------------------------------------------

#[test]
fn upsert_with_hash_index() {
    let mut table = CreatureTable::new();
    let id = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();

    // Upsert update path — change species.
    table
        .upsert_no_fk(Creature {
            id,
            species: "human".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();

    assert!(
        table
            .by_species(&"elf".to_string(), QueryOpts::ASC)
            .is_empty()
    );
    assert_eq!(
        table.by_species(&"human".to_string(), QueryOpts::ASC).len(),
        1
    );
}

// ---------------------------------------------------------------------------
// iter_by on hash index
// ---------------------------------------------------------------------------

#[test]
fn iter_by_hash_index() {
    let mut table = CreatureTable::new();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "B".into(),
            age: 2,
        })
        .unwrap();

    let names: Vec<String> = table
        .iter_by_species(&"elf".to_string(), QueryOpts::ASC)
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names.len(), 2);
}

// ---------------------------------------------------------------------------
// Filtered hash index
// ---------------------------------------------------------------------------

fn is_adult(c: &FilterCreature) -> bool {
    c.age >= 18
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "adult_species",
    fields("species"),
    kind = "hash",
    filter = "is_adult"
)]
struct FilterCreature {
    #[primary_key(auto_increment)]
    id: u32,
    species: String,
    name: String,
    age: u32,
}

#[test]
fn filtered_hash_index_only_includes_matching_rows() {
    let mut table = FilterCreatureTable::new();
    table
        .insert_auto_no_fk(|id| FilterCreature {
            id,
            species: "elf".into(),
            name: "Child".into(),
            age: 10,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| FilterCreature {
            id,
            species: "elf".into(),
            name: "Adult".into(),
            age: 100,
        })
        .unwrap();

    let adults = table.by_adult_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(adults.len(), 1);
    assert_eq!(adults[0].name, "Adult");
}

#[test]
fn filtered_hash_index_update_filter_transition() {
    let mut table = FilterCreatureTable::new();
    let id = table
        .insert_auto_no_fk(|id| FilterCreature {
            id,
            species: "elf".into(),
            name: "Young".into(),
            age: 10,
        })
        .unwrap();

    // Not in index yet (age < 18).
    assert!(
        table
            .by_adult_species(&"elf".to_string(), QueryOpts::ASC)
            .is_empty()
    );

    // Update to adult — should enter filtered index.
    table
        .update_no_fk(FilterCreature {
            id,
            species: "elf".into(),
            name: "Young".into(),
            age: 25,
        })
        .unwrap();

    let adults = table.by_adult_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(adults.len(), 1);

    // Update back to child — should leave filtered index.
    table
        .update_no_fk(FilterCreature {
            id,
            species: "elf".into(),
            name: "Young".into(),
            age: 5,
        })
        .unwrap();

    assert!(
        table
            .by_adult_species(&"elf".to_string(), QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn filtered_hash_index_remove() {
    let mut table = FilterCreatureTable::new();
    let id = table
        .insert_auto_no_fk(|id| FilterCreature {
            id,
            species: "elf".into(),
            name: "Elder".into(),
            age: 200,
        })
        .unwrap();

    assert_eq!(
        table
            .by_adult_species(&"elf".to_string(), QueryOpts::ASC)
            .len(),
        1
    );

    table.remove_no_fk(&id).unwrap();

    assert!(
        table
            .by_adult_species(&"elf".to_string(), QueryOpts::ASC)
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// Filtered unique hash index
// ---------------------------------------------------------------------------

fn is_active(u: &FilterUser) -> bool {
    u.active
}

#[derive(Table, Clone, Debug, PartialEq)]
#[index(
    name = "active_email",
    fields("email"),
    kind = "hash",
    filter = "is_active",
    unique
)]
struct FilterUser {
    #[primary_key(auto_increment)]
    id: u32,
    email: String,
    active: bool,
}

#[test]
fn filtered_unique_hash_index_allows_duplicate_when_filtered_out() {
    let mut table = FilterUserTable::new();
    table
        .insert_auto_no_fk(|id| FilterUser {
            id,
            email: "alice@example.com".into(),
            active: false, // Not in index.
        })
        .unwrap();

    // Same email, active — should succeed since the first is filtered out.
    table
        .insert_auto_no_fk(|id| FilterUser {
            id,
            email: "alice@example.com".into(),
            active: true,
        })
        .unwrap();

    let result = table.by_active_email(&"alice@example.com".to_string(), QueryOpts::ASC);
    assert_eq!(result.len(), 1);
}

#[test]
fn filtered_unique_hash_index_rejects_duplicate_when_both_active() {
    let mut table = FilterUserTable::new();
    table
        .insert_auto_no_fk(|id| FilterUser {
            id,
            email: "alice@example.com".into(),
            active: true,
        })
        .unwrap();

    let result = table.insert_auto_no_fk(|id| FilterUser {
        id,
        email: "alice@example.com".into(),
        active: true,
    });

    assert!(matches!(result, Err(Error::DuplicateIndex { .. })));
}

// ---------------------------------------------------------------------------
// Mixed BTree + hash indexes on same table
// ---------------------------------------------------------------------------

#[derive(Table, Clone, Debug, PartialEq)]
#[index(name = "species_hash", fields("species"), kind = "hash")]
struct MixedCreature {
    #[primary_key(auto_increment)]
    id: u32,
    #[indexed]
    species: String,
    name: String,
}

#[test]
fn mixed_btree_and_hash_indexes_both_work() {
    let mut table = MixedCreatureTable::new();
    table
        .insert_auto_no_fk(|id| MixedCreature {
            id,
            species: "elf".into(),
            name: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| MixedCreature {
            id,
            species: "dwarf".into(),
            name: "B".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| MixedCreature {
            id,
            species: "elf".into(),
            name: "C".into(),
        })
        .unwrap();

    // BTree index supports range queries.
    let range_result = table.by_species("d".to_string().."f".to_string(), QueryOpts::ASC);
    assert_eq!(range_result.len(), 3); // "dwarf" (1 row) + "elf" (2 rows)
    // Hash index gives exact match.
    let hash_result = table.by_species_hash(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(hash_result.len(), 2);

    // Both maintained after update.
    table
        .update_no_fk(MixedCreature {
            id: 0,
            species: "human".into(),
            name: "A".into(),
        })
        .unwrap();

    let elves_btree = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves_btree.len(), 1);
    let elves_hash = table.by_species_hash(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves_hash.len(), 1);
}

// ---------------------------------------------------------------------------
// Compound hash index update
// ---------------------------------------------------------------------------

#[test]
fn compound_hash_index_update_maintenance() {
    let mut table = AnimalTable::new();
    let id = table
        .insert_auto_no_fk(|id| Animal {
            id,
            species: "cat".into(),
            age: 3,
            name: "Whiskers".into(),
        })
        .unwrap();

    // Update species — should move to different compound key.
    table
        .update_no_fk(Animal {
            id,
            species: "dog".into(),
            age: 3,
            name: "Whiskers".into(),
        })
        .unwrap();

    let cats = table.by_by_species_age(&"cat".to_string(), &3u32, QueryOpts::ASC);
    assert!(cats.is_empty());

    let dogs = table.by_by_species_age(&"dog".to_string(), &3u32, QueryOpts::ASC);
    assert_eq!(dogs.len(), 1);
    assert_eq!(dogs[0].name, "Whiskers");
}

// ---------------------------------------------------------------------------
// Hash index on compound PK table
// ---------------------------------------------------------------------------

#[derive(Table, Clone, Debug, PartialEq)]
#[primary_key("region", "slot")]
struct CompoundPkItem {
    region: u32,
    slot: u32,
    #[indexed(hash)]
    category: String,
    name: String,
}

#[test]
fn hash_index_on_compound_pk_table() {
    let mut table = CompoundPkItemTable::new();
    table
        .insert_no_fk(CompoundPkItem {
            region: 1,
            slot: 0,
            category: "weapon".into(),
            name: "Sword".into(),
        })
        .unwrap();
    table
        .insert_no_fk(CompoundPkItem {
            region: 1,
            slot: 1,
            category: "armor".into(),
            name: "Shield".into(),
        })
        .unwrap();
    table
        .insert_no_fk(CompoundPkItem {
            region: 2,
            slot: 0,
            category: "weapon".into(),
            name: "Bow".into(),
        })
        .unwrap();

    let weapons = table.by_category(&"weapon".to_string(), QueryOpts::ASC);
    assert_eq!(weapons.len(), 2);

    // Remove and verify.
    table.remove_no_fk(&(1, 0)).unwrap();
    let weapons = table.by_category(&"weapon".to_string(), QueryOpts::ASC);
    assert_eq!(weapons.len(), 1);
    assert_eq!(weapons[0].name, "Bow");
}

// ---------------------------------------------------------------------------
// Unique hash index: remove then reinsert
// ---------------------------------------------------------------------------

#[test]
fn unique_hash_index_remove_then_reinsert() {
    let mut table = UserTable::new();
    let id = table
        .insert_auto_no_fk(|id| User {
            id,
            email: "alice@example.com".into(),
            name: "Alice".into(),
        })
        .unwrap();

    table.remove_no_fk(&id).unwrap();

    // Should succeed — email is no longer in the index.
    table
        .insert_auto_no_fk(|id| User {
            id,
            email: "alice@example.com".into(),
            name: "New Alice".into(),
        })
        .unwrap();

    let result = table.by_email(&"alice@example.com".to_string(), QueryOpts::ASC);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "New Alice");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_table_hash_query() {
    let table = CreatureTable::new();
    let exact = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert!(exact.is_empty());
    let all = table.by_species(MatchAll, QueryOpts::ASC);
    assert!(all.is_empty());
    assert_eq!(
        table.count_by_species(&"elf".to_string(), QueryOpts::ASC),
        0
    );
}

#[test]
fn rebuild_empty_table_with_hash_index() {
    let mut table = CreatureTable::new();
    table.manual_rebuild_all_indexes();
    assert!(
        table
            .by_species(&"elf".to_string(), QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn rebuild_after_removals() {
    let mut table = CreatureTable::new();
    let id1 = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "B".into(),
            age: 2,
        })
        .unwrap();
    let id3 = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "dwarf".into(),
            name: "C".into(),
            age: 3,
        })
        .unwrap();

    table.remove_no_fk(&id1).unwrap();
    table.remove_no_fk(&id3).unwrap();

    table.manual_rebuild_all_indexes();

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 1);
    assert_eq!(elves[0].name, "B");

    assert!(
        table
            .by_species(&"dwarf".to_string(), QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn update_same_species_noop_nonunique() {
    let mut table = CreatureTable::new();
    let id = table
        .insert_auto_no_fk(|id| Creature {
            id,
            species: "elf".into(),
            name: "A".into(),
            age: 1,
        })
        .unwrap();

    // Update name but keep species — index should still work.
    table
        .update_no_fk(Creature {
            id,
            species: "elf".into(),
            name: "Updated".into(),
            age: 99,
        })
        .unwrap();

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 1);
    assert_eq!(elves[0].name, "Updated");
}

// ---------------------------------------------------------------------------
// Hash primary storage
// ---------------------------------------------------------------------------

#[derive(Table, Clone, Debug, PartialEq)]
#[table(primary_storage = "hash")]
struct HashPrimary {
    #[primary_key(auto_increment)]
    id: u32,
    #[indexed(hash)]
    species: String,
    name: String,
}

#[test]
fn hash_primary_basic_crud() {
    let mut table = HashPrimaryTable::new();
    let id1 = table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "elf".into(),
            name: "A".into(),
        })
        .unwrap();
    let id2 = table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "dwarf".into(),
            name: "B".into(),
        })
        .unwrap();

    assert_eq!(table.len(), 2);
    assert_eq!(table.get(&id1).unwrap().name, "A");
    assert!(table.contains(&id2));

    // Hash index works.
    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 1);

    // Update.
    table
        .update_no_fk(HashPrimary {
            id: id1,
            species: "human".into(),
            name: "A".into(),
        })
        .unwrap();
    assert!(
        table
            .by_species(&"elf".to_string(), QueryOpts::ASC)
            .is_empty()
    );
    assert_eq!(
        table.by_species(&"human".to_string(), QueryOpts::ASC).len(),
        1
    );

    // Remove.
    table.remove_no_fk(&id1).unwrap();
    assert_eq!(table.len(), 1);
    assert!(!table.contains(&id1));
}

#[test]
fn hash_primary_iter_all_in_insertion_order() {
    let mut table = HashPrimaryTable::new();
    table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "c".into(),
            name: "C".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "a".into(),
            name: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "b".into(),
            name: "B".into(),
        })
        .unwrap();

    // InsOrdHashMap iterates in insertion order, not PK-sorted order.
    let names: Vec<String> = table.all().iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, vec!["C", "A", "B"]);
}

#[test]
fn hash_primary_modify_unchecked_all() {
    let mut table = HashPrimaryTable::new();
    for i in 0..3 {
        table
            .insert_auto_no_fk(|id| HashPrimary {
                id,
                species: "elf".into(),
                name: format!("Elf{}", i),
            })
            .unwrap();
    }

    let count = table.modify_unchecked_all(|_pk, row| {
        row.name = format!("Modified-{}", row.name);
    });
    assert_eq!(count, 3);
    for row in table.all() {
        assert!(row.name.starts_with("Modified-"));
    }
}

#[test]
#[should_panic(expected = "modify_unchecked_range is not supported")]
fn hash_primary_modify_unchecked_range_panics() {
    let mut table = HashPrimaryTable::new();
    table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "elf".into(),
            name: "A".into(),
        })
        .unwrap();

    table.modify_unchecked_range(.., |_pk, _row| {});
}

#[test]
fn hash_primary_rebuild() {
    let mut table = HashPrimaryTable::new();
    table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "elf".into(),
            name: "A".into(),
        })
        .unwrap();
    table
        .insert_auto_no_fk(|id| HashPrimary {
            id,
            species: "elf".into(),
            name: "B".into(),
        })
        .unwrap();

    table.manual_rebuild_all_indexes();

    let elves = table.by_species(&"elf".to_string(), QueryOpts::ASC);
    assert_eq!(elves.len(), 2);
}
