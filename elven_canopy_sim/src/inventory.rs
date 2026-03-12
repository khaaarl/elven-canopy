// Item type enum for the simulation.
//
// Provides `ItemKind` (the enum of distinct item types: Bread, Fruit, Bow,
// Arrow, Bowstring, extracted fruit components — Pulp, Husk, Seed,
// FruitFiber, FruitSap, FruitResin — processed products — Flour, Thread,
// Cord, Cloth — and clothing — Tunic, Leggings, Boots, Hat),
// `Material` (wood species for crafted items, `FruitSpecies` for
// fruits, extracted components, and processed products),
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
///
/// **STABLE ENUM:** Discriminant values are persisted in save files via
/// `RecipeKey`. Never reorder, never reuse a number. Append new variants
/// at the end with the next available discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum ItemKind {
    Bread = 0,
    Fruit = 1,
    Bow = 2,
    Arrow = 3,
    Bowstring = 4,
    /// Extracted fruit flesh (from PartType::Flesh).
    Pulp = 5,
    /// Extracted fruit rind/shell (from PartType::Rind).
    Husk = 6,
    /// Extracted fruit seed (from PartType::Seed).
    Seed = 7,
    /// Extracted fruit fiber (from PartType::Fiber).
    FruitFiber = 8,
    /// Extracted fruit sap (from PartType::Sap).
    FruitSap = 9,
    /// Extracted fruit resin (from PartType::Resin).
    FruitResin = 10,
    /// Milled flour from a starchy fruit component.
    Flour = 11,
    /// Spun thread from a fine-fibrous fruit component.
    Thread = 12,
    /// Twisted cord from a coarse-fibrous fruit component.
    Cord = 13,
    /// Woven cloth from fine thread.
    Cloth = 14,
    /// Sewn tunic (torso garment).
    Tunic = 15,
    /// Sewn leggings (leg garment).
    Leggings = 16,
    /// Sewn pair of boots (foot garment).
    Boots = 17,
    /// Sewn hat (head garment).
    Hat = 18,
    // Append new variants here with the next sequential number.
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
            ItemKind::Flour => "Flour",
            ItemKind::Thread => "Thread",
            ItemKind::Cord => "Cord",
            ItemKind::Cloth => "Cloth",
            ItemKind::Tunic => "Tunic",
            ItemKind::Leggings => "Leggings",
            ItemKind::Boots => "Boots",
            ItemKind::Hat => "Hat",
        }
    }

    /// Whether this item kind is an extracted fruit component.
    pub fn is_extracted_component(self) -> bool {
        matches!(
            self,
            ItemKind::Pulp
                | ItemKind::Husk
                | ItemKind::Seed
                | ItemKind::FruitFiber
                | ItemKind::FruitSap
                | ItemKind::FruitResin
        )
    }

    /// Whether this item kind is a processed fruit product (flour, thread, cord, cloth).
    pub fn is_processed_component(self) -> bool {
        matches!(
            self,
            ItemKind::Flour | ItemKind::Thread | ItemKind::Cord | ItemKind::Cloth
        )
    }

    /// Whether this item kind is a clothing garment.
    pub fn is_clothing(self) -> bool {
        matches!(
            self,
            ItemKind::Tunic | ItemKind::Leggings | ItemKind::Boots | ItemKind::Hat
        )
    }
}

/// Material variant for items. Optional — many items have `material: None`.
///
/// Wood types are used for crafted items. `FruitSpecies` identifies which
/// procedurally generated fruit species an item came from (see `fruit.rs`).
///
/// **STABLE ENUM:** Discriminant values are persisted in save files via
/// `RecipeKey`. Never reorder, never reuse a number. Append new variants
/// at the end with the next available discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum Material {
    Oak = 0,
    Birch = 1,
    Willow = 2,
    Ash = 3,
    Yew = 4,
    /// A procedurally generated fruit species, identified by worldgen ID.
    FruitSpecies(crate::fruit::FruitSpeciesId) = 5,
    // Append new variants here with the next sequential number.
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
///
/// **STABLE ENUM:** Discriminant values are persisted in save files via
/// `RecipeKey`. Never reorder, never reuse a number. Append new variants
/// at the end with the next available discriminant.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[repr(u16)]
pub enum MaterialFilter {
    /// Any material or no material. "Give me any Fruit."
    #[default]
    Any = 0,
    /// A specific material. "Give me Shinethúni Fruit."
    Specific(Material) = 1,
    // Append new variants here with the next sequential number.
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
