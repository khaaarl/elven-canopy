//! Tests for per-row CRC with `#[table(checksummed)]`.
//!
//! Covers: RowEntry wrapper, CrcFeed derive, table-level XOR aggregation,
//! steady-state mutation flow (insert/update/upsert/remove/modify_unchecked),
//! checksum request flow (first-time full scan, incremental dirty drain),
//! and serde roundtrip (CRC is transient, not serialized).

use tabulosity::{Bounded, CrcFeed, Table};

// --- Test types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded, CrcFeed)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct ItemId(u32);

#[derive(Table, CrcFeed, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[table(checksummed)]
struct Item {
    #[primary_key]
    pub id: ItemId,
    pub name: String,
    pub weight: u32,
}

// --- CrcFeed derive tests ---

#[derive(CrcFeed, Clone, Debug, PartialEq)]
struct SimpleStruct {
    x: u32,
    y: String,
}

#[derive(CrcFeed, Clone, Debug, PartialEq)]
struct Newtype(u64);

#[derive(CrcFeed, Clone, Debug, PartialEq)]
enum Color {
    Red,
    Green,
    Blue(u8),
    Custom { r: u8, g: u8, b: u8 },
}

#[test]
fn crc_feed_derive_struct() {
    let a = SimpleStruct {
        x: 1,
        y: "hello".into(),
    };
    let b = SimpleStruct {
        x: 1,
        y: "hello".into(),
    };
    let c = SimpleStruct {
        x: 2,
        y: "hello".into(),
    };
    assert_eq!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&b));
    assert_ne!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&c));
}

#[test]
fn crc_feed_derive_newtype() {
    let a = Newtype(42);
    let b = Newtype(42);
    let c = Newtype(99);
    assert_eq!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&b));
    assert_ne!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&c));
}

#[test]
fn crc_feed_derive_enum() {
    assert_ne!(
        tabulosity::crc32_of(&Color::Red),
        tabulosity::crc32_of(&Color::Green)
    );
    assert_ne!(
        tabulosity::crc32_of(&Color::Blue(1)),
        tabulosity::crc32_of(&Color::Blue(2))
    );
    assert_eq!(
        tabulosity::crc32_of(&Color::Custom { r: 1, g: 2, b: 3 }),
        tabulosity::crc32_of(&Color::Custom { r: 1, g: 2, b: 3 })
    );
    assert_ne!(
        tabulosity::crc32_of(&Color::Custom { r: 1, g: 2, b: 3 }),
        tabulosity::crc32_of(&Color::Custom { r: 1, g: 2, b: 4 })
    );
}

// --- Checksummed table tests ---

#[test]
fn checksum_first_call_computes_full() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "Shield".into(),
            weight: 20,
        })
        .unwrap();

    let crc = table.checksum();
    assert!(crc.is_some());
    // Calling again without changes should return the same value.
    assert_eq!(table.checksum(), crc);
}

#[test]
fn checksum_empty_table() {
    let mut table = ItemTable::new();
    let crc = table.checksum();
    assert!(crc.is_some());
    // XOR of no elements should be 0.
    assert_eq!(crc, Some(0));
}

#[test]
fn checksum_changes_after_update() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    table
        .update_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 15,
        })
        .unwrap();

    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);
}

#[test]
fn checksum_changes_after_modify_unchecked() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    table
        .modify_unchecked(&ItemId(1), |item| {
            item.weight = 15;
        })
        .unwrap();

    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);
}

#[test]
fn checksum_changes_after_insert() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "Shield".into(),
            weight: 20,
        })
        .unwrap();

    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);
}

#[test]
fn checksum_changes_after_remove() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "Shield".into(),
            weight: 20,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    table.remove_no_fk(&ItemId(1)).unwrap();

    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);
}

#[test]
fn checksum_remove_all_returns_zero() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    let _ = table.checksum(); // Activate CRC.
    table.remove_no_fk(&ItemId(1)).unwrap();
    assert_eq!(table.checksum(), Some(0));
}

#[test]
fn checksum_insert_remove_same_row_is_identity() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    let crc1 = table.checksum().unwrap();

    // Insert and remove a second row — should return to original checksum.
    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "Shield".into(),
            weight: 20,
        })
        .unwrap();
    table.remove_no_fk(&ItemId(2)).unwrap();
    let crc2 = table.checksum().unwrap();
    assert_eq!(crc1, crc2);
}

#[test]
fn checksum_upsert_new_row() {
    let mut table = ItemTable::new();
    let _ = table.checksum(); // Activate CRC.

    table
        .upsert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let crc = table.checksum().unwrap();
    assert_ne!(crc, 0); // Non-empty table should have non-zero CRC (almost certainly).
}

#[test]
fn checksum_upsert_existing_row() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    let crc1 = table.checksum().unwrap();

    table
        .upsert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 15,
        })
        .unwrap();
    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);
}

#[test]
fn checksum_order_independent() {
    // Insert A then B vs. B then A should yield the same checksum (XOR is commutative).
    let mut t1 = ItemTable::new();
    t1.insert_no_fk(Item {
        id: ItemId(1),
        name: "Sword".into(),
        weight: 10,
    })
    .unwrap();
    t1.insert_no_fk(Item {
        id: ItemId(2),
        name: "Shield".into(),
        weight: 20,
    })
    .unwrap();

    let mut t2 = ItemTable::new();
    t2.insert_no_fk(Item {
        id: ItemId(2),
        name: "Shield".into(),
        weight: 20,
    })
    .unwrap();
    t2.insert_no_fk(Item {
        id: ItemId(1),
        name: "Sword".into(),
        weight: 10,
    })
    .unwrap();

    assert_eq!(t1.checksum(), t2.checksum());
}

#[test]
fn checksum_multiple_mutations_before_checksum() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    let _ = table.checksum(); // Activate.

    // Multiple mutations without checksum in between.
    table
        .modify_unchecked(&ItemId(1), |item| item.weight = 20)
        .unwrap();
    table
        .modify_unchecked(&ItemId(1), |item| item.weight = 30)
        .unwrap();

    let crc = table.checksum().unwrap();

    // Should match a fresh table with the final state.
    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 30,
        })
        .unwrap();
    assert_eq!(fresh.checksum(), Some(crc));
}

#[test]
fn checksum_dirty_row_removed_before_drain() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "Shield".into(),
            weight: 20,
        })
        .unwrap();
    let _ = table.checksum(); // Activate.

    // Dirty row 1, then remove it before checksum.
    table
        .modify_unchecked(&ItemId(1), |item| item.weight = 99)
        .unwrap();
    table.remove_no_fk(&ItemId(1)).unwrap();

    // Should equal a table with only row 2.
    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "Shield".into(),
            weight: 20,
        })
        .unwrap();
    assert_eq!(table.checksum(), fresh.checksum());
}

#[test]
fn checksum_inactive_until_first_request() {
    let mut table = ItemTable::new();
    // Insert and remove without ever calling checksum — should be inert.
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
            name: "Sword".into(),
            weight: 20,
        })
        .unwrap();
    table
        .modify_unchecked(&ItemId(1), |item| item.weight = 30)
        .unwrap();

    // Now activate — should compute from scratch correctly.
    let crc = table.checksum().unwrap();

    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 30,
        })
        .unwrap();
    assert_eq!(fresh.checksum(), Some(crc));
}

#[test]
fn modify_unchecked_range_dirties_rows() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "A".into(),
            weight: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "B".into(),
            weight: 20,
        })
        .unwrap();
    let crc1 = table.checksum().unwrap();

    // Use asymmetric mutation to avoid XOR collision.
    let count = table.modify_unchecked_all(|pk, item| {
        item.weight += pk.0 * 100;
    });
    assert_eq!(count, 2, "modify_unchecked_all should have modified 2 rows");

    // Verify the rows actually changed.
    assert_eq!(table.get(&ItemId(1)).unwrap().weight, 110);
    assert_eq!(table.get(&ItemId(2)).unwrap().weight, 220);

    let crc2 = table.checksum().unwrap();

    // Compare with a fresh table built from the post-mutation state.
    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "A".into(),
            weight: 110,
        })
        .unwrap();
    fresh
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "B".into(),
            weight: 220,
        })
        .unwrap();
    let crc_fresh = fresh.checksum().unwrap();

    // Incremental checksum must match the from-scratch checksum.
    assert_eq!(
        crc2, crc_fresh,
        "incremental checksum should match fresh computation"
    );
    assert_ne!(crc1, crc2);
}

#[test]
fn modify_unchecked_range_bounded_dirties_only_affected_rows() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "A".into(),
            weight: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "B".into(),
            weight: 20,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(3),
            name: "C".into(),
            weight: 30,
        })
        .unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(4),
            name: "D".into(),
            weight: 40,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    // Modify only rows 2 and 3 via a bounded range.
    let count = table.modify_unchecked_range(ItemId(2)..=ItemId(3), |_pk, item| {
        item.weight += 100;
    });
    assert_eq!(count, 2);

    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);

    // Compare with a fresh table that has the expected final state.
    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "A".into(),
            weight: 10,
        })
        .unwrap();
    fresh
        .insert_no_fk(Item {
            id: ItemId(2),
            name: "B".into(),
            weight: 120,
        })
        .unwrap();
    fresh
        .insert_no_fk(Item {
            id: ItemId(3),
            name: "C".into(),
            weight: 130,
        })
        .unwrap();
    fresh
        .insert_no_fk(Item {
            id: ItemId(4),
            name: "D".into(),
            weight: 40,
        })
        .unwrap();
    assert_eq!(crc2, fresh.checksum().unwrap());
}

// --- Indexed checksummed table for modify_each_by tests ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded, CrcFeed)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct UnitId(u32);

#[derive(Table, CrcFeed, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[table(checksummed)]
struct Unit {
    #[primary_key]
    pub id: UnitId,
    #[indexed]
    pub team: u32,
    pub hp: u32,
}

#[test]
fn modify_each_by_dirties_crc() {
    let mut table = UnitTable::new();
    table
        .insert_no_fk(Unit {
            id: UnitId(1),
            team: 1,
            hp: 100,
        })
        .unwrap();
    table
        .insert_no_fk(Unit {
            id: UnitId(2),
            team: 1,
            hp: 80,
        })
        .unwrap();
    table
        .insert_no_fk(Unit {
            id: UnitId(3),
            team: 2,
            hp: 90,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    // Mutate only team 1 units via modify_each_by_team.
    let count = table.modify_each_by_team(&1, tabulosity::QueryOpts::ASC, |_pk, unit| {
        unit.hp -= 10;
    });
    assert_eq!(count, 2);

    let crc2 = table.checksum().unwrap();

    // Compare with fresh table.
    let mut fresh = UnitTable::new();
    fresh
        .insert_no_fk(Unit {
            id: UnitId(1),
            team: 1,
            hp: 90,
        })
        .unwrap();
    fresh
        .insert_no_fk(Unit {
            id: UnitId(2),
            team: 1,
            hp: 70,
        })
        .unwrap();
    fresh
        .insert_no_fk(Unit {
            id: UnitId(3),
            team: 2,
            hp: 90,
        })
        .unwrap();
    assert_eq!(crc2, fresh.checksum().unwrap());
    assert_ne!(crc1, crc2);
}

#[test]
fn insert_remove_reinsert_same_pk_between_checksums() {
    let mut table = ItemTable::new();
    let _ = table.checksum(); // Activate CRC on empty table.

    // Insert, remove, reinsert with same PK but different data.
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    table.remove_no_fk(&ItemId(1)).unwrap();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Axe".into(),
            weight: 15,
        })
        .unwrap();

    let crc = table.checksum().unwrap();

    // Must match a fresh table with only the final row.
    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Axe".into(),
            weight: 15,
        })
        .unwrap();
    assert_eq!(crc, fresh.checksum().unwrap());
}

#[test]
fn double_update_between_checksums() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    let _ = table.checksum(); // Activate.

    // Two updates without checksum in between.
    table
        .update_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 20,
        })
        .unwrap();
    table
        .update_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 30,
        })
        .unwrap();

    let crc = table.checksum().unwrap();

    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 30,
        })
        .unwrap();
    assert_eq!(crc, fresh.checksum().unwrap());
}

// --- CrcFeed derive edge cases ---

#[derive(CrcFeed)]
struct UnitStruct;

#[test]
fn crc_feed_derive_unit_struct() {
    // Unit struct feeds zero bytes — should produce a consistent CRC.
    assert_eq!(
        tabulosity::crc32_of(&UnitStruct),
        tabulosity::crc32_of(&UnitStruct)
    );
}

#[derive(CrcFeed)]
struct Pair(u32, u64);

#[test]
fn crc_feed_derive_multi_field_tuple_struct() {
    let a = Pair(1, 2);
    let b = Pair(1, 2);
    let c = Pair(1, 3);
    assert_eq!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&b));
    assert_ne!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&c));
}

#[derive(CrcFeed)]
struct Wrapper<T> {
    inner: T,
}

#[test]
fn crc_feed_derive_generic_struct() {
    let a = Wrapper { inner: 42u32 };
    let b = Wrapper { inner: 42u32 };
    let c = Wrapper { inner: 99u32 };
    assert_eq!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&b));
    assert_ne!(tabulosity::crc32_of(&a), tabulosity::crc32_of(&c));
}

// --- Unique-indexed checksummed table ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded, CrcFeed)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct SlotId(u32);

#[derive(Table, CrcFeed, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[table(checksummed)]
struct Slot {
    #[primary_key]
    pub id: SlotId,
    #[indexed(unique)]
    pub label: String,
    pub value: u32,
}

#[test]
fn update_unique_check_failure_preserves_crc() {
    let mut table = SlotTable::new();
    table
        .insert_no_fk(Slot {
            id: SlotId(1),
            label: "alpha".into(),
            value: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Slot {
            id: SlotId(2),
            label: "beta".into(),
            value: 20,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    // Attempt update that violates the unique constraint on `label`.
    let result = table.update_no_fk(Slot {
        id: SlotId(1),
        label: "beta".into(), // conflicts with SlotId(2)
        value: 99,
    });
    assert!(result.is_err());

    // Checksum must be unchanged — the failed update should not corrupt CRC state.
    let crc2 = table.checksum().unwrap();
    assert_eq!(crc1, crc2);
}

#[test]
fn upsert_unique_check_failure_preserves_crc() {
    let mut table = SlotTable::new();
    table
        .insert_no_fk(Slot {
            id: SlotId(1),
            label: "alpha".into(),
            value: 10,
        })
        .unwrap();
    table
        .insert_no_fk(Slot {
            id: SlotId(2),
            label: "beta".into(),
            value: 20,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    // Attempt upsert that violates the unique constraint on `label`.
    let result = table.upsert_no_fk(Slot {
        id: SlotId(1),
        label: "beta".into(),
        value: 99,
    });
    assert!(result.is_err());

    let crc2 = table.checksum().unwrap();
    assert_eq!(crc1, crc2);
}

// --- Compound PK checksummed table ---

#[derive(Table, CrcFeed, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[table(checksummed)]
#[primary_key("owner", "seq")]
struct Task {
    pub owner: u32,
    pub seq: u32,
    pub label: String,
}

#[test]
fn checksum_with_compound_pk() {
    let mut table = TaskTable::new();
    table
        .insert_no_fk(Task {
            owner: 1,
            seq: 0,
            label: "build".into(),
        })
        .unwrap();
    table
        .insert_no_fk(Task {
            owner: 1,
            seq: 1,
            label: "cook".into(),
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    table
        .modify_unchecked(&(1, 1), |t| {
            t.label = "forge".into();
        })
        .unwrap();

    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);

    // Compare with fresh.
    let mut fresh = TaskTable::new();
    fresh
        .insert_no_fk(Task {
            owner: 1,
            seq: 0,
            label: "build".into(),
        })
        .unwrap();
    fresh
        .insert_no_fk(Task {
            owner: 1,
            seq: 1,
            label: "forge".into(),
        })
        .unwrap();
    assert_eq!(crc2, fresh.checksum().unwrap());
}

#[test]
fn modify_unchecked_noop_preserves_checksum() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    // Closure that changes nothing.
    table.modify_unchecked(&ItemId(1), |_item| {}).unwrap();

    let crc2 = table.checksum().unwrap();
    assert_eq!(crc1, crc2);
}

#[test]
fn modify_unchecked_then_update_same_row() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    let _ = table.checksum();

    // Dirty via modify_unchecked, then replace via update.
    table
        .modify_unchecked(&ItemId(1), |item| {
            item.weight = 20;
        })
        .unwrap();
    table
        .update_no_fk(Item {
            id: ItemId(1),
            name: "Axe".into(),
            weight: 30,
        })
        .unwrap();

    let crc = table.checksum().unwrap();

    let mut fresh = ItemTable::new();
    fresh
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Axe".into(),
            weight: 30,
        })
        .unwrap();
    assert_eq!(crc, fresh.checksum().unwrap());
}

#[test]
fn insert_duplicate_key_preserves_crc() {
    let mut table = ItemTable::new();
    table
        .insert_no_fk(Item {
            id: ItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    let crc1 = table.checksum().unwrap();

    // Duplicate key insert should fail without corrupting CRC.
    let result = table.insert_no_fk(Item {
        id: ItemId(1),
        name: "Axe".into(),
        weight: 20,
    });
    assert!(result.is_err());

    let crc2 = table.checksum().unwrap();
    assert_eq!(crc1, crc2);
}

#[test]
fn insert_unique_check_failure_preserves_crc() {
    let mut table = SlotTable::new();
    table
        .insert_no_fk(Slot {
            id: SlotId(1),
            label: "alpha".into(),
            value: 10,
        })
        .unwrap();
    let crc1 = table.checksum().unwrap();

    // Insert with duplicate unique label should fail without corrupting CRC.
    let result = table.insert_no_fk(Slot {
        id: SlotId(2),
        label: "alpha".into(),
        value: 20,
    });
    assert!(result.is_err());

    let crc2 = table.checksum().unwrap();
    assert_eq!(crc1, crc2);
}

// --- Hash primary storage + checksummed ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Bounded, CrcFeed)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct TagId(u32);

#[derive(Table, CrcFeed, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[table(primary_storage = "hash", checksummed)]
struct Tag {
    #[primary_key]
    pub id: TagId,
    pub label: String,
}

#[test]
fn hash_primary_storage_checksummed() {
    let mut table = TagTable::new();
    table
        .insert_no_fk(Tag {
            id: TagId(1),
            label: "red".into(),
        })
        .unwrap();
    table
        .insert_no_fk(Tag {
            id: TagId(2),
            label: "blue".into(),
        })
        .unwrap();

    let crc1 = table.checksum().unwrap();

    table
        .modify_unchecked(&TagId(1), |t| {
            t.label = "green".into();
        })
        .unwrap();

    let crc2 = table.checksum().unwrap();
    assert_ne!(crc1, crc2);

    // modify_unchecked_all on hash storage.
    table.modify_unchecked_all(|_pk, t| {
        t.label = format!("{}!", t.label);
    });

    let crc3 = table.checksum().unwrap();
    assert_ne!(crc2, crc3);
}

#[test]
fn overlapping_ranges_with_interleaved_modify() {
    let mut table = ItemTable::new();
    for i in 1..=10 {
        table
            .insert_no_fk(Item {
                id: ItemId(i),
                name: format!("item{i}"),
                weight: i * 10,
            })
            .unwrap();
    }
    let _ = table.checksum(); // Activate.

    // Overlapping range modifications with a single-row modify in between.
    table.modify_unchecked_range(ItemId(1)..=ItemId(5), |_pk, item| {
        item.weight += 1;
    });
    table
        .modify_unchecked(&ItemId(3), |item| {
            item.weight += 100;
        })
        .unwrap();
    table.modify_unchecked_range(ItemId(4)..=ItemId(8), |_pk, item| {
        item.weight += 2;
    });

    let crc = table.checksum().unwrap();

    // Build fresh table with expected final state.
    let mut fresh = ItemTable::new();
    // Row 1: +1 (from first range)
    // Row 2: +1 (from first range)
    // Row 3: +1 (first range) +100 (single modify) = +101
    // Row 4: +1 (first range) +2 (second range) = +3
    // Row 5: +1 (first range) +2 (second range) = +3
    // Row 6: +2 (second range only)
    // Row 7: +2 (second range only)
    // Row 8: +2 (second range only)
    // Row 9-10: unchanged
    let expected_weights = [11, 21, 131, 43, 53, 62, 72, 82, 90, 100];
    for (i, &w) in expected_weights.iter().enumerate() {
        let id = (i + 1) as u32;
        fresh
            .insert_no_fk(Item {
                id: ItemId(id),
                name: format!("item{id}"),
                weight: w,
            })
            .unwrap();
    }
    assert_eq!(crc, fresh.checksum().unwrap());
}

#[test]
fn single_row_checksum_equals_crc32_of_row() {
    let row = Item {
        id: ItemId(1),
        name: "Sword".into(),
        weight: 10,
    };
    let mut table = ItemTable::new();
    table.insert_no_fk(row.clone()).unwrap();

    // For a single-row table, the XOR of one value is that value.
    let table_crc = table.checksum().unwrap();
    let row_crc = tabulosity::crc32_of(&row);
    assert_eq!(table_crc, row_crc);
}
