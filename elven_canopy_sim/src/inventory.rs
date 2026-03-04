// Item and inventory data model.
//
// Provides `ItemKind`, `Item`, and `GroundPile` types plus helper functions
// for manipulating inventories (add, remove, count, reserve). Inventories are
// `Vec<Item>` stored on `Creature` (in `sim.rs`), `CompletedStructure` (in
// `building.rs`), and as `GroundPile`s in `SimState.ground_piles`.
//
// Items stack: adding bread to an inventory that already has bread increases
// the existing stack's quantity rather than creating a duplicate entry.
// Stacking matches on `(kind, owner, reserved_by)`.
//
// See also: `sim.rs` for creature inventories, `building.rs` for structure
// inventories, `sim_bridge.rs` for the GDExtension interface that exposes
// inventories to GDScript.
//
// **Critical constraint: determinism.** All operations are pure functions over
// `Vec<Item>` — no hash-based collections, no OS calls.

use crate::types::{CreatureId, TaskId, VoxelCoord};
use serde::{Deserialize, Serialize};

/// The kind of item. Each variant is a distinct item type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    Bread,
    Fruit,
}

impl ItemKind {
    /// Human-readable display name for this item kind.
    pub fn display_name(self) -> &'static str {
        match self {
            ItemKind::Bread => "Bread",
            ItemKind::Fruit => "Fruit",
        }
    }
}

/// A stack of items in an inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: ItemKind,
    pub quantity: u32,
    /// Which creature owns this item (for tracking provenance).
    pub owner: Option<CreatureId>,
    /// Which task has reserved this stack, if any. `None` means unreserved.
    /// Old saves with the former `task_related` field will ignore it (serde
    /// ignores unknown fields by default), and this defaults to `None`.
    #[serde(default)]
    pub reserved_by: Option<TaskId>,
}

/// A pile of items on the ground at a specific voxel position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundPile {
    pub position: VoxelCoord,
    pub items: Vec<Item>,
}

/// Add items to an inventory, stacking with existing items that match
/// `(kind, owner, reserved_by)`. Returns the resulting quantity of
/// that item kind in the stack.
pub fn add_item(
    inventory: &mut Vec<Item>,
    kind: ItemKind,
    quantity: u32,
    owner: Option<CreatureId>,
    reserved_by: Option<TaskId>,
) -> u32 {
    for item in inventory.iter_mut() {
        if item.kind == kind && item.owner == owner && item.reserved_by == reserved_by {
            item.quantity += quantity;
            return item.quantity;
        }
    }
    inventory.push(Item {
        kind,
        quantity,
        owner,
        reserved_by,
    });
    quantity
}

/// Remove up to `quantity` items of the given kind from an inventory.
/// Returns the amount actually removed. Drops stacks that reach zero.
pub fn remove_item(inventory: &mut Vec<Item>, kind: ItemKind, quantity: u32) -> u32 {
    let mut remaining = quantity;
    let mut removed = 0u32;

    for item in inventory.iter_mut() {
        if item.kind == kind && remaining > 0 {
            let take = remaining.min(item.quantity);
            item.quantity -= take;
            remaining -= take;
            removed += take;
        }
    }

    // Drop empty stacks.
    inventory.retain(|item| item.quantity > 0);

    removed
}

/// Count the total quantity of a given item kind across all stacks.
pub fn item_count(inventory: &[Item], kind: ItemKind) -> u32 {
    inventory
        .iter()
        .filter(|item| item.kind == kind)
        .map(|item| item.quantity)
        .sum()
}

/// Count items of a given kind owned by a specific creature.
pub fn count_owned(inventory: &[Item], kind: ItemKind, owner: CreatureId) -> u32 {
    inventory
        .iter()
        .filter(|item| item.kind == kind && item.owner == Some(owner))
        .map(|item| item.quantity)
        .sum()
}

/// Count unreserved items of the given kind (stacks with `reserved_by == None`).
pub fn unreserved_item_count(inventory: &[Item], kind: ItemKind) -> u32 {
    inventory
        .iter()
        .filter(|item| item.kind == kind && item.reserved_by.is_none())
        .map(|item| item.quantity)
        .sum()
}

/// Remove up to `quantity` items of the given kind owned by a specific creature.
/// Returns the amount actually removed. Drops stacks that reach zero.
pub fn remove_owned_item(
    inventory: &mut Vec<Item>,
    kind: ItemKind,
    owner: CreatureId,
    quantity: u32,
) -> u32 {
    let mut remaining = quantity;
    let mut removed = 0u32;

    for item in inventory.iter_mut() {
        if item.kind == kind && item.owner == Some(owner) && remaining > 0 {
            let take = remaining.min(item.quantity);
            item.quantity -= take;
            remaining -= take;
            removed += take;
        }
    }

    // Drop empty stacks.
    inventory.retain(|item| item.quantity > 0);

    removed
}

/// Reserve up to `quantity` unreserved items of the given kind for `task_id`.
///
/// Splits stacks as needed: if a stack has more than needed, it is split into
/// a reserved portion and an unreserved remainder. Returns the amount actually
/// reserved (may be less than requested if fewer are available).
pub fn reserve_items(
    inventory: &mut Vec<Item>,
    kind: ItemKind,
    quantity: u32,
    task_id: TaskId,
) -> u32 {
    let mut remaining = quantity;
    let mut reserved = 0u32;

    // First pass: reserve from existing unreserved stacks, splitting if needed.
    let mut new_items = Vec::new();
    for item in inventory.iter_mut() {
        if item.kind == kind && item.reserved_by.is_none() && remaining > 0 {
            let take = remaining.min(item.quantity);
            if take == item.quantity {
                // Reserve the entire stack.
                item.reserved_by = Some(task_id);
            } else {
                // Split: reduce this stack and create a new reserved stack.
                item.quantity -= take;
                new_items.push(Item {
                    kind,
                    quantity: take,
                    owner: item.owner,
                    reserved_by: Some(task_id),
                });
            }
            remaining -= take;
            reserved += take;
        }
    }

    inventory.extend(new_items);
    reserved
}

/// Clear all reservations for the given `task_id`, setting `reserved_by` to `None`.
///
/// After clearing, re-merges stacks that now match on `(kind, owner, None)`.
pub fn clear_reservations(inventory: &mut Vec<Item>, task_id: TaskId) {
    for item in inventory.iter_mut() {
        if item.reserved_by == Some(task_id) {
            item.reserved_by = None;
        }
    }
    merge_stacks(inventory);
}

/// Remove up to `quantity` items of the given kind that are reserved by `task_id`.
/// Returns the amount actually removed. Drops stacks that reach zero.
pub fn remove_reserved_items(
    inventory: &mut Vec<Item>,
    kind: ItemKind,
    quantity: u32,
    task_id: TaskId,
) -> u32 {
    let mut remaining = quantity;
    let mut removed = 0u32;

    for item in inventory.iter_mut() {
        if item.kind == kind && item.reserved_by == Some(task_id) && remaining > 0 {
            let take = remaining.min(item.quantity);
            item.quantity -= take;
            remaining -= take;
            removed += take;
        }
    }

    inventory.retain(|item| item.quantity > 0);
    removed
}

/// Count items of a given kind that are unowned (`owner == None`) and
/// unreserved (`reserved_by == None`).
pub fn count_unowned_unreserved(inventory: &[Item], kind: ItemKind) -> u32 {
    inventory
        .iter()
        .filter(|item| item.kind == kind && item.owner.is_none() && item.reserved_by.is_none())
        .map(|item| item.quantity)
        .sum()
}

/// Reserve up to `quantity` unowned (`owner == None`) unreserved items of the
/// given kind for `task_id`. Splits stacks as needed, like `reserve_items`.
/// Returns the amount actually reserved.
pub fn reserve_unowned_items(
    inventory: &mut Vec<Item>,
    kind: ItemKind,
    quantity: u32,
    task_id: TaskId,
) -> u32 {
    let mut remaining = quantity;
    let mut reserved = 0u32;

    let mut new_items = Vec::new();
    for item in inventory.iter_mut() {
        if item.kind == kind && item.owner.is_none() && item.reserved_by.is_none() && remaining > 0
        {
            let take = remaining.min(item.quantity);
            if take == item.quantity {
                item.reserved_by = Some(task_id);
            } else {
                item.quantity -= take;
                new_items.push(Item {
                    kind,
                    quantity: take,
                    owner: None,
                    reserved_by: Some(task_id),
                });
            }
            remaining -= take;
            reserved += take;
        }
    }

    inventory.extend(new_items);
    reserved
}

/// Merge stacks that match on `(kind, owner, reserved_by)`.
fn merge_stacks(inventory: &mut Vec<Item>) {
    let mut i = 0;
    while i < inventory.len() {
        let mut j = i + 1;
        while j < inventory.len() {
            if inventory[i].kind == inventory[j].kind
                && inventory[i].owner == inventory[j].owner
                && inventory[i].reserved_by == inventory[j].reserved_by
            {
                inventory[i].quantity += inventory[j].quantity;
                inventory.remove(j);
            } else {
                j += 1;
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::GameRng;
    use crate::types::{CreatureId, TaskId};

    #[test]
    fn add_creates_new_stack() {
        let mut inv = Vec::new();
        let result = add_item(&mut inv, ItemKind::Bread, 3, None, None);
        assert_eq!(result, 3);
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].kind, ItemKind::Bread);
        assert_eq!(inv[0].quantity, 3);
    }

    #[test]
    fn add_stacks_matching_items() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, None, None);
        let result = add_item(&mut inv, ItemKind::Bread, 5, None, None);
        assert_eq!(result, 7);
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].quantity, 7);
    }

    #[test]
    fn add_different_owner_creates_separate_stack() {
        let mut rng = GameRng::new(42);
        let owner_a = CreatureId::new(&mut rng);
        let owner_b = CreatureId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, Some(owner_a), None);
        add_item(&mut inv, ItemKind::Bread, 3, Some(owner_b), None);
        assert_eq!(inv.len(), 2);
        assert_eq!(inv[0].quantity, 2);
        assert_eq!(inv[1].quantity, 3);
    }

    #[test]
    fn add_different_reserved_by_creates_separate_stack() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, None, None);
        add_item(&mut inv, ItemKind::Bread, 3, None, Some(task_id));
        assert_eq!(inv.len(), 2);
    }

    #[test]
    fn remove_decreases_quantity() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None);
        let removed = remove_item(&mut inv, ItemKind::Bread, 3);
        assert_eq!(removed, 3);
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].quantity, 2);
    }

    #[test]
    fn remove_drops_empty_stack() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, None, None);
        let removed = remove_item(&mut inv, ItemKind::Bread, 3);
        assert_eq!(removed, 3);
        assert!(inv.is_empty());
    }

    #[test]
    fn remove_caps_at_available() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, None, None);
        let removed = remove_item(&mut inv, ItemKind::Bread, 10);
        assert_eq!(removed, 2);
        assert!(inv.is_empty());
    }

    #[test]
    fn remove_from_empty_returns_zero() {
        let mut inv: Vec<Item> = Vec::new();
        let removed = remove_item(&mut inv, ItemKind::Bread, 5);
        assert_eq!(removed, 0);
    }

    #[test]
    fn item_count_sums_across_stacks() {
        let mut rng = GameRng::new(42);
        let owner = CreatureId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, None, None);
        add_item(&mut inv, ItemKind::Bread, 2, Some(owner), None);
        assert_eq!(item_count(&inv, ItemKind::Bread), 5);
    }

    #[test]
    fn item_count_empty_inventory() {
        let inv: Vec<Item> = Vec::new();
        assert_eq!(item_count(&inv, ItemKind::Bread), 0);
    }

    #[test]
    fn display_name() {
        assert_eq!(ItemKind::Bread.display_name(), "Bread");
        assert_eq!(ItemKind::Fruit.display_name(), "Fruit");
    }

    #[test]
    fn serialization_roundtrip() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 7, None, None);

        let json = serde_json::to_string(&inv).unwrap();
        let restored: Vec<Item> = serde_json::from_str(&json).unwrap();
        assert_eq!(inv, restored);
    }

    #[test]
    fn ground_pile_serialization() {
        let pile = GroundPile {
            position: VoxelCoord::new(10, 5, 20),
            items: vec![Item {
                kind: ItemKind::Bread,
                quantity: 4,
                owner: None,
                reserved_by: None,
            }],
        };

        let json = serde_json::to_string(&pile).unwrap();
        let restored: GroundPile = serde_json::from_str(&json).unwrap();
        assert_eq!(pile, restored);
    }

    #[test]
    fn count_owned_filters_by_owner() {
        let mut rng = GameRng::new(42);
        let owner_a = CreatureId::new(&mut rng);
        let owner_b = CreatureId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, Some(owner_a), None);
        add_item(&mut inv, ItemKind::Bread, 5, Some(owner_b), None);
        add_item(&mut inv, ItemKind::Bread, 2, None, None);

        assert_eq!(count_owned(&inv, ItemKind::Bread, owner_a), 3);
        assert_eq!(count_owned(&inv, ItemKind::Bread, owner_b), 5);
    }

    #[test]
    fn count_owned_returns_zero_when_none() {
        let mut rng = GameRng::new(42);
        let owner = CreatureId::new(&mut rng);

        let inv: Vec<Item> = Vec::new();
        assert_eq!(count_owned(&inv, ItemKind::Bread, owner), 0);
    }

    #[test]
    fn remove_owned_item_removes_only_owned() {
        let mut rng = GameRng::new(42);
        let owner_a = CreatureId::new(&mut rng);
        let owner_b = CreatureId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, Some(owner_a), None);
        add_item(&mut inv, ItemKind::Bread, 5, Some(owner_b), None);

        let removed = remove_owned_item(&mut inv, ItemKind::Bread, owner_a, 2);
        assert_eq!(removed, 2);
        assert_eq!(count_owned(&inv, ItemKind::Bread, owner_a), 1);
        assert_eq!(count_owned(&inv, ItemKind::Bread, owner_b), 5);
    }

    #[test]
    fn remove_owned_item_caps_at_available() {
        let mut rng = GameRng::new(42);
        let owner = CreatureId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, Some(owner), None);

        let removed = remove_owned_item(&mut inv, ItemKind::Bread, owner, 10);
        assert_eq!(removed, 2);
        assert!(inv.is_empty());
    }

    #[test]
    fn remove_owned_item_ignores_unowned() {
        let mut rng = GameRng::new(42);
        let owner = CreatureId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None);

        let removed = remove_owned_item(&mut inv, ItemKind::Bread, owner, 3);
        assert_eq!(removed, 0);
        assert_eq!(item_count(&inv, ItemKind::Bread), 5);
    }

    #[test]
    fn test_fruit_item_kind() {
        assert_eq!(ItemKind::Fruit.display_name(), "Fruit");
        // Fruit and Bread are distinct kinds and don't stack together.
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, None, None);
        add_item(&mut inv, ItemKind::Fruit, 2, None, None);
        assert_eq!(inv.len(), 2);
        assert_eq!(item_count(&inv, ItemKind::Bread), 3);
        assert_eq!(item_count(&inv, ItemKind::Fruit), 2);
    }

    #[test]
    fn test_reserve_items_splits_stack() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None);
        let reserved = reserve_items(&mut inv, ItemKind::Bread, 3, task_id);
        assert_eq!(reserved, 3);
        // Should have two stacks: 2 unreserved + 3 reserved.
        assert_eq!(inv.len(), 2);
        assert_eq!(unreserved_item_count(&inv, ItemKind::Bread), 2);
        let reserved_count: u32 = inv
            .iter()
            .filter(|i| i.kind == ItemKind::Bread && i.reserved_by == Some(task_id))
            .map(|i| i.quantity)
            .sum();
        assert_eq!(reserved_count, 3);
    }

    #[test]
    fn test_reserve_items_caps_at_available() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, None, None);
        let reserved = reserve_items(&mut inv, ItemKind::Bread, 10, task_id);
        assert_eq!(reserved, 3);
        assert_eq!(unreserved_item_count(&inv, ItemKind::Bread), 0);
    }

    #[test]
    fn test_unreserved_item_count() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None);
        reserve_items(&mut inv, ItemKind::Bread, 2, task_id);
        assert_eq!(unreserved_item_count(&inv, ItemKind::Bread), 3);
        assert_eq!(item_count(&inv, ItemKind::Bread), 5);
    }

    #[test]
    fn test_clear_reservations() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None);
        reserve_items(&mut inv, ItemKind::Bread, 3, task_id);
        assert_eq!(inv.len(), 2); // split into 2 + 3
        clear_reservations(&mut inv, task_id);
        // Should be merged back into one stack of 5.
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].quantity, 5);
        assert_eq!(inv[0].reserved_by, None);
    }

    #[test]
    fn test_remove_reserved_items() {
        let mut rng = GameRng::new(42);
        let task_a = TaskId::new(&mut rng);
        let task_b = TaskId::new(&mut rng);
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None);
        reserve_items(&mut inv, ItemKind::Bread, 3, task_a);
        reserve_items(&mut inv, ItemKind::Bread, 2, task_b);
        // Only remove items reserved by task_a.
        let removed = remove_reserved_items(&mut inv, ItemKind::Bread, 3, task_a);
        assert_eq!(removed, 3);
        // task_b's items should still be there.
        assert_eq!(item_count(&inv, ItemKind::Bread), 2);
        let b_count: u32 = inv
            .iter()
            .filter(|i| i.reserved_by == Some(task_b))
            .map(|i| i.quantity)
            .sum();
        assert_eq!(b_count, 2);
    }

    #[test]
    fn count_unowned_unreserved_filters_correctly() {
        let mut rng = GameRng::new(42);
        let owner = CreatureId::new(&mut rng);
        let task_id = TaskId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None); // unowned, unreserved
        add_item(&mut inv, ItemKind::Bread, 3, Some(owner), None); // owned
        add_item(&mut inv, ItemKind::Bread, 2, None, Some(task_id)); // reserved

        assert_eq!(count_unowned_unreserved(&inv, ItemKind::Bread), 5);
    }

    #[test]
    fn count_unowned_unreserved_empty() {
        let inv: Vec<Item> = Vec::new();
        assert_eq!(count_unowned_unreserved(&inv, ItemKind::Bread), 0);
    }

    #[test]
    fn reserve_unowned_items_skips_owned() {
        let mut rng = GameRng::new(42);
        let owner = CreatureId::new(&mut rng);
        let task_id = TaskId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, Some(owner), None); // owned
        add_item(&mut inv, ItemKind::Bread, 5, None, None); // unowned

        let reserved = reserve_unowned_items(&mut inv, ItemKind::Bread, 4, task_id);
        assert_eq!(reserved, 4);
        // Owned stack unchanged.
        assert_eq!(count_owned(&inv, ItemKind::Bread, owner), 3);
        // 1 unowned unreserved remains.
        assert_eq!(count_unowned_unreserved(&inv, ItemKind::Bread), 1);
    }

    #[test]
    fn reserve_unowned_items_caps_at_available() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, None, None);

        let reserved = reserve_unowned_items(&mut inv, ItemKind::Bread, 10, task_id);
        assert_eq!(reserved, 2);
        assert_eq!(count_unowned_unreserved(&inv, ItemKind::Bread), 0);
    }

    #[test]
    fn reserve_unowned_items_splits_stack() {
        let mut rng = GameRng::new(42);
        let task_id = TaskId::new(&mut rng);

        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, None);

        let reserved = reserve_unowned_items(&mut inv, ItemKind::Bread, 3, task_id);
        assert_eq!(reserved, 3);
        assert_eq!(inv.len(), 2);
        assert_eq!(count_unowned_unreserved(&inv, ItemKind::Bread), 2);
        let reserved_count: u32 = inv
            .iter()
            .filter(|i| i.kind == ItemKind::Bread && i.reserved_by == Some(task_id))
            .map(|i| i.quantity)
            .sum();
        assert_eq!(reserved_count, 3);
    }
}
