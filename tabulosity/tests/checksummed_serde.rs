//! Serde tests for `#[table(checksummed)]`.
//!
//! Verifies that CRC state is transient (not serialized) and that
//! deserialized tables compute correct checksums from scratch.

#![cfg(feature = "serde")]

use tabulosity::{Bounded, CrcFeed, Table};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Bounded,
    CrcFeed,
    serde::Serialize,
    serde::Deserialize,
)]
struct SerItemId(u32);

#[derive(Table, CrcFeed, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[table(checksummed)]
struct SerItem {
    #[primary_key]
    pub id: SerItemId,
    pub name: String,
    pub weight: u32,
}

#[test]
fn serde_roundtrip_checksummed_table() {
    let mut table = SerItemTable::new();
    table
        .insert_no_fk(SerItem {
            id: SerItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();
    table
        .insert_no_fk(SerItem {
            id: SerItemId(2),
            name: "Shield".into(),
            weight: 20,
        })
        .unwrap();

    // Activate CRC.
    let crc_before = table.checksum().unwrap();

    // Serialize and deserialize.
    let json = serde_json::to_string(&table).unwrap();
    let mut restored: SerItemTable = serde_json::from_str(&json).unwrap();

    // Data should match.
    assert_eq!(restored.all(), table.all());

    // CRC is transient — starts inert on deserialization.
    // First checksum on restored table should match the original.
    let crc_after = restored.checksum().unwrap();
    assert_eq!(crc_before, crc_after);
}

#[test]
fn serde_roundtrip_then_mutate_then_checksum() {
    let mut table = SerItemTable::new();
    table
        .insert_no_fk(SerItem {
            id: SerItemId(1),
            name: "Sword".into(),
            weight: 10,
        })
        .unwrap();

    let json = serde_json::to_string(&table).unwrap();
    let mut restored: SerItemTable = serde_json::from_str(&json).unwrap();

    // CRC is inert after deserialization. Mutate, then checksum.
    restored
        .modify_unchecked(&SerItemId(1), |item| {
            item.weight = 99;
        })
        .unwrap();

    let crc = restored.checksum().unwrap();

    // Must match a fresh table with the mutated state.
    let mut fresh = SerItemTable::new();
    fresh
        .insert_no_fk(SerItem {
            id: SerItemId(1),
            name: "Sword".into(),
            weight: 99,
        })
        .unwrap();
    assert_eq!(crc, fresh.checksum().unwrap());
}
