// Item type enum for the simulation.
//
// Provides `ItemKind`, the enum of distinct item types (Bread, Fruit, etc.).
// Item storage is now handled by the `db::ItemStack` and `db::Inventory`
// tabulosity tables. `SimState` has `inv_*` methods (in `sim.rs`) for all
// inventory operations (add, remove, count, reserve, etc.).
//
// See also: `db.rs` for the tabulosity table definitions, `sim.rs` for the
// `inv_*` methods on `SimState`.

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
