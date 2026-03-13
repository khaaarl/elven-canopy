// Item type enum for the simulation.
//
// Provides `ItemKind` (the enum of distinct item types: Bread, Fruit, Bow,
// Arrow, Bowstring, extracted fruit components — Pulp, Husk, Seed,
// FruitFiber, FruitSap, FruitResin — processed products — Flour, Thread,
// Cord, Cloth — clothing — Tunic, Leggings, Boots, Hat, Gloves — and armor —
// Helmet, Breastplate, Greaves, Gauntlets),
// `Material` (wood species for crafted items, `FruitSpecies` for
// fruits, extracted components, and processed products),
// `MaterialFilter` (logistics want constraint: `Any` or `Specific(Material)`),
// `EquipSlot` (body slot for wearable clothing: Head, Torso, Legs, Feet, Hands),
// and `EffectKind` (stubbed enchantment effect types for future use).
// Item storage is now handled by the `db::ItemStack` and `db::Inventory`
// tabulosity tables. `SimState` has `inv_*` methods (in `sim.rs`) for all
// inventory operations (add, remove, count, reserve, equip, etc.).
//
// `MaterialFilter` is used by building logistics wants and creature personal
// wants to constrain which materials satisfy a request. `Any` matches all
// materials; `Specific(m)` matches only items with that exact material.
//
// `EquipSlot` maps clothing `ItemKind` variants to body slots via
// `ItemKind::equip_slot()`. Used by the `equipped_slot` field on
// `db::ItemStack` to track which items a creature is wearing.
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
    /// Sewn pair of gloves (hand garment).
    Gloves = 19,
    /// Grown wooden helmet (head armor).
    Helmet = 20,
    /// Grown wooden breastplate (torso armor).
    Breastplate = 21,
    /// Grown wooden greaves (leg armor).
    Greaves = 22,
    /// Grown wooden gauntlets (hand armor).
    Gauntlets = 23,
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
            ItemKind::Gloves => "Gloves",
            ItemKind::Helmet => "Helmet",
            ItemKind::Breastplate => "Breastplate",
            ItemKind::Greaves => "Greaves",
            ItemKind::Gauntlets => "Gauntlets",
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
            ItemKind::Tunic
                | ItemKind::Leggings
                | ItemKind::Boots
                | ItemKind::Hat
                | ItemKind::Gloves
        )
    }

    /// Which body slot this item occupies when equipped, if any.
    /// Both clothing and armor items map to slots — they compete for the
    /// same slot (e.g., Hat vs Helmet for Head).
    pub fn equip_slot(self) -> Option<EquipSlot> {
        match self {
            ItemKind::Hat | ItemKind::Helmet => Some(EquipSlot::Head),
            ItemKind::Tunic | ItemKind::Breastplate => Some(EquipSlot::Torso),
            ItemKind::Leggings | ItemKind::Greaves => Some(EquipSlot::Legs),
            ItemKind::Boots => Some(EquipSlot::Feet),
            ItemKind::Gloves | ItemKind::Gauntlets => Some(EquipSlot::Hands),
            _ => None,
        }
    }

    /// Whether this item kind is an armor piece.
    pub fn is_armor(self) -> bool {
        matches!(
            self,
            ItemKind::Helmet
                | ItemKind::Breastplate
                | ItemKind::Greaves
                | ItemKind::Gauntlets
                | ItemKind::Boots
        )
    }

    /// Flat damage reduction for this item kind when made from the given
    /// material. Returns 0 for non-armor items and for non-wood materials
    /// (e.g., cloth boots provide no protection). All wood types currently
    /// give the same values; material-specific differentiation is deferred.
    /// Quality will also factor in later (F-item-quality).
    ///
    /// Minimum damage per hit (1) is enforced by the combat system, not here.
    pub fn armor_value(self, material: Option<Material>) -> i32 {
        let is_wood = material.is_some_and(|m| m.is_wood());
        if !is_wood {
            return 0;
        }
        match self {
            ItemKind::Helmet => 2,
            ItemKind::Breastplate => 3,
            ItemKind::Greaves => 2,
            ItemKind::Gauntlets => 1,
            ItemKind::Boots => 1,
            _ => 0,
        }
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
    /// All wood-type materials. Used for generating per-wood-type recipes.
    pub const WOOD_TYPES: [Material; 5] = [
        Material::Oak,
        Material::Birch,
        Material::Willow,
        Material::Ash,
        Material::Yew,
    ];

    /// Whether this material is a wood type (as opposed to a fruit species).
    pub fn is_wood(self) -> bool {
        !matches!(self, Material::FruitSpecies(_))
    }

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
    /// Any material except wood types. Used for clothing wants so elves
    /// don't pick up wood boots (armor) when they want cloth boots (clothing).
    NonWood = 2,
    // Append new variants here with the next sequential number.
}

impl MaterialFilter {
    /// Does this filter accept an item with the given material?
    pub fn matches(self, material: Option<Material>) -> bool {
        match self {
            MaterialFilter::Any => true,
            MaterialFilter::Specific(m) => material == Some(m),
            MaterialFilter::NonWood => match material {
                Some(m) => !m.is_wood(),
                None => true,
            },
        }
    }
}

/// Body slot for wearable items (clothing and armor).
///
/// Each wearable `ItemKind` maps to exactly one slot via `ItemKind::equip_slot()`.
/// A creature can equip at most one item per slot. Clothing and armor compete
/// for the same slots (e.g., Hat vs Helmet for Head).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EquipSlot {
    Head = 0,
    Torso = 1,
    Legs = 2,
    Feet = 3,
    Hands = 4,
}

impl EquipSlot {
    /// Human-readable display name for this slot.
    pub fn display_name(self) -> &'static str {
        match self {
            EquipSlot::Head => "Head",
            EquipSlot::Torso => "Torso",
            EquipSlot::Legs => "Legs",
            EquipSlot::Feet => "Feet",
            EquipSlot::Hands => "Hands",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_armor_includes_armor_pieces() {
        assert!(ItemKind::Helmet.is_armor());
        assert!(ItemKind::Breastplate.is_armor());
        assert!(ItemKind::Greaves.is_armor());
        assert!(ItemKind::Gauntlets.is_armor());
        assert!(ItemKind::Boots.is_armor());
    }

    #[test]
    fn is_armor_excludes_non_armor() {
        assert!(!ItemKind::Tunic.is_armor());
        assert!(!ItemKind::Hat.is_armor());
        assert!(!ItemKind::Leggings.is_armor());
        assert!(!ItemKind::Bow.is_armor());
        assert!(!ItemKind::Bread.is_armor());
        assert!(!ItemKind::Gloves.is_armor());
    }

    #[test]
    fn boots_is_both_clothing_and_armor() {
        // Boots can be either — wood boots are armor, cloth boots are clothing.
        assert!(ItemKind::Boots.is_clothing());
        assert!(ItemKind::Boots.is_armor());
    }

    #[test]
    fn armor_value_wood_materials() {
        assert_eq!(ItemKind::Helmet.armor_value(Some(Material::Oak)), 2);
        assert_eq!(ItemKind::Breastplate.armor_value(Some(Material::Oak)), 3);
        assert_eq!(ItemKind::Greaves.armor_value(Some(Material::Oak)), 2);
        assert_eq!(ItemKind::Gauntlets.armor_value(Some(Material::Oak)), 1);
        assert_eq!(ItemKind::Boots.armor_value(Some(Material::Oak)), 1);
    }

    #[test]
    fn armor_value_all_wood_types_equal() {
        // All wood types give the same armor value for now.
        for wood in &Material::WOOD_TYPES {
            assert_eq!(
                ItemKind::Breastplate.armor_value(Some(*wood)),
                3,
                "{:?} breastplate should give 3 armor",
                wood
            );
        }
    }

    #[test]
    fn armor_value_non_wood_materials_zero() {
        let fruit_mat = Material::FruitSpecies(crate::fruit::FruitSpeciesId(0));
        assert_eq!(ItemKind::Boots.armor_value(Some(fruit_mat)), 0);
        assert_eq!(ItemKind::Helmet.armor_value(Some(fruit_mat)), 0);
    }

    #[test]
    fn armor_value_no_material_zero() {
        assert_eq!(ItemKind::Helmet.armor_value(None), 0);
        assert_eq!(ItemKind::Breastplate.armor_value(None), 0);
    }

    #[test]
    fn armor_value_non_armor_items_zero() {
        assert_eq!(ItemKind::Tunic.armor_value(Some(Material::Oak)), 0);
        assert_eq!(ItemKind::Hat.armor_value(Some(Material::Oak)), 0);
        assert_eq!(ItemKind::Bow.armor_value(Some(Material::Oak)), 0);
    }

    #[test]
    fn armor_value_total_full_set() {
        // Full armor set: helmet(2) + breastplate(3) + greaves(2) +
        // gauntlets(1) + boots(1) = 9.
        let wood = Some(Material::Oak);
        let total = ItemKind::Helmet.armor_value(wood)
            + ItemKind::Breastplate.armor_value(wood)
            + ItemKind::Greaves.armor_value(wood)
            + ItemKind::Gauntlets.armor_value(wood)
            + ItemKind::Boots.armor_value(wood);
        assert_eq!(total, 9);
    }

    #[test]
    fn material_is_wood() {
        assert!(Material::Oak.is_wood());
        assert!(Material::Birch.is_wood());
        assert!(Material::Willow.is_wood());
        assert!(Material::Ash.is_wood());
        assert!(Material::Yew.is_wood());
        assert!(!Material::FruitSpecies(crate::fruit::FruitSpeciesId(0)).is_wood());
    }

    #[test]
    fn wood_types_constant_complete() {
        assert_eq!(Material::WOOD_TYPES.len(), 5);
        for wood in &Material::WOOD_TYPES {
            assert!(wood.is_wood());
        }
    }

    #[test]
    fn equip_slot_armor_competes_with_clothing() {
        // Armor and clothing share the same equip slots.
        assert_eq!(ItemKind::Hat.equip_slot(), ItemKind::Helmet.equip_slot());
        assert_eq!(
            ItemKind::Tunic.equip_slot(),
            ItemKind::Breastplate.equip_slot()
        );
        assert_eq!(
            ItemKind::Leggings.equip_slot(),
            ItemKind::Greaves.equip_slot()
        );
        assert_eq!(
            ItemKind::Gloves.equip_slot(),
            ItemKind::Gauntlets.equip_slot()
        );
    }

    #[test]
    fn equip_slot_all_armor_has_slots() {
        assert!(ItemKind::Helmet.equip_slot().is_some());
        assert!(ItemKind::Breastplate.equip_slot().is_some());
        assert!(ItemKind::Greaves.equip_slot().is_some());
        assert!(ItemKind::Gauntlets.equip_slot().is_some());
    }

    #[test]
    fn material_filter_non_wood_rejects_wood() {
        for wood in &Material::WOOD_TYPES {
            assert!(
                !MaterialFilter::NonWood.matches(Some(*wood)),
                "{:?} should be rejected by NonWood filter",
                wood
            );
        }
    }

    #[test]
    fn material_filter_non_wood_accepts_non_wood() {
        let fruit_mat = Material::FruitSpecies(crate::fruit::FruitSpeciesId(0));
        assert!(MaterialFilter::NonWood.matches(Some(fruit_mat)));
        assert!(MaterialFilter::NonWood.matches(None));
    }

    #[test]
    fn material_filter_non_wood_serde_roundtrip() {
        let filter = MaterialFilter::NonWood;
        let json = serde_json::to_string(&filter).unwrap();
        let restored: MaterialFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(filter, restored);
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
