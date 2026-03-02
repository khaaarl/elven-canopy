// Item and inventory data model.
//
// Provides `ItemKind`, `Item`, and `GroundPile` types plus helper functions
// for manipulating inventories (add, remove, count). Inventories are `Vec<Item>`
// stored on `Creature` (in `sim.rs`), `CompletedStructure` (in `building.rs`),
// and as `GroundPile`s in `SimState.ground_piles`.
//
// Items stack: adding bread to an inventory that already has bread increases
// the existing stack's quantity rather than creating a duplicate entry.
// Stacking matches on `(kind, owner, task_related)`.
//
// See also: `sim.rs` for creature inventories, `building.rs` for structure
// inventories, `sim_bridge.rs` for the GDExtension interface that exposes
// inventories to GDScript.
//
// **Critical constraint: determinism.** All operations are pure functions over
// `Vec<Item>` — no hash-based collections, no OS calls.

use crate::types::{CreatureId, VoxelCoord};
use serde::{Deserialize, Serialize};

/// The kind of item. Each variant is a distinct item type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    Bread,
}

impl ItemKind {
    /// Human-readable display name for this item kind.
    pub fn display_name(self) -> &'static str {
        match self {
            ItemKind::Bread => "Bread",
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
    /// Whether this item is reserved for a task. Always false for now.
    pub task_related: bool,
}

/// A pile of items on the ground at a specific voxel position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundPile {
    pub position: VoxelCoord,
    pub items: Vec<Item>,
}

/// Add items to an inventory, stacking with existing items that match
/// `(kind, owner, task_related)`. Returns the resulting quantity of
/// that item kind in the stack.
pub fn add_item(
    inventory: &mut Vec<Item>,
    kind: ItemKind,
    quantity: u32,
    owner: Option<CreatureId>,
    task_related: bool,
) -> u32 {
    for item in inventory.iter_mut() {
        if item.kind == kind && item.owner == owner && item.task_related == task_related {
            item.quantity += quantity;
            return item.quantity;
        }
    }
    inventory.push(Item {
        kind,
        quantity,
        owner,
        task_related,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::GameRng;
    use crate::types::CreatureId;

    #[test]
    fn add_creates_new_stack() {
        let mut inv = Vec::new();
        let result = add_item(&mut inv, ItemKind::Bread, 3, None, false);
        assert_eq!(result, 3);
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].kind, ItemKind::Bread);
        assert_eq!(inv[0].quantity, 3);
    }

    #[test]
    fn add_stacks_matching_items() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, None, false);
        let result = add_item(&mut inv, ItemKind::Bread, 5, None, false);
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
        add_item(&mut inv, ItemKind::Bread, 2, Some(owner_a), false);
        add_item(&mut inv, ItemKind::Bread, 3, Some(owner_b), false);
        assert_eq!(inv.len(), 2);
        assert_eq!(inv[0].quantity, 2);
        assert_eq!(inv[1].quantity, 3);
    }

    #[test]
    fn add_different_task_related_creates_separate_stack() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, None, false);
        add_item(&mut inv, ItemKind::Bread, 3, None, true);
        assert_eq!(inv.len(), 2);
    }

    #[test]
    fn remove_decreases_quantity() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 5, None, false);
        let removed = remove_item(&mut inv, ItemKind::Bread, 3);
        assert_eq!(removed, 3);
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].quantity, 2);
    }

    #[test]
    fn remove_drops_empty_stack() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 3, None, false);
        let removed = remove_item(&mut inv, ItemKind::Bread, 3);
        assert_eq!(removed, 3);
        assert!(inv.is_empty());
    }

    #[test]
    fn remove_caps_at_available() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 2, None, false);
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
        add_item(&mut inv, ItemKind::Bread, 3, None, false);
        add_item(&mut inv, ItemKind::Bread, 2, Some(owner), false);
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
    }

    #[test]
    fn serialization_roundtrip() {
        let mut inv = Vec::new();
        add_item(&mut inv, ItemKind::Bread, 7, None, false);

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
                task_related: false,
            }],
        };

        let json = serde_json::to_string(&pile).unwrap();
        let restored: GroundPile = serde_json::from_str(&json).unwrap();
        assert_eq!(pile, restored);
    }
}
