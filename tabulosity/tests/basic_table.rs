//! Integration tests for basic `#[derive(Table)]` — no secondary indexes.

use tabulosity::{Bounded, Error, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct ItemId(u32);

#[derive(Table, Clone, Debug, PartialEq)]
struct Item {
    #[primary_key]
    pub id: ItemId,
    pub name: String,
    pub weight: u32,
}

#[test]
fn new_table_is_empty() {
    let table = ItemTable::new();
    assert!(table.is_empty());
    assert_eq!(table.len(), 0);
}

#[test]
fn insert_and_get() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    assert_eq!(table.len(), 1);
    assert!(!table.is_empty());

    let item = table.get(&ItemId(1)).unwrap();
    assert_eq!(item.name, "Sword");
    assert_eq!(item.weight, 10);
}

#[test]
fn get_ref() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Bow".into(),
            weight: 5,
        })
        .unwrap();

    let item_ref = table.get_ref(&ItemId(1)).unwrap();
    assert_eq!(item_ref.name, "Bow");
}

#[test]
fn get_missing_returns_none() {
    let table = ItemTable::new();
    assert!(table.get(&ItemId(99)).is_none());
    assert!(table.get_ref(&ItemId(99)).is_none());
}

#[test]
fn contains() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Shield".into(),
            weight: 15,
        })
        .unwrap();

    assert!(table.contains(&ItemId(1)));
    assert!(!table.contains(&ItemId(2)));
}

#[test]
fn insert_duplicate_key_error() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let err = table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Duplicate".into(),
            weight: 0,
        })
        .unwrap_err();

    assert_eq!(
        err,
        Error::DuplicateKey {
            table: "items",
            key: "ItemId(1)".into(),
        }
    );

    // Original row unchanged.
    assert_eq!(table.get(&ItemId(1)).unwrap().name, "Sword");
}

#[test]
fn update_existing_row() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    table
        .update_no_fk(Item {
            id: ItemId(1),
            name: "Enchanted Sword".into(),
            weight: 12,
        })
        .unwrap();

    let item = table.get(&ItemId(1)).unwrap();
    assert_eq!(item.name, "Enchanted Sword");
    assert_eq!(item.weight, 12);
}

#[test]
fn update_not_found_error() {
    let mut table = ItemTable::new();
    let err = table
        .update_no_fk(Item {
            id: ItemId(99),
            name: "Ghost".into(),
            weight: 0,
        })
        .unwrap_err();

    assert_eq!(
        err,
        Error::NotFound {
            table: "items",
            key: "ItemId(99)".into(),
        }
    );
}

#[test]
fn upsert_inserts_when_missing() {
    let mut table = ItemTable::new();
    table.upsert_no_fk(Item {
        id: ItemId(1),
        name: "Potion".into(),
        weight: 1,
    });

    assert_eq!(table.len(), 1);
    assert_eq!(table.get(&ItemId(1)).unwrap().name, "Potion");
}

#[test]
fn upsert_updates_when_existing() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Potion".into(),
            weight: 1,
        })
        .unwrap();

    table.upsert_no_fk(Item {
        id: ItemId(1),
        name: "Greater Potion".into(),
        weight: 2,
    });

    assert_eq!(table.len(), 1);
    assert_eq!(table.get(&ItemId(1)).unwrap().name, "Greater Potion");
}

#[test]
fn remove_existing_row() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let removed = table.remove_no_fk(&ItemId(1)).unwrap();
    assert_eq!(removed.name, "Sword");
    assert!(table.is_empty());
    assert!(!table.contains(&ItemId(1)));
}

#[test]
fn remove_not_found_error() {
    let mut table = ItemTable::new();
    let err = table.remove_no_fk(&ItemId(99)).unwrap_err();

    assert_eq!(
        err,
        Error::NotFound {
            table: "items",
            key: "ItemId(99)".into(),
        }
    );
}

#[test]
fn keys_and_iter_keys() {
    let mut table = ItemTable::new();
    for i in [3, 1, 2] {
        table
            .insert_no_fk(Item {
                id: ItemId(i),
                name: format!("Item {}", i),
                weight: i,
            })
            .unwrap();
    }

    // keys() returns in PK order
    assert_eq!(table.keys(), vec![ItemId(1), ItemId(2), ItemId(3)]);

    // iter_keys() returns in PK order
    let iter_keys: Vec<_> = table.iter_keys().cloned().collect();
    assert_eq!(iter_keys, vec![ItemId(1), ItemId(2), ItemId(3)]);
}

#[test]
fn all_and_iter_all() {
    let mut table = ItemTable::new();
    for i in [3, 1, 2] {
        table
            .insert_no_fk(Item {
                id: ItemId(i),
                name: format!("Item {}", i),
                weight: i,
            })
            .unwrap();
    }

    // all() returns in PK order
    let all = table.all();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, ItemId(1));
    assert_eq!(all[1].id, ItemId(2));
    assert_eq!(all[2].id, ItemId(3));

    // iter_all() returns in PK order
    let iter_ids: Vec<_> = table.iter_all().map(|item| item.id).collect();
    assert_eq!(iter_ids, vec![ItemId(1), ItemId(2), ItemId(3)]);
}
