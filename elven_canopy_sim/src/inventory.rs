// Item types, materials, and color system for the simulation.
//
// Provides `ItemKind` (the enum of distinct item types: Bread, Fruit, Bow,
// Arrow, Bowstring, extracted fruit components — Pulp, Husk, Seed,
// FruitFiber, FruitSap, FruitResin — processed products — Flour, Thread,
// Cord, Cloth, Dye — clothing — Tunic, Leggings, Sandals, Shoes, Hat,
// Gloves — armor — Helmet, Breastplate, Greaves, Gauntlets, Boots),
// `Material` (wood species for crafted items, `FruitSpecies` for
// fruits, extracted components, and processed products),
// `MaterialFilter` (logistics want constraint: `Any` or `Specific(Material)`),
// `ArmorParams` + `effective_armor_value()` (condition-aware armor damage
// reduction — the public API used by combat code in `sim/combat.rs`),
// `EquipSlot` (body slot for wearable items: Head, Torso, Legs, Feet, Hands),
// `ItemColor` (RGB color for items, derived from material or dye),
// and `EffectKind` (stubbed enchantment effect types for future use).
// Item storage is now handled by the `db::ItemStack` and `db::Inventory`
// tabulosity tables. `SimState` has `inv_*` methods (in `sim/inventory_mgmt.rs`) for all
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
// See also: `db.rs` for the tabulosity table definitions, `sim/inventory_mgmt.rs` for the
// `inv_*` methods on `SimState`, `building.rs` for `LogisticsWant` DTO.

use serde::{Deserialize, Serialize};

/// The kind of item. Each variant is a distinct item type.
///
/// **STABLE ENUM:** Discriminant values are persisted in save files via
/// `Recipe` enum. Never reorder, never reuse a number. Append new variants
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
    /// Grown wooden boots (foot armor).
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
    /// Pressed dye from a pigmented fruit component.
    /// Color is stored in `dye_color` on the `ItemStack`, not in the variant.
    Dye = 24,
    /// Grown wooden spear (melee weapon with extended reach).
    Spear = 25,
    /// Grown wooden club (melee weapon with high damage).
    Club = 26,
    /// Sewn pair of sandals (light civilian footwear).
    Sandals = 27,
    /// Sewn pair of shoes (standard civilian footwear).
    Shoes = 28,
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
            ItemKind::Dye => "Dye",
            ItemKind::Spear => "Spear",
            ItemKind::Club => "Club",
            ItemKind::Sandals => "Sandals",
            ItemKind::Shoes => "Shoes",
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
    /// Dye is excluded — it uses a separate crafting path (Press) and carries
    /// a `dye_color` rather than being differentiated purely by material.
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
                | ItemKind::Sandals
                | ItemKind::Shoes
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
            ItemKind::Boots | ItemKind::Sandals | ItemKind::Shoes => Some(EquipSlot::Feet),
            ItemKind::Gloves | ItemKind::Gauntlets => Some(EquipSlot::Hands),
            _ => None,
        }
    }

    /// Whether this item kind is a melee weapon (spear, club).
    pub fn is_melee_weapon(self) -> bool {
        matches!(self, ItemKind::Spear | ItemKind::Club)
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

    /// Base (unconditioned) damage reduction for this item kind when made
    /// from the given material. Returns 0 for non-armor items and for
    /// non-wood materials (only wood grants armor protection). All
    /// wood types currently give the same values; material-specific
    /// differentiation is deferred.
    ///
    /// **Internal helper** — combat code should call `effective_armor_value()`
    /// on an `ItemStack` instead, which applies condition penalties and any
    /// future modifiers (enchantments, quality, etc.).
    pub fn base_armor_value(self, material: Option<Material>) -> i32 {
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
/// `Recipe` enum. Never reorder, never reuse a number. Append new variants
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
/// `Recipe` enum. Never reorder, never reuse a number. Append new variants
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
    /// Any material except wood types. Originally used for the boots want
    /// workaround (before boots were split into Sandals/Shoes/Boots); kept
    /// for general use and serde backward compatibility.
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

/// Durability condition category for an item, derived from its HP ratio
/// and the game config thresholds (`durability_worn_pct`, `durability_damaged_pct`).
///
/// Used by the sprite system (`CreatureDrawInfo`) to fingerprint equipment
/// appearance, and by `SimState::condition_label` for display strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WearCategory {
    /// HP above the worn threshold (or full/indestructible).
    Good,
    /// HP at or below the worn threshold but above the damaged threshold.
    Worn,
    /// HP at or below the damaged threshold.
    Damaged,
}

impl WearCategory {
    /// Compute the wear category from HP values and config thresholds.
    /// Items with `max_hp <= 0` or `current_hp >= max_hp` are `Good`.
    pub fn from_hp(current_hp: i32, max_hp: i32, worn_pct: i32, damaged_pct: i32) -> Self {
        if max_hp <= 0 || current_hp >= max_hp {
            return WearCategory::Good;
        }
        let ratio = current_hp * 100 / max_hp;
        if ratio <= damaged_pct {
            WearCategory::Damaged
        } else if ratio <= worn_pct {
            WearCategory::Worn
        } else {
            WearCategory::Good
        }
    }
}

/// Config parameters needed by `effective_armor_value()`. Extracted from
/// `GameConfig` to keep the function signature small and extensible.
#[derive(Clone, Copy, Debug)]
pub struct ArmorParams {
    pub worn_pct: i32,
    pub damaged_pct: i32,
    pub worn_penalty: i32,
    pub damaged_penalty: i32,
}

/// Compute the effective armor value for an equipped item, accounting for
/// the item's condition (wear category). This is the public API for combat
/// damage reduction — it wraps `ItemKind::base_armor_value()` and applies
/// condition-based penalties.
///
/// - `worn_penalty`: subtracted from base value when item is `Worn` (default 1)
/// - `damaged_penalty`: subtracted from base value when item is `Damaged` (default 2)
/// - Floors: `Worn` items floor at 2, `Damaged` items floor at 1.
/// - `Good` condition: no penalty.
///
/// Future modifiers (enchantments, quality, material bonuses) will be added
/// here behind this same interface.
pub fn effective_armor_value(
    kind: ItemKind,
    material: Option<Material>,
    current_hp: i32,
    max_hp: i32,
    params: ArmorParams,
) -> i32 {
    let base = kind.base_armor_value(material);
    if base <= 0 {
        return 0;
    }
    let category = WearCategory::from_hp(current_hp, max_hp, params.worn_pct, params.damaged_pct);
    match category {
        WearCategory::Good => base,
        WearCategory::Worn => (base - params.worn_penalty).max(2).min(base),
        WearCategory::Damaged => (base - params.damaged_penalty).max(1).min(base),
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
    /// Number of equip slots (for fixed-size arrays indexed by slot).
    pub const COUNT: usize = 5;

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
    fn wear_category_good_at_full_hp() {
        assert_eq!(WearCategory::from_hp(100, 100, 70, 40), WearCategory::Good);
    }

    #[test]
    fn wear_category_good_above_worn_threshold() {
        assert_eq!(WearCategory::from_hp(71, 100, 70, 40), WearCategory::Good);
    }

    #[test]
    fn wear_category_worn_at_threshold() {
        assert_eq!(WearCategory::from_hp(70, 100, 70, 40), WearCategory::Worn);
    }

    #[test]
    fn wear_category_worn_between_thresholds() {
        assert_eq!(WearCategory::from_hp(50, 100, 70, 40), WearCategory::Worn);
    }

    #[test]
    fn wear_category_damaged_at_threshold() {
        assert_eq!(
            WearCategory::from_hp(40, 100, 70, 40),
            WearCategory::Damaged
        );
    }

    #[test]
    fn wear_category_damaged_below_threshold() {
        assert_eq!(
            WearCategory::from_hp(10, 100, 70, 40),
            WearCategory::Damaged
        );
    }

    #[test]
    fn wear_category_zero_hp_positive_max_is_damaged() {
        assert_eq!(WearCategory::from_hp(0, 100, 70, 40), WearCategory::Damaged);
    }

    #[test]
    fn wear_category_indestructible_is_good() {
        assert_eq!(WearCategory::from_hp(0, 0, 70, 40), WearCategory::Good);
        assert_eq!(WearCategory::from_hp(0, -1, 70, 40), WearCategory::Good);
    }

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
        assert!(!ItemKind::Sandals.is_armor());
        assert!(!ItemKind::Shoes.is_armor());
    }

    #[test]
    fn boots_is_armor_only() {
        // Boots are armor only — civilian footwear is Sandals/Shoes.
        assert!(!ItemKind::Boots.is_clothing());
        assert!(ItemKind::Boots.is_armor());
    }

    #[test]
    fn sandals_and_shoes_are_clothing_only() {
        assert!(ItemKind::Sandals.is_clothing());
        assert!(!ItemKind::Sandals.is_armor());
        assert!(ItemKind::Shoes.is_clothing());
        assert!(!ItemKind::Shoes.is_armor());
    }

    #[test]
    fn sandals_and_shoes_equip_to_feet() {
        assert_eq!(ItemKind::Sandals.equip_slot(), Some(EquipSlot::Feet));
        assert_eq!(ItemKind::Shoes.equip_slot(), Some(EquipSlot::Feet));
    }

    #[test]
    fn sandals_and_shoes_have_no_armor_value() {
        assert_eq!(ItemKind::Sandals.base_armor_value(Some(Material::Oak)), 0);
        assert_eq!(ItemKind::Shoes.base_armor_value(Some(Material::Oak)), 0);
    }

    #[test]
    fn base_armor_value_wood_materials() {
        assert_eq!(ItemKind::Helmet.base_armor_value(Some(Material::Oak)), 2);
        assert_eq!(
            ItemKind::Breastplate.base_armor_value(Some(Material::Oak)),
            3
        );
        assert_eq!(ItemKind::Greaves.base_armor_value(Some(Material::Oak)), 2);
        assert_eq!(ItemKind::Gauntlets.base_armor_value(Some(Material::Oak)), 1);
        assert_eq!(ItemKind::Boots.base_armor_value(Some(Material::Oak)), 1);
    }

    #[test]
    fn base_armor_value_all_wood_types_equal() {
        // All wood types give the same armor value for now.
        for wood in &Material::WOOD_TYPES {
            assert_eq!(
                ItemKind::Breastplate.base_armor_value(Some(*wood)),
                3,
                "{:?} breastplate should give 3 armor",
                wood
            );
        }
    }

    #[test]
    fn base_armor_value_non_wood_materials_zero() {
        let fruit_mat = Material::FruitSpecies(crate::fruit::FruitSpeciesId(0));
        assert_eq!(ItemKind::Boots.base_armor_value(Some(fruit_mat)), 0);
        assert_eq!(ItemKind::Helmet.base_armor_value(Some(fruit_mat)), 0);
    }

    #[test]
    fn base_armor_value_no_material_zero() {
        assert_eq!(ItemKind::Helmet.base_armor_value(None), 0);
        assert_eq!(ItemKind::Breastplate.base_armor_value(None), 0);
    }

    #[test]
    fn base_armor_value_non_armor_items_zero() {
        assert_eq!(ItemKind::Tunic.base_armor_value(Some(Material::Oak)), 0);
        assert_eq!(ItemKind::Hat.base_armor_value(Some(Material::Oak)), 0);
        assert_eq!(ItemKind::Bow.base_armor_value(Some(Material::Oak)), 0);
    }

    #[test]
    fn base_armor_value_total_full_set() {
        // Full armor set: helmet(2) + breastplate(3) + greaves(2) +
        // gauntlets(1) + boots(1) = 9.
        let wood = Some(Material::Oak);
        let total = ItemKind::Helmet.base_armor_value(wood)
            + ItemKind::Breastplate.base_armor_value(wood)
            + ItemKind::Greaves.base_armor_value(wood)
            + ItemKind::Gauntlets.base_armor_value(wood)
            + ItemKind::Boots.base_armor_value(wood);
        assert_eq!(total, 9);
    }

    // -- effective_armor_value tests --

    const TEST_ARMOR_PARAMS: ArmorParams = ArmorParams {
        worn_pct: 70,
        damaged_pct: 40,
        worn_penalty: 1,
        damaged_penalty: 2,
    };

    #[test]
    fn effective_armor_value_good_condition_no_penalty() {
        // Full HP breastplate: base 3, no penalty.
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Breastplate, oak, 60, 60, TEST_ARMOR_PARAMS),
            3
        );
    }

    #[test]
    fn effective_armor_value_worn_condition_minus_one() {
        // Worn breastplate (70% HP): base 3, -1 penalty = 2, floor 2.
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Breastplate, oak, 42, 60, TEST_ARMOR_PARAMS),
            2
        );
    }

    #[test]
    fn effective_armor_value_damaged_condition_minus_two() {
        // Damaged breastplate (40% HP): base 3, -2 penalty = 1, floor 1.
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Breastplate, oak, 24, 60, TEST_ARMOR_PARAMS),
            1
        );
    }

    #[test]
    fn effective_armor_value_worn_gauntlets_floors_at_two() {
        // Worn gauntlets: base 1, -1 = 0, but floor at 2 → clamped to base (1).
        // The .min(base) clamp ensures we never exceed base.
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Gauntlets, oak, 42, 60, TEST_ARMOR_PARAMS),
            1
        );
    }

    #[test]
    fn effective_armor_value_damaged_gauntlets_floors_at_one() {
        // Damaged gauntlets: base 1, -2 = -1, but floor at 1.
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Gauntlets, oak, 24, 60, TEST_ARMOR_PARAMS),
            1
        );
    }

    #[test]
    fn effective_armor_value_damaged_helmet_floors_at_one() {
        // Damaged helmet: base 2, -2 = 0, but floor at 1.
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Helmet, oak, 24, 60, TEST_ARMOR_PARAMS),
            1
        );
    }

    #[test]
    fn effective_armor_value_non_armor_returns_zero() {
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Tunic, oak, 30, 30, TEST_ARMOR_PARAMS),
            0
        );
    }

    #[test]
    fn effective_armor_value_fruit_material_returns_zero() {
        let fruit = Material::FruitSpecies(crate::fruit::FruitSpeciesId(0));
        assert_eq!(
            effective_armor_value(
                ItemKind::Breastplate,
                Some(fruit),
                60,
                60,
                TEST_ARMOR_PARAMS
            ),
            0
        );
    }

    #[test]
    fn effective_armor_value_indestructible_is_good() {
        // Indestructible item (0/0): treated as Good condition.
        let oak = Some(Material::Oak);
        assert_eq!(
            effective_armor_value(ItemKind::Breastplate, oak, 0, 0, TEST_ARMOR_PARAMS),
            3
        );
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

/// An RGB color for items (material-derived or dye-applied).
///
/// Used by `item_color()` to resolve an item's visual color. Undyed items
/// derive a muted color from their material; dyed items use the dye color
/// directly. See also: `FruitColor` in `fruit.rs` (converted via `From`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ItemColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ItemColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Human-readable color name based on the dominant hue.
    /// Used in item display names for dyed items (e.g., "Red Oak Breastplate").
    pub fn display_name(self) -> &'static str {
        let (r, g, b) = (self.r as u16, self.g as u16, self.b as u16);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lum = (max + min) / 2;

        // Very dark or very light → achromatic names.
        if max <= 40 {
            return "Black";
        }
        if min >= 210 {
            return "White";
        }
        // Low saturation → grey.
        if max - min < 30 {
            return if lum > 160 { "Light Grey" } else { "Grey" };
        }

        // Chromatic: find dominant hue.
        if r >= g && r >= b {
            if r - g < 30 && b < r / 2 {
                "Yellow"
            } else if r - b < 30 && g < r / 2 {
                "Purple"
            } else if g > b + 20 {
                "Orange"
            } else {
                "Red"
            }
        } else if g >= r && g >= b {
            if g - r < 30 && b < g / 2 {
                "Yellow"
            } else if g - b < 30 && r < g / 2 {
                "Teal"
            } else {
                "Green"
            }
        } else {
            // b is dominant
            if b - r <= 40 && g < b / 2 {
                "Purple"
            } else if b - g < 30 && r < b / 2 {
                "Teal"
            } else {
                "Blue"
            }
        }
    }

    /// Desaturate and darken a color to produce a muted variant.
    /// Used for material-derived colors when no explicit dye is applied.
    pub fn muted(self) -> Self {
        // Convert to approximate luminance, then blend toward grey and darken.
        let grey = ((self.r as u16 * 77 + self.g as u16 * 150 + self.b as u16 * 29) >> 8) as u8;
        // 60% original color + 40% grey, then darken by ~15%.
        let mix = |c: u8| -> u8 {
            let blended = (c as u16 * 60 + grey as u16 * 40) / 100;
            (blended * 85 / 100).min(255) as u8
        };
        Self {
            r: mix(self.r),
            g: mix(self.g),
            b: mix(self.b),
        }
    }
}

impl From<crate::fruit::FruitColor> for ItemColor {
    fn from(fc: crate::fruit::FruitColor) -> Self {
        Self {
            r: fc.r,
            g: fc.g,
            b: fc.b,
        }
    }
}

impl Material {
    /// Base color associated with this material. For wood types, returns a
    /// characteristic wood color. For fruit species, returns a generic
    /// fruit-brown — callers should use the fruit's `FruitAppearance` color
    /// instead when available.
    pub fn base_color(self) -> ItemColor {
        match self {
            Material::Oak => ItemColor::new(160, 120, 60),
            Material::Birch => ItemColor::new(220, 210, 180),
            Material::Willow => ItemColor::new(140, 160, 100),
            Material::Ash => ItemColor::new(200, 190, 160),
            Material::Yew => ItemColor::new(180, 100, 60),
            // Generic fallback — callers override with actual fruit color.
            Material::FruitSpecies(_) => ItemColor::new(170, 130, 90),
        }
    }
}

/// Default color for items with no material (neutral grey-tan).
pub const DEFAULT_ITEM_COLOR: ItemColor = ItemColor::new(160, 150, 140);

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

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn item_color_new() {
        let c = ItemColor::new(100, 200, 50);
        assert_eq!(c.r, 100);
        assert_eq!(c.g, 200);
        assert_eq!(c.b, 50);
    }

    #[test]
    fn item_color_muted_is_darker_and_less_saturated() {
        let bright = ItemColor::new(200, 50, 50); // saturated red
        let muted = bright.muted();
        // Muted version should be darker overall.
        let bright_sum = bright.r as u16 + bright.g as u16 + bright.b as u16;
        let muted_sum = muted.r as u16 + muted.g as u16 + muted.b as u16;
        assert!(
            muted_sum < bright_sum,
            "muted ({muted_sum}) should be darker than original ({bright_sum})"
        );
        // Muted version should be less saturated (channels closer together).
        let bright_range =
            bright.r.max(bright.g).max(bright.b) - bright.r.min(bright.g).min(bright.b);
        let muted_range = muted.r.max(muted.g).max(muted.b) - muted.r.min(muted.g).min(muted.b);
        assert!(
            muted_range < bright_range,
            "muted range ({muted_range}) should be less than original ({bright_range})"
        );
    }

    #[test]
    fn item_color_muted_extreme_values() {
        // White mutes to a dimmed grey — verify it doesn't wrap or panic.
        let white = ItemColor::new(255, 255, 255);
        let muted = white.muted();
        assert!(muted.r < 255, "muted white should be dimmer");
        assert_eq!(muted.r, muted.g);
        assert_eq!(muted.g, muted.b);

        // Black stays black.
        let black = ItemColor::new(0, 0, 0);
        let muted_black = black.muted();
        assert_eq!(muted_black.r, 0);
        assert_eq!(muted_black.g, 0);
        assert_eq!(muted_black.b, 0);
    }

    #[test]
    fn item_color_from_fruit_color() {
        let fc = crate::fruit::FruitColor {
            r: 100,
            g: 150,
            b: 200,
        };
        let ic = ItemColor::from(fc);
        assert_eq!(ic.r, 100);
        assert_eq!(ic.g, 150);
        assert_eq!(ic.b, 200);
    }

    #[test]
    fn material_base_color_all_woods_distinct() {
        let colors: Vec<ItemColor> = Material::WOOD_TYPES
            .iter()
            .map(|m| m.base_color())
            .collect();
        // Each wood type should have a unique color.
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(
                    colors[i],
                    colors[j],
                    "{:?} and {:?} should have different base colors",
                    Material::WOOD_TYPES[i],
                    Material::WOOD_TYPES[j]
                );
            }
        }
    }

    #[test]
    fn material_base_color_fruit_species_returns_fallback() {
        let fc = Material::FruitSpecies(crate::fruit::FruitSpeciesId(99)).base_color();
        // Should return the generic fruit-brown fallback.
        assert_eq!(fc, ItemColor::new(170, 130, 90));
    }

    #[test]
    fn default_item_color_is_neutral() {
        // Grey-tan, none of the channels should be extremely bright or dark.
        assert!(DEFAULT_ITEM_COLOR.r > 100 && DEFAULT_ITEM_COLOR.r < 200);
        assert!(DEFAULT_ITEM_COLOR.g > 100 && DEFAULT_ITEM_COLOR.g < 200);
        assert!(DEFAULT_ITEM_COLOR.b > 100 && DEFAULT_ITEM_COLOR.b < 200);
    }

    #[test]
    fn item_color_display_name_basic_hues() {
        assert_eq!(ItemColor::new(200, 30, 30).display_name(), "Red");
        assert_eq!(ItemColor::new(30, 180, 30).display_name(), "Green");
        assert_eq!(ItemColor::new(30, 30, 200).display_name(), "Blue");
        assert_eq!(ItemColor::new(200, 180, 30).display_name(), "Yellow");
        assert_eq!(ItemColor::new(200, 100, 30).display_name(), "Orange");
        assert_eq!(ItemColor::new(130, 50, 160).display_name(), "Purple");
    }

    #[test]
    fn item_color_display_name_achromatic() {
        assert_eq!(ItemColor::new(10, 10, 10).display_name(), "Black");
        assert_eq!(ItemColor::new(240, 240, 240).display_name(), "White");
        assert_eq!(ItemColor::new(130, 130, 130).display_name(), "Grey");
        assert_eq!(ItemColor::new(190, 190, 190).display_name(), "Light Grey");
    }

    #[test]
    fn item_color_serde_roundtrip() {
        let color = ItemColor::new(42, 128, 200);
        let json = serde_json::to_string(&color).unwrap();
        let restored: ItemColor = serde_json::from_str(&json).unwrap();
        assert_eq!(color, restored);
    }

    #[test]
    fn item_color_optional_serde_roundtrip() {
        // None case (for dye_color field on ItemStack).
        let none_color: Option<ItemColor> = None;
        let json = serde_json::to_string(&none_color).unwrap();
        let restored: Option<ItemColor> = serde_json::from_str(&json).unwrap();
        assert_eq!(none_color, restored);

        // Some case.
        let some_color = Some(ItemColor::new(255, 0, 128));
        let json = serde_json::to_string(&some_color).unwrap();
        let restored: Option<ItemColor> = serde_json::from_str(&json).unwrap();
        assert_eq!(some_color, restored);
    }
}
