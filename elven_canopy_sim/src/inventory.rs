// Item type enum for the simulation.
//
// Provides `ItemKind` (the enum of distinct item types: Bread, Fruit, Bow,
// Arrow, Bowstring), `Material` (wood species for crafted items), and
// `EffectKind` (stubbed enchantment effect types for future use).
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
    Bow,
    Arrow,
    Bowstring,
}

impl ItemKind {
    /// Human-readable display name for this item kind.
    pub fn display_name(self) -> &'static str {
        match self {
            ItemKind::Bread => "Bread",
            ItemKind::Fruit => "Fruit",
            ItemKind::Bow => "Bow",
            ItemKind::Arrow => "Arrow",
            ItemKind::Bowstring => "Bowstring",
        }
    }
}

/// Material variant for items. Optional — many items have `material: None`.
///
/// Wood types are used for crafted items. `FruitSpecies` identifies which
/// procedurally generated fruit species an item came from (see `fruit.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Material {
    Oak,
    Birch,
    Willow,
    Ash,
    Yew,
    /// A procedurally generated fruit species, identified by worldgen ID.
    FruitSpecies(crate::fruit::FruitSpeciesId),
}

impl Material {
    /// Human-readable display name for this material.
    pub fn display_name(self) -> &'static str {
        match self {
            Material::Oak => "Oak",
            Material::Birch => "Birch",
            Material::Willow => "Willow",
            Material::Ash => "Ash",
            Material::Yew => "Yew",
            Material::FruitSpecies(_) => "Fruit",
        }
    }
}

/// Kind of enchantment effect. Stubbed for future magic item system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EffectKind {
    Placeholder,
}

impl EffectKind {
    /// Human-readable display name for this effect kind.
    pub fn display_name(self) -> &'static str {
        match self {
            EffectKind::Placeholder => "Placeholder",
        }
    }
}
