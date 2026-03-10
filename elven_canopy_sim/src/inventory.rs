// Item type enum for the simulation.
//
// Provides `ItemKind` (the enum of distinct item types: Bread, Fruit, Bow,
// Arrow, Bowstring), `Material` (wood species for crafted items),
// `MaterialFilter` (logistics want constraint: `Any` or `Specific(Material)`),
// and `EffectKind` (stubbed enchantment effect types for future use).
// Item storage is now handled by the `db::ItemStack` and `db::Inventory`
// tabulosity tables. `SimState` has `inv_*` methods (in `sim.rs`) for all
// inventory operations (add, remove, count, reserve, etc.).
//
// `MaterialFilter` is used by building logistics wants and creature personal
// wants to constrain which materials satisfy a request. `Any` matches all
// materials; `Specific(m)` matches only items with that exact material.
//
// See also: `db.rs` for the tabulosity table definitions, `sim.rs` for the
// `inv_*` methods on `SimState`, `building.rs` for `LogisticsWant` DTO.

use serde::{Deserialize, Serialize};

/// The kind of item. Each variant is a distinct item type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    Bread,
    Fruit,
    Bow,
    Arrow,
    Bowstring,
    // Extracted fruit components (produced by hulling/separating whole fruit).
    // Each corresponds to a `PartType` variant in `fruit.rs`.
    Pulp,
    Husk,
    Seed,
    FruitFiber,
    FruitSap,
    FruitResin,
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
            ItemKind::Pulp => "Pulp",
            ItemKind::Husk => "Husk",
            ItemKind::Seed => "Seed",
            ItemKind::FruitFiber => "Fiber",
            ItemKind::FruitSap => "Sap",
            ItemKind::FruitResin => "Resin",
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

/// Constrains which materials satisfy a logistics want.
///
/// `Any` matches any material (or no material). `Specific(m)` matches only
/// items with `material == Some(m)`. Derives `Default` (→ `Any`) for serde
/// backward compatibility, and `Ord` for use as a BTreeMap key (determinism).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum MaterialFilter {
    /// Any material or no material. "Give me any Fruit."
    #[default]
    Any,
    /// A specific material. "Give me Shinethúni Fruit."
    Specific(Material),
}

impl MaterialFilter {
    /// Does this filter accept an item with the given material?
    pub fn matches(self, material: Option<Material>) -> bool {
        match self {
            MaterialFilter::Any => true,
            MaterialFilter::Specific(m) => material == Some(m),
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
