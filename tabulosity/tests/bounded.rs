//! Integration tests for `#[derive(Bounded)]`.

use tabulosity::Bounded;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct CreatureId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct SmallId(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Bounded)]
struct SignedId(i32);

#[test]
fn bounded_u64_newtype() {
    assert_eq!(CreatureId::MIN, CreatureId(0));
    assert_eq!(CreatureId::MAX, CreatureId(u64::MAX));
}

#[test]
fn bounded_u8_newtype() {
    assert_eq!(SmallId::MIN, SmallId(0));
    assert_eq!(SmallId::MAX, SmallId(255));
}

#[test]
fn bounded_i32_newtype() {
    assert_eq!(SignedId::MIN, SignedId(i32::MIN));
    assert_eq!(SignedId::MAX, SignedId(i32::MAX));
}

#[test]
fn bounded_ordering() {
    assert!(CreatureId::MIN < CreatureId::MAX);
    assert!(SmallId::MIN < SmallId::MAX);
}

#[test]
fn bounded_option_of_newtype() {
    assert_eq!(<Option<CreatureId> as Bounded>::MIN, None);
    assert_eq!(
        <Option<CreatureId> as Bounded>::MAX,
        Some(CreatureId(u64::MAX))
    );
}
