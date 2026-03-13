// Unified recipe catalog for data-driven crafting.
//
// Replaces the per-building-type crafting systems (kitchen bread baking,
// workshop recipe list) with a single unified model. Recipes are identified
// structurally by `RecipeKey` (verb + sorted inputs/outputs), not by string
// names. The `RecipeCatalog` is built at startup from config recipes,
// dynamically generated fruit recipes (extraction, component processing —
// flour, thread, cord, cloth — and clothing — tunic, leggings, boots, hat),
// and per-wood-type Grow recipes (bows, arrows, armor — helmet, breastplate,
// greaves, gauntlets, boots), then stored immutably on `SimState`.
//
// Key types:
// - `RecipeVerb` — stable enum distinguishing crafting methods (Cook, Assemble,
//   etc.). Part of `RecipeKey` serialization contract.
// - `RecipeKey` — structural identity for a recipe. Two recipes with identical
//   keys are the same recipe. Serialized as deterministic JSON.
// - `RecipeDef` — full recipe definition with display name, category, inputs,
//   outputs, work ticks, and building type constraints.
// - `RecipeCatalog` — immutable BTreeMap of all recipes, keyed by `RecipeKey`.
//
// GDScript receives `RecipeKey` as a JSON string and treats it as opaque —
// never parsed or constructed on the GDScript side.
//
// See also: `config.rs` for `RecipeInput`/`RecipeOutput` structs,
// `db.rs` for `ActiveRecipe`/`ActiveRecipeTarget` tables,
// `command.rs` for crafting commands, `sim.rs` for the crafting monitor.

use crate::config::{RecipeInput, RecipeOutput, RecipeSubcomponentRecord};
use crate::inventory::{ItemKind, Material, MaterialFilter};
use crate::types::{FurnishingType, Species};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// RecipeVerb
// ---------------------------------------------------------------------------

/// Distinguishes fundamentally different crafting processes that happen to
/// share the same inputs and outputs (e.g., "husk fruit" vs "press fruit").
///
/// **STABLE ENUM:** Discriminant values are persisted in save files via
/// `RecipeKey`. Never reorder, never reuse a number. Append new variants
/// at the end with the next available discriminant. Comment out removed
/// variants (do not delete) to prevent accidental reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum RecipeVerb {
    Assemble = 0,
    Brew = 1,
    Cook = 2,
    Extract = 3,
    Fletch = 4,
    Husk = 5,
    Mill = 6,
    Spin = 7,
    Twist = 8,
    Bake = 9,
    /// Weave thread into cloth on a loom.
    Weave = 10,
    /// Sew cloth into a garment.
    Sew = 11,
    /// Grow an item from the home tree's wood at a workshop.
    Grow = 12,
    // Append new variants here with the next sequential number.
}

// ---------------------------------------------------------------------------
// RecipeKey
// ---------------------------------------------------------------------------

/// Structural identity for a recipe. Two recipes with identical keys are the
/// same recipe, regardless of display name changes.
///
/// Input and output Vecs MUST be sorted in canonical order (derived `Ord`)
/// to ensure identical recipes produce identical keys regardless of definition
/// order. The catalog builder enforces this at construction time.
///
/// **Serialization contract:** Field declaration order is stable — reordering
/// fields would change the JSON representation and orphan all saved keys.
/// Serde JSON serializes enum variants by name, so don't rename enum variants.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RecipeKey {
    // STABLE FIELD ORDER — do not reorder. See doc comment above.
    pub verb: RecipeVerb,
    pub inputs: Vec<(ItemKind, MaterialFilter, u32)>,
    pub outputs: Vec<(ItemKind, Option<Material>, u32)>,
}

impl RecipeKey {
    /// Serialize this key to a deterministic JSON string.
    ///
    /// Used as the opaque key representation passed to GDScript and stored in
    /// save files alongside `ActiveRecipe` rows.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("RecipeKey should always serialize")
    }

    /// Deserialize a key from its JSON representation.
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

// ---------------------------------------------------------------------------
// RecipeDef
// ---------------------------------------------------------------------------

/// Full definition of a recipe. Stored in the `RecipeCatalog`.
///
/// The `key` field is the structural identity. `inputs` and `outputs` are the
/// full structs with all metadata (quality, etc.), while `RecipeKey` contains
/// only the identity-relevant subset.
#[derive(Clone, Debug)]
pub struct RecipeDef {
    pub key: RecipeKey,
    pub display_name: String,
    /// Category path for hierarchical browsing, e.g. `["Brewing", "Cordials"]`.
    /// Empty vec = root level (no nesting).
    pub category: Vec<String>,
    /// Which building furnishing types can use this recipe.
    pub furnishing_types: Vec<FurnishingType>,
    pub inputs: Vec<RecipeInput>,
    pub outputs: Vec<RecipeOutput>,
    pub work_ticks: u64,
    pub subcomponent_records: Vec<RecipeSubcomponentRecord>,
    /// Species restriction. `None` = any species can craft.
    pub required_species: Option<Species>,
    /// Whether this recipe is auto-added to buildings when they are furnished.
    /// Config recipes (bread, weapons) are true; dynamic extraction recipes are
    /// false (user adds them manually from the available catalog).
    pub auto_add_on_furnish: bool,
}

// ---------------------------------------------------------------------------
// RecipeCatalog
// ---------------------------------------------------------------------------

/// Immutable catalog of all recipes, built at game startup.
///
/// Keyed by `RecipeKey` for O(log n) lookups. Iteration order is the canonical
/// order: config recipes first (in config Vec order), then dynamically generated
/// fruit-variety recipes (ordered by FruitSpeciesId, then by verb). The
/// `BTreeMap` ordering matches this because keys are constructed in that order
/// and `RecipeKey`'s derived `Ord` produces a consistent sort.
#[derive(Clone, Debug, Default)]
pub struct RecipeCatalog {
    recipes: BTreeMap<RecipeKey, RecipeDef>,
}

impl RecipeCatalog {
    /// Look up a recipe by its structural key.
    pub fn get(&self, key: &RecipeKey) -> Option<&RecipeDef> {
        self.recipes.get(key)
    }

    /// Iterate all recipes in canonical order.
    pub fn iter(&self) -> impl Iterator<Item = (&RecipeKey, &RecipeDef)> {
        self.recipes.iter()
    }

    /// Number of recipes in the catalog.
    pub fn len(&self) -> usize {
        self.recipes.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }

    /// Get all recipes available for a given furnishing type.
    pub fn recipes_for_furnishing(&self, ft: FurnishingType) -> Vec<&RecipeDef> {
        self.recipes
            .values()
            .filter(|r| r.furnishing_types.contains(&ft))
            .collect()
    }

    /// Get recipes that should be auto-added when a building is furnished.
    /// Filters by furnishing type AND `auto_add_on_furnish == true`.
    pub fn default_recipes_for_furnishing(&self, ft: FurnishingType) -> Vec<&RecipeDef> {
        self.recipes
            .values()
            .filter(|r| r.furnishing_types.contains(&ft) && r.auto_add_on_furnish)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Catalog builder
// ---------------------------------------------------------------------------

/// Builds a `RecipeCatalog` from config recipes, cooking parameters, and
/// fruit species (for extraction recipes).
///
/// `fruit_species` should be the full list of fruit species from the world.
/// For each species, one extraction recipe is generated (1 fruit → N component
/// items based on the species' parts). Pass an empty slice when fruit species
/// are not yet available (e.g., during early initialization before worldgen).
pub fn build_catalog(
    config: &crate::config::GameConfig,
    fruit_species: &[crate::fruit::FruitSpecies],
) -> RecipeCatalog {
    let mut recipes = BTreeMap::new();

    // Convert the bread recipe (formerly hardcoded kitchen logic).
    let bread_def = build_bread_recipe(config);
    recipes.insert(bread_def.key.clone(), bread_def);

    // Convert config workshop recipes.
    for recipe in &config.recipes {
        let def = convert_config_recipe(recipe);
        recipes.insert(def.key.clone(), def);
    }

    // Generate extraction recipes for each fruit species.
    for species in fruit_species {
        let def = build_extraction_recipe(config, species);
        recipes.insert(def.key.clone(), def);
    }

    // Generate component processing recipes for each fruit species based on
    // part properties (starchy → flour/bread, fine fiber → thread, etc.).
    for species in fruit_species {
        for def in build_component_recipes(config, species) {
            recipes.insert(def.key.clone(), def);
        }
    }

    // Generate per-wood-type Grow recipes (bows, arrows, armor).
    for def in build_wood_type_recipes(config) {
        recipes.insert(def.key.clone(), def);
    }

    RecipeCatalog { recipes }
}

/// Build an extraction recipe for a specific fruit species.
///
/// Input: 1 fruit of the given species.
/// Outputs: one item stack per part, using `PartType::extracted_item_kind()`
/// with quantity from `part.component_units`.
fn build_extraction_recipe(
    config: &crate::config::GameConfig,
    species: &crate::fruit::FruitSpecies,
) -> RecipeDef {
    let material = Material::FruitSpecies(species.id);
    let material_filter = MaterialFilter::Specific(material);

    let inputs_key = vec![(ItemKind::Fruit, material_filter, 1)];

    let mut outputs_key: Vec<(ItemKind, Option<Material>, u32)> = species
        .parts
        .iter()
        .map(|part| {
            (
                part.part_type.extracted_item_kind(),
                Some(material),
                part.component_units as u32,
            )
        })
        .collect();
    outputs_key.sort();

    let inputs = vec![crate::config::RecipeInput {
        item_kind: ItemKind::Fruit,
        quantity: 1,
        material_filter,
    }];

    let outputs: Vec<crate::config::RecipeOutput> = species
        .parts
        .iter()
        .map(|part| crate::config::RecipeOutput {
            item_kind: part.part_type.extracted_item_kind(),
            quantity: part.component_units as u32,
            material: Some(material),
            quality: 0,
        })
        .collect();

    let display_name = if species.vaelith_name.is_empty() {
        format!("Extract Fruit #{}", species.id.0)
    } else {
        format!("Extract {}", species.vaelith_name)
    };

    RecipeDef {
        key: RecipeKey {
            verb: RecipeVerb::Extract,
            inputs: inputs_key,
            outputs: outputs_key,
        },
        display_name,
        category: vec!["Extraction".to_string()],
        furnishing_types: vec![FurnishingType::Kitchen],
        inputs,
        outputs,
        work_ticks: config.extract_work_ticks,
        subcomponent_records: vec![],
        required_species: Some(Species::Elf),
        auto_add_on_furnish: false,
    }
}

/// Build component processing recipes for a fruit species based on part
/// properties. Each recipe-relevant property on a part generates a chain:
///
/// - **Starchy**: component → flour (Mill, Kitchen), flour → bread (Bake, Kitchen)
/// - **FibrousFine**: component → thread (Spin, Workshop), thread → bowstring (Assemble, Workshop),
///   thread → cloth (Weave, Workshop), cloth → tunic/leggings/boots/hat (Sew, Workshop)
/// - **FibrousCoarse**: component → cord (Twist, Workshop), cord → bowstring (Assemble, Workshop)
///
/// The dedup constraint in `generate_parts()` guarantees each recipe-relevant
/// property appears on at most one part per species, so no ambiguous recipes.
fn build_component_recipes(
    config: &crate::config::GameConfig,
    species: &crate::fruit::FruitSpecies,
) -> Vec<RecipeDef> {
    use crate::fruit::PartProperty;

    let cr = &config.component_recipes;
    let material = Material::FruitSpecies(species.id);
    let material_filter = MaterialFilter::Specific(material);
    let name = &species.vaelith_name;

    let mut recipes = Vec::new();

    for part in &species.parts {
        let component_item = part.part_type.extracted_item_kind();

        if part.properties.contains(&PartProperty::Starchy) {
            // Mill: starchy component → flour
            recipes.push(build_simple_recipe(
                RecipeVerb::Mill,
                &format!("Mill {name} {}", component_item.display_name()),
                vec!["Processing".to_string(), "Milling".to_string()],
                vec![FurnishingType::Kitchen],
                component_item,
                material_filter,
                cr.mill_input,
                ItemKind::Flour,
                Some(material),
                cr.mill_output,
                cr.mill_work_ticks,
            ));

            // Bake: flour → bread
            recipes.push(build_simple_recipe(
                RecipeVerb::Bake,
                &format!("Bake {name} Bread"),
                vec!["Processing".to_string(), "Baking".to_string()],
                vec![FurnishingType::Kitchen],
                ItemKind::Flour,
                material_filter,
                cr.bake_input,
                ItemKind::Bread,
                Some(material),
                cr.bake_output,
                cr.bake_work_ticks,
            ));
        }

        if part.properties.contains(&PartProperty::FibrousFine) {
            // Spin: fine fiber component → thread
            recipes.push(build_simple_recipe(
                RecipeVerb::Spin,
                &format!("Spin {name} {}", component_item.display_name()),
                vec!["Processing".to_string(), "Spinning".to_string()],
                vec![FurnishingType::Workshop],
                component_item,
                material_filter,
                cr.spin_input,
                ItemKind::Thread,
                Some(material),
                cr.spin_output,
                cr.spin_work_ticks,
            ));

            // Thread → bowstring
            recipes.push(build_simple_recipe(
                RecipeVerb::Assemble,
                &format!("{name} Thread Bowstring"),
                vec!["Processing".to_string(), "Bowstrings".to_string()],
                vec![FurnishingType::Workshop],
                ItemKind::Thread,
                material_filter,
                cr.thread_bowstring_input,
                ItemKind::Bowstring,
                Some(material),
                cr.thread_bowstring_output,
                cr.thread_bowstring_work_ticks,
            ));

            // Thread → cloth (weave)
            recipes.push(build_simple_recipe(
                RecipeVerb::Weave,
                &format!("Weave {name} Cloth"),
                vec!["Processing".to_string(), "Weaving".to_string()],
                vec![FurnishingType::Workshop],
                ItemKind::Thread,
                material_filter,
                cr.weave_input,
                ItemKind::Cloth,
                Some(material),
                cr.weave_output,
                cr.weave_work_ticks,
            ));

            // Cloth → clothing (sew)
            for (item, label, input, output, ticks) in [
                (
                    ItemKind::Tunic,
                    "Tunic",
                    cr.sew_tunic_input,
                    cr.sew_tunic_output,
                    cr.sew_tunic_work_ticks,
                ),
                (
                    ItemKind::Leggings,
                    "Leggings",
                    cr.sew_leggings_input,
                    cr.sew_leggings_output,
                    cr.sew_leggings_work_ticks,
                ),
                (
                    ItemKind::Boots,
                    "Boots",
                    cr.sew_boots_input,
                    cr.sew_boots_output,
                    cr.sew_boots_work_ticks,
                ),
                (
                    ItemKind::Hat,
                    "Hat",
                    cr.sew_hat_input,
                    cr.sew_hat_output,
                    cr.sew_hat_work_ticks,
                ),
                (
                    ItemKind::Gloves,
                    "Gloves",
                    cr.sew_gloves_input,
                    cr.sew_gloves_output,
                    cr.sew_gloves_work_ticks,
                ),
            ] {
                recipes.push(build_simple_recipe(
                    RecipeVerb::Sew,
                    &format!("Sew {name} {label}"),
                    vec!["Processing".to_string(), "Tailoring".to_string()],
                    vec![FurnishingType::Workshop],
                    ItemKind::Cloth,
                    material_filter,
                    input,
                    item,
                    Some(material),
                    output,
                    ticks,
                ));
            }
        }

        if part.properties.contains(&PartProperty::FibrousCoarse) {
            // Twist: coarse fiber component → cord
            recipes.push(build_simple_recipe(
                RecipeVerb::Twist,
                &format!("Twist {name} {}", component_item.display_name()),
                vec!["Processing".to_string(), "Twisting".to_string()],
                vec![FurnishingType::Workshop],
                component_item,
                material_filter,
                cr.twist_input,
                ItemKind::Cord,
                Some(material),
                cr.twist_output,
                cr.twist_work_ticks,
            ));

            // Cord → bowstring
            recipes.push(build_simple_recipe(
                RecipeVerb::Assemble,
                &format!("{name} Cord Bowstring"),
                vec!["Processing".to_string(), "Bowstrings".to_string()],
                vec![FurnishingType::Workshop],
                ItemKind::Cord,
                material_filter,
                cr.cord_bowstring_input,
                ItemKind::Bowstring,
                Some(material),
                cr.cord_bowstring_output,
                cr.cord_bowstring_work_ticks,
            ));
        }
    }

    recipes
}

/// Build per-wood-type Grow recipes for bows, arrows, and armor pieces.
///
/// For each of the 5 wood materials (Oak, Birch, Willow, Ash, Yew), generates:
/// - Grow {Wood} Bow (1 Bowstring input → 1 Bow output)
/// - Grow {Wood} Arrow (zero inputs → N arrows)
/// - Grow {Wood} Helmet/Breastplate/Greaves/Gauntlets/Boots (zero inputs → 1 armor piece)
///
/// All recipes use `RecipeVerb::Grow`, target `FurnishingType::Workshop`,
/// require `Species::Elf`, and are not auto-added on furnish (player selects
/// which wood-type recipes to activate based on their home tree).
fn build_wood_type_recipes(config: &crate::config::GameConfig) -> Vec<RecipeDef> {
    let gr = &config.grow_recipes;
    let mut recipes = Vec::new();

    for &wood in &Material::WOOD_TYPES {
        let name = wood.display_name();
        let material_filter = MaterialFilter::Specific(wood);

        // Bow: 1 Bowstring → 1 Bow
        recipes.push(build_simple_recipe(
            RecipeVerb::Grow,
            &format!("Grow {name} Bow"),
            vec!["Woodcraft".to_string(), "Weapons".to_string()],
            vec![FurnishingType::Workshop],
            ItemKind::Bowstring,
            MaterialFilter::Any,
            1,
            ItemKind::Bow,
            Some(wood),
            1,
            gr.grow_bow_work_ticks,
        ));

        // Arrow: zero inputs → N arrows
        recipes.push(build_no_input_recipe(
            RecipeVerb::Grow,
            &format!("Grow {name} Arrows"),
            vec!["Woodcraft".to_string(), "Weapons".to_string()],
            vec![FurnishingType::Workshop],
            ItemKind::Arrow,
            Some(wood),
            gr.grow_arrow_output,
            gr.grow_arrow_work_ticks,
        ));

        // Armor pieces: zero inputs → 1 piece each
        for (item, label, ticks) in [
            (ItemKind::Helmet, "Helmet", gr.grow_helmet_work_ticks),
            (
                ItemKind::Breastplate,
                "Breastplate",
                gr.grow_breastplate_work_ticks,
            ),
            (ItemKind::Greaves, "Greaves", gr.grow_greaves_work_ticks),
            (
                ItemKind::Gauntlets,
                "Gauntlets",
                gr.grow_gauntlets_work_ticks,
            ),
            (ItemKind::Boots, "Boots", gr.grow_boots_work_ticks),
        ] {
            recipes.push(build_no_input_recipe(
                RecipeVerb::Grow,
                &format!("Grow {name} {label}"),
                vec!["Woodcraft".to_string(), "Armor".to_string()],
                vec![FurnishingType::Workshop],
                item,
                Some(wood),
                1,
                ticks,
            ));
        }

        // Suppress unused variable warning — material_filter is available for
        // future recipes that need wood-specific inputs.
        let _ = material_filter;
    }

    recipes
}

/// Build a zero-input recipe (e.g., arrows grown from the tree, armor pieces).
#[allow(clippy::too_many_arguments)]
fn build_no_input_recipe(
    verb: RecipeVerb,
    display_name: &str,
    category: Vec<String>,
    furnishing_types: Vec<FurnishingType>,
    output_kind: ItemKind,
    output_material: Option<Material>,
    output_qty: u32,
    work_ticks: u64,
) -> RecipeDef {
    let outputs_key = vec![(output_kind, output_material, output_qty)];

    RecipeDef {
        key: RecipeKey {
            verb,
            inputs: vec![],
            outputs: outputs_key,
        },
        display_name: display_name.to_string(),
        category,
        furnishing_types,
        inputs: vec![],
        outputs: vec![RecipeOutput {
            item_kind: output_kind,
            quantity: output_qty,
            material: output_material,
            quality: 0,
        }],
        work_ticks,
        subcomponent_records: vec![],
        required_species: Some(Species::Elf),
        auto_add_on_furnish: false,
    }
}

/// Helper to build a simple 1-input → 1-output recipe. Used by the component
/// recipe generator to avoid repetitive struct construction.
#[allow(clippy::too_many_arguments)]
fn build_simple_recipe(
    verb: RecipeVerb,
    display_name: &str,
    category: Vec<String>,
    furnishing_types: Vec<FurnishingType>,
    input_kind: ItemKind,
    input_material: MaterialFilter,
    input_qty: u32,
    output_kind: ItemKind,
    output_material: Option<Material>,
    output_qty: u32,
    work_ticks: u64,
) -> RecipeDef {
    let mut inputs_key = vec![(input_kind, input_material, input_qty)];
    inputs_key.sort();

    let mut outputs_key = vec![(output_kind, output_material, output_qty)];
    outputs_key.sort();

    RecipeDef {
        key: RecipeKey {
            verb,
            inputs: inputs_key,
            outputs: outputs_key,
        },
        display_name: display_name.to_string(),
        category,
        furnishing_types,
        inputs: vec![RecipeInput {
            item_kind: input_kind,
            quantity: input_qty,
            material_filter: input_material,
        }],
        outputs: vec![RecipeOutput {
            item_kind: output_kind,
            quantity: output_qty,
            material: output_material,
            quality: 0,
        }],
        work_ticks,
        subcomponent_records: vec![],
        required_species: Some(Species::Elf),
        auto_add_on_furnish: false,
    }
}

/// Build the bread recipe from kitchen config parameters.
fn build_bread_recipe(config: &crate::config::GameConfig) -> RecipeDef {
    let mut inputs_key: Vec<(ItemKind, MaterialFilter, u32)> = vec![(
        ItemKind::Fruit,
        MaterialFilter::Any,
        config.cook_fruit_input,
    )];
    inputs_key.sort();

    let mut outputs_key: Vec<(ItemKind, Option<Material>, u32)> =
        vec![(ItemKind::Bread, None, config.cook_bread_output)];
    outputs_key.sort();

    RecipeDef {
        key: RecipeKey {
            verb: RecipeVerb::Cook,
            inputs: inputs_key,
            outputs: outputs_key,
        },
        display_name: "Bread".to_string(),
        category: vec![],
        furnishing_types: vec![FurnishingType::Kitchen],
        inputs: vec![RecipeInput {
            item_kind: ItemKind::Fruit,
            quantity: config.cook_fruit_input,
            material_filter: MaterialFilter::Any,
        }],
        outputs: vec![RecipeOutput {
            item_kind: ItemKind::Bread,
            quantity: config.cook_bread_output,
            material: None,
            quality: 0,
        }],
        work_ticks: config.cook_bread_work_ticks,
        subcomponent_records: vec![],
        required_species: Some(Species::Elf),
        auto_add_on_furnish: true,
    }
}

/// Extract a `RecipeKey` from an old-style `Recipe` config entry.
/// Used for backward-compatible mapping between the unified system and
/// legacy `TaskKind::Craft { recipe_id }`.
pub fn convert_config_recipe_key(recipe: &crate::config::Recipe) -> RecipeKey {
    let verb = match recipe.id.as_str() {
        "bowstring" => RecipeVerb::Assemble,
        _ => RecipeVerb::Assemble,
    };
    let mut inputs_key: Vec<(ItemKind, MaterialFilter, u32)> = recipe
        .inputs
        .iter()
        .map(|i| (i.item_kind, i.material_filter, i.quantity))
        .collect();
    inputs_key.sort();
    let mut outputs_key: Vec<(ItemKind, Option<Material>, u32)> = recipe
        .outputs
        .iter()
        .map(|o| (o.item_kind, o.material, o.quantity))
        .collect();
    outputs_key.sort();
    RecipeKey {
        verb,
        inputs: inputs_key,
        outputs: outputs_key,
    }
}

/// Convert an old-style `Recipe` to a `RecipeDef`.
fn convert_config_recipe(recipe: &crate::config::Recipe) -> RecipeDef {
    // Determine verb from the recipe ID. Remaining config recipe is bowstring
    // (assembly); bow and arrow moved to per-wood-type Grow generation.
    let verb = match recipe.id.as_str() {
        "bowstring" => RecipeVerb::Assemble,
        _ => RecipeVerb::Assemble,
    };

    let mut inputs_key: Vec<(ItemKind, MaterialFilter, u32)> = recipe
        .inputs
        .iter()
        .map(|i| (i.item_kind, i.material_filter, i.quantity))
        .collect();
    inputs_key.sort();

    let mut outputs_key: Vec<(ItemKind, Option<Material>, u32)> = recipe
        .outputs
        .iter()
        .map(|o| (o.item_kind, o.material, o.quantity))
        .collect();
    outputs_key.sort();

    RecipeDef {
        key: RecipeKey {
            verb,
            inputs: inputs_key,
            outputs: outputs_key,
        },
        display_name: recipe.display_name.clone(),
        category: vec![],
        furnishing_types: vec![FurnishingType::Workshop],
        inputs: recipe.inputs.clone(),
        outputs: recipe.outputs.clone(),
        work_ticks: recipe.work_ticks,
        subcomponent_records: recipe.subcomponent_records.clone(),
        required_species: Some(Species::Elf),
        auto_add_on_furnish: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GameConfig;

    #[test]
    fn recipe_key_json_roundtrip() {
        let key = RecipeKey {
            verb: RecipeVerb::Cook,
            inputs: vec![(ItemKind::Fruit, MaterialFilter::Any, 1)],
            outputs: vec![(ItemKind::Bread, None, 10)],
        };
        let json = key.to_json();
        let restored = RecipeKey::from_json(&json).expect("should deserialize");
        assert_eq!(key, restored);
    }

    #[test]
    fn recipe_key_canonical_sort() {
        // Keys with sorted inputs/outputs should be deterministic.
        let key1 = RecipeKey {
            verb: RecipeVerb::Assemble,
            inputs: vec![
                (ItemKind::Bowstring, MaterialFilter::Any, 1),
                (ItemKind::Fruit, MaterialFilter::Any, 2),
            ],
            outputs: vec![(ItemKind::Bow, None, 1)],
        };
        let key2 = RecipeKey {
            verb: RecipeVerb::Assemble,
            inputs: vec![
                (ItemKind::Bowstring, MaterialFilter::Any, 1),
                (ItemKind::Fruit, MaterialFilter::Any, 2),
            ],
            outputs: vec![(ItemKind::Bow, None, 1)],
        };
        assert_eq!(key1, key2);
        assert_eq!(key1.to_json(), key2.to_json());
    }

    #[test]
    fn build_catalog_contains_expected_recipes() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        // bread (1) + bowstring config (1) + wood-type grow recipes
        // (5 woods × 7 items = 35) = 37 total.
        assert_eq!(catalog.len(), 37);

        // Kitchen should have exactly 1 recipe (bread).
        let kitchen_recipes = catalog.recipes_for_furnishing(FurnishingType::Kitchen);
        assert_eq!(kitchen_recipes.len(), 1);
        assert_eq!(kitchen_recipes[0].display_name, "Bread");

        // Workshop should have bowstring (1) + wood-type grow recipes (35) = 36.
        let workshop_recipes = catalog.recipes_for_furnishing(FurnishingType::Workshop);
        assert_eq!(workshop_recipes.len(), 36);
    }

    #[test]
    fn catalog_outputs_have_nonzero_quantity() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);
        for (_key, def) in catalog.iter() {
            for output in &def.outputs {
                assert!(
                    output.quantity >= 1,
                    "Recipe '{}' has zero-quantity output {:?}",
                    def.display_name,
                    output.item_kind,
                );
            }
        }
    }

    #[test]
    fn recipe_key_inputs_outputs_are_sorted() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);
        for (key, _def) in catalog.iter() {
            let mut sorted_inputs = key.inputs.clone();
            sorted_inputs.sort();
            assert_eq!(
                key.inputs, sorted_inputs,
                "RecipeKey inputs not sorted for {:?}",
                key.verb,
            );
            let mut sorted_outputs = key.outputs.clone();
            sorted_outputs.sort();
            assert_eq!(
                key.outputs, sorted_outputs,
                "RecipeKey outputs not sorted for {:?}",
                key.verb,
            );
        }
    }

    #[test]
    fn extraction_recipes_generated_from_fruit_species() {
        use crate::fruit::{
            FruitAppearance, FruitColor, FruitPart, FruitShape, FruitSpecies, GrowthHabitat,
            PartType, Rarity,
        };
        use std::collections::BTreeSet;

        let config = GameConfig::default();
        let species = vec![FruitSpecies {
            id: crate::types::FruitSpeciesId(0),
            vaelith_name: "Testaleth".to_string(),
            english_gloss: "test-berry".to_string(),
            parts: vec![
                FruitPart {
                    part_type: PartType::Flesh,
                    properties: BTreeSet::new(),
                    pigment: None,
                    component_units: 40,
                },
                FruitPart {
                    part_type: PartType::Seed,
                    properties: BTreeSet::new(),
                    pigment: None,
                    component_units: 10,
                },
            ],
            habitat: GrowthHabitat::Branch,
            rarity: Rarity::Common,
            greenhouse_cultivable: false,
            appearance: FruitAppearance {
                exterior_color: FruitColor {
                    r: 200,
                    g: 100,
                    b: 50,
                },
                shape: FruitShape::Round,
                size_percent: 100,
                glows: false,
            },
        }];

        let catalog = build_catalog(&config, &species);

        // bread (1) + bowstring (1) + wood-type (35) + extraction (1) = 38.
        assert_eq!(catalog.len(), 38);

        // Kitchen should have bread + extraction = 2.
        let kitchen_recipes = catalog.recipes_for_furnishing(FurnishingType::Kitchen);
        assert_eq!(kitchen_recipes.len(), 2);

        let extract_def = kitchen_recipes
            .iter()
            .find(|r| r.display_name == "Extract Testaleth")
            .expect("extraction recipe should exist");

        assert_eq!(extract_def.key.verb, RecipeVerb::Extract);
        assert_eq!(extract_def.outputs.len(), 2);
        assert!(!extract_def.auto_add_on_furnish);
        assert_eq!(extract_def.category, vec!["Extraction".to_string()]);
        assert_eq!(extract_def.work_ticks, config.extract_work_ticks);

        // Keys should be sorted.
        let mut sorted_outputs = extract_def.key.outputs.clone();
        sorted_outputs.sort();
        assert_eq!(extract_def.key.outputs, sorted_outputs);

        // Default recipes should NOT include extraction.
        let defaults = catalog.default_recipes_for_furnishing(FurnishingType::Kitchen);
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].display_name, "Bread");
    }

    // --- Test helpers for component recipe tests ---

    use crate::fruit::{
        FruitAppearance, FruitColor, FruitPart, FruitShape, FruitSpecies, GrowthHabitat,
        PartProperty, PartType, Rarity,
    };
    use crate::types::FruitSpeciesId;
    use std::collections::BTreeSet;

    fn test_species(id: u16, name: &str, parts: Vec<FruitPart>) -> FruitSpecies {
        FruitSpecies {
            id: FruitSpeciesId(id),
            vaelith_name: name.to_string(),
            english_gloss: "test".to_string(),
            parts,
            habitat: GrowthHabitat::Branch,
            rarity: Rarity::Common,
            greenhouse_cultivable: false,
            appearance: FruitAppearance {
                exterior_color: FruitColor {
                    r: 200,
                    g: 100,
                    b: 50,
                },
                shape: FruitShape::Round,
                size_percent: 100,
                glows: false,
            },
        }
    }

    fn part(pt: PartType, props: &[PartProperty], units: u16) -> FruitPart {
        FruitPart {
            part_type: pt,
            properties: props.iter().copied().collect(),
            pigment: None,
            component_units: units,
        }
    }

    // --- Component recipe generation tests ---

    #[test]
    fn starchy_fruit_generates_mill_and_bake_recipes() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Starchi",
            vec![part(PartType::Flesh, &[PartProperty::Starchy], 40)],
        )];

        let catalog = build_catalog(&config, &species);

        // Should find mill and bake recipes for kitchen.
        let kitchen = catalog.recipes_for_furnishing(FurnishingType::Kitchen);
        let mill = kitchen
            .iter()
            .find(|r| r.display_name == "Mill Starchi Pulp")
            .expect("mill recipe should exist");
        assert_eq!(mill.key.verb, RecipeVerb::Mill);
        assert_eq!(mill.inputs[0].item_kind, ItemKind::Pulp);
        assert_eq!(mill.inputs[0].quantity, config.component_recipes.mill_input);
        assert_eq!(mill.outputs[0].item_kind, ItemKind::Flour);
        assert_eq!(
            mill.outputs[0].quantity,
            config.component_recipes.mill_output
        );
        assert!(!mill.auto_add_on_furnish);

        let bake = kitchen
            .iter()
            .find(|r| r.display_name == "Bake Starchi Bread")
            .expect("bake recipe should exist");
        assert_eq!(bake.key.verb, RecipeVerb::Bake);
        assert_eq!(bake.inputs[0].item_kind, ItemKind::Flour);
        assert_eq!(bake.outputs[0].item_kind, ItemKind::Bread);
        assert_eq!(
            bake.outputs[0].quantity,
            config.component_recipes.bake_output
        );
    }

    #[test]
    fn fibrous_fine_generates_spin_and_bowstring_recipes() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];

        let catalog = build_catalog(&config, &species);

        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);
        let spin = workshop
            .iter()
            .find(|r| r.display_name == "Spin Silkweed Fiber")
            .expect("spin recipe should exist");
        assert_eq!(spin.key.verb, RecipeVerb::Spin);
        assert_eq!(spin.inputs[0].item_kind, ItemKind::FruitFiber);
        assert_eq!(spin.outputs[0].item_kind, ItemKind::Thread);

        let bowstring = workshop
            .iter()
            .find(|r| r.display_name == "Silkweed Thread Bowstring")
            .expect("thread bowstring recipe should exist");
        assert_eq!(bowstring.key.verb, RecipeVerb::Assemble);
        assert_eq!(bowstring.inputs[0].item_kind, ItemKind::Thread);
        assert_eq!(bowstring.outputs[0].item_kind, ItemKind::Bowstring);
    }

    #[test]
    fn fibrous_coarse_generates_twist_and_bowstring_recipes() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Ropevine",
            vec![part(PartType::Fiber, &[PartProperty::FibrousCoarse], 50)],
        )];

        let catalog = build_catalog(&config, &species);

        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);
        let twist = workshop
            .iter()
            .find(|r| r.display_name == "Twist Ropevine Fiber")
            .expect("twist recipe should exist");
        assert_eq!(twist.key.verb, RecipeVerb::Twist);
        assert_eq!(twist.inputs[0].item_kind, ItemKind::FruitFiber);
        assert_eq!(twist.outputs[0].item_kind, ItemKind::Cord);

        let bowstring = workshop
            .iter()
            .find(|r| r.display_name == "Ropevine Cord Bowstring")
            .expect("cord bowstring recipe should exist");
        assert_eq!(bowstring.key.verb, RecipeVerb::Assemble);
        assert_eq!(bowstring.inputs[0].item_kind, ItemKind::Cord);
        assert_eq!(bowstring.outputs[0].item_kind, ItemKind::Bowstring);
    }

    #[test]
    fn no_component_recipes_for_non_recipe_properties() {
        let config = GameConfig::default();
        // Fruit with only Aromatic and Sweet — no recipe-relevant properties.
        let species = vec![test_species(
            0,
            "Sweetbloom",
            vec![
                part(PartType::Flesh, &[PartProperty::Sweet], 40),
                part(PartType::Rind, &[PartProperty::Aromatic], 20),
            ],
        )];

        let catalog = build_catalog(&config, &species);

        // Base (37) + 1 extraction = 38. No component recipes.
        assert_eq!(catalog.len(), 38);
    }

    #[test]
    fn multi_property_fruit_generates_all_chains() {
        let config = GameConfig::default();
        // Fruit with starchy flesh AND fine fiber — both chains should generate.
        let species = vec![test_species(
            0,
            "Allberry",
            vec![
                part(PartType::Flesh, &[PartProperty::Starchy], 40),
                part(PartType::Fiber, &[PartProperty::FibrousFine], 30),
            ],
        )];

        let catalog = build_catalog(&config, &species);

        // Base (37) + extraction (1) + mill + bake + spin + thread bowstring
        // + weave + sew tunic + sew leggings + sew boots + sew hat
        // + sew gloves = 48.
        assert_eq!(catalog.len(), 48);

        // Starchy chain in kitchen.
        let kitchen = catalog.recipes_for_furnishing(FurnishingType::Kitchen);
        assert!(
            kitchen
                .iter()
                .any(|r| r.display_name == "Mill Allberry Pulp")
        );
        assert!(
            kitchen
                .iter()
                .any(|r| r.display_name == "Bake Allberry Bread")
        );

        // Fiber chain in workshop.
        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);
        assert!(
            workshop
                .iter()
                .any(|r| r.display_name == "Spin Allberry Fiber")
        );
        assert!(
            workshop
                .iter()
                .any(|r| r.display_name == "Allberry Thread Bowstring")
        );
    }

    #[test]
    fn component_recipes_carry_species_material() {
        let config = GameConfig::default();
        let species = vec![test_species(
            7,
            "Testfruit",
            vec![part(PartType::Flesh, &[PartProperty::Starchy], 40)],
        )];

        let catalog = build_catalog(&config, &species);

        let kitchen = catalog.recipes_for_furnishing(FurnishingType::Kitchen);
        let mill = kitchen
            .iter()
            .find(|r| r.key.verb == RecipeVerb::Mill)
            .expect("mill recipe");

        // Input requires specific species material.
        assert_eq!(
            mill.inputs[0].material_filter,
            MaterialFilter::Specific(Material::FruitSpecies(FruitSpeciesId(7)))
        );
        // Output carries species material.
        assert_eq!(
            mill.outputs[0].material,
            Some(Material::FruitSpecies(FruitSpeciesId(7)))
        );

        let bake = kitchen
            .iter()
            .find(|r| r.key.verb == RecipeVerb::Bake)
            .expect("bake recipe");
        // Bake input is flour with species material.
        assert_eq!(bake.inputs[0].item_kind, ItemKind::Flour);
        assert_eq!(
            bake.inputs[0].material_filter,
            MaterialFilter::Specific(Material::FruitSpecies(FruitSpeciesId(7)))
        );
        // Bake output is bread with species material.
        assert_eq!(
            bake.outputs[0].material,
            Some(Material::FruitSpecies(FruitSpeciesId(7)))
        );
    }

    #[test]
    fn component_recipes_not_auto_added_on_furnish() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Starchi",
            vec![
                part(PartType::Flesh, &[PartProperty::Starchy], 40),
                part(PartType::Fiber, &[PartProperty::FibrousFine], 30),
            ],
        )];

        let catalog = build_catalog(&config, &species);

        // Default kitchen recipes should only include generic bread.
        let defaults = catalog.default_recipes_for_furnishing(FurnishingType::Kitchen);
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].display_name, "Bread");

        // Default workshop recipes should only include the bowstring config recipe.
        let defaults = catalog.default_recipes_for_furnishing(FurnishingType::Workshop);
        assert_eq!(defaults.len(), 1);
    }

    #[test]
    fn component_recipe_keys_sorted_and_roundtrip() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Testfruit",
            vec![
                part(PartType::Flesh, &[PartProperty::Starchy], 40),
                part(PartType::Fiber, &[PartProperty::FibrousFine], 30),
                part(PartType::Fiber, &[PartProperty::FibrousCoarse], 20),
            ],
        )];

        let catalog = build_catalog(&config, &species);
        for (key, _def) in catalog.iter() {
            // Keys should be sorted.
            let mut sorted_inputs = key.inputs.clone();
            sorted_inputs.sort();
            assert_eq!(key.inputs, sorted_inputs);
            let mut sorted_outputs = key.outputs.clone();
            sorted_outputs.sort();
            assert_eq!(key.outputs, sorted_outputs);

            // JSON roundtrip.
            let json = key.to_json();
            let restored = RecipeKey::from_json(&json).expect("should deserialize");
            assert_eq!(*key, restored);
        }
    }

    #[test]
    fn starchy_seed_uses_seed_item_kind() {
        // If seed has starchy, mill input should be Seed, not Pulp.
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Nutfruit",
            vec![part(PartType::Seed, &[PartProperty::Starchy], 25)],
        )];

        let catalog = build_catalog(&config, &species);
        let kitchen = catalog.recipes_for_furnishing(FurnishingType::Kitchen);
        let mill = kitchen
            .iter()
            .find(|r| r.key.verb == RecipeVerb::Mill)
            .expect("mill recipe");
        assert_eq!(mill.inputs[0].item_kind, ItemKind::Seed);
        assert_eq!(mill.display_name, "Mill Nutfruit Seed");
    }

    #[test]
    fn component_recipes_with_generated_fruits() {
        // Test with actual procedurally generated fruits across multiple seeds.
        use crate::fruit::generate_fruit_species;
        use elven_canopy_prng::GameRng;

        let config = GameConfig::default();

        for seed in 0..10 {
            let mut rng = GameRng::new(seed);
            let fruit_config = crate::config::FruitConfig::default();
            let fruits = generate_fruit_species(&mut rng, &fruit_config);
            let catalog = build_catalog(&config, &fruits);

            // Every recipe should have valid keys and nonzero outputs.
            for (_key, def) in catalog.iter() {
                for output in &def.outputs {
                    assert!(
                        output.quantity >= 1,
                        "Seed {}: recipe '{}' has zero-quantity output",
                        seed,
                        def.display_name,
                    );
                }
            }

            // Count component recipes: should match properties in fruits.
            let mut expected_starchy = 0;
            let mut expected_fine = 0;
            let mut expected_coarse = 0;
            for fruit in &fruits {
                for p in &fruit.parts {
                    if p.properties.contains(&PartProperty::Starchy) {
                        expected_starchy += 1;
                    }
                    if p.properties.contains(&PartProperty::FibrousFine) {
                        expected_fine += 1;
                    }
                    if p.properties.contains(&PartProperty::FibrousCoarse) {
                        expected_coarse += 1;
                    }
                }
            }

            let mill_count = catalog
                .iter()
                .filter(|(_, d)| d.key.verb == RecipeVerb::Mill)
                .count();
            let bake_count = catalog
                .iter()
                .filter(|(_, d)| d.key.verb == RecipeVerb::Bake)
                .count();
            let spin_count = catalog
                .iter()
                .filter(|(_, d)| d.key.verb == RecipeVerb::Spin)
                .count();
            let twist_count = catalog
                .iter()
                .filter(|(_, d)| d.key.verb == RecipeVerb::Twist)
                .count();

            assert_eq!(
                mill_count, expected_starchy,
                "Seed {}: mill recipe count mismatch",
                seed
            );
            assert_eq!(
                bake_count, expected_starchy,
                "Seed {}: bake recipe count mismatch",
                seed
            );
            assert_eq!(
                spin_count, expected_fine,
                "Seed {}: spin recipe count mismatch",
                seed
            );
            assert_eq!(
                twist_count, expected_coarse,
                "Seed {}: twist recipe count mismatch",
                seed
            );
        }
    }

    // --- Textile and clothing recipe tests ---

    #[test]
    fn fibrous_fine_generates_weave_and_clothing_recipes() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];

        let catalog = build_catalog(&config, &species);

        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);

        // Weave: thread → cloth
        let weave = workshop
            .iter()
            .find(|r| r.key.verb == RecipeVerb::Weave)
            .expect("weave recipe should exist");
        assert_eq!(weave.inputs[0].item_kind, ItemKind::Thread);
        assert_eq!(weave.outputs[0].item_kind, ItemKind::Cloth);
        assert_eq!(
            weave.inputs[0].quantity,
            config.component_recipes.weave_input
        );
        assert_eq!(
            weave.outputs[0].quantity,
            config.component_recipes.weave_output
        );
        assert_eq!(weave.display_name, "Weave Silkweed Cloth");
        assert!(!weave.auto_add_on_furnish);

        // Sew: cloth → tunic
        let tunic = workshop
            .iter()
            .find(|r| r.display_name == "Sew Silkweed Tunic")
            .expect("tunic recipe should exist");
        assert_eq!(tunic.key.verb, RecipeVerb::Sew);
        assert_eq!(tunic.inputs[0].item_kind, ItemKind::Cloth);
        assert_eq!(tunic.outputs[0].item_kind, ItemKind::Tunic);
        assert_eq!(
            tunic.inputs[0].quantity,
            config.component_recipes.sew_tunic_input
        );

        // Sew: cloth → leggings
        let leggings = workshop
            .iter()
            .find(|r| r.display_name == "Sew Silkweed Leggings")
            .expect("leggings recipe should exist");
        assert_eq!(leggings.key.verb, RecipeVerb::Sew);
        assert_eq!(leggings.inputs[0].item_kind, ItemKind::Cloth);
        assert_eq!(leggings.outputs[0].item_kind, ItemKind::Leggings);
        assert_eq!(
            leggings.inputs[0].quantity,
            config.component_recipes.sew_leggings_input
        );

        // Sew: cloth → boots
        let boots = workshop
            .iter()
            .find(|r| r.display_name == "Sew Silkweed Boots")
            .expect("boots recipe should exist");
        assert_eq!(boots.key.verb, RecipeVerb::Sew);
        assert_eq!(boots.inputs[0].item_kind, ItemKind::Cloth);
        assert_eq!(boots.outputs[0].item_kind, ItemKind::Boots);
        assert_eq!(
            boots.inputs[0].quantity,
            config.component_recipes.sew_boots_input
        );

        // Sew: cloth → hat
        let hat = workshop
            .iter()
            .find(|r| r.display_name == "Sew Silkweed Hat")
            .expect("hat recipe should exist");
        assert_eq!(hat.key.verb, RecipeVerb::Sew);
        assert_eq!(hat.inputs[0].item_kind, ItemKind::Cloth);
        assert_eq!(hat.outputs[0].item_kind, ItemKind::Hat);
        assert_eq!(
            hat.inputs[0].quantity,
            config.component_recipes.sew_hat_input
        );
    }

    #[test]
    fn textile_recipes_carry_species_material() {
        let config = GameConfig::default();
        let species = vec![test_species(
            5,
            "Lintberry",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];

        let catalog = build_catalog(&config, &species);
        let mat = Material::FruitSpecies(FruitSpeciesId(5));
        let mat_filter = MaterialFilter::Specific(mat);

        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);

        // Weave input/output carry material.
        let weave = workshop
            .iter()
            .find(|r| r.key.verb == RecipeVerb::Weave)
            .expect("weave recipe");
        assert_eq!(weave.inputs[0].material_filter, mat_filter);
        assert_eq!(weave.outputs[0].material, Some(mat));

        // Sew tunic input/output carry material.
        let tunic = workshop
            .iter()
            .find(|r| r.outputs[0].item_kind == ItemKind::Tunic)
            .expect("tunic recipe");
        assert_eq!(tunic.inputs[0].material_filter, mat_filter);
        assert_eq!(tunic.outputs[0].material, Some(mat));
    }

    #[test]
    fn textile_recipe_categories() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];

        let catalog = build_catalog(&config, &species);
        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);

        let weave = workshop
            .iter()
            .find(|r| r.key.verb == RecipeVerb::Weave)
            .unwrap();
        assert_eq!(
            weave.category,
            vec!["Processing".to_string(), "Weaving".to_string()]
        );

        let tunic = workshop
            .iter()
            .find(|r| r.outputs[0].item_kind == ItemKind::Tunic)
            .unwrap();
        assert_eq!(
            tunic.category,
            vec!["Processing".to_string(), "Tailoring".to_string()]
        );
    }

    #[test]
    fn multi_property_fruit_generates_textile_chain_too() {
        let config = GameConfig::default();
        // Fruit with starchy flesh AND fine fiber — textile chain should generate.
        let species = vec![test_species(
            0,
            "Allberry",
            vec![
                part(PartType::Flesh, &[PartProperty::Starchy], 40),
                part(PartType::Fiber, &[PartProperty::FibrousFine], 30),
            ],
        )];

        let catalog = build_catalog(&config, &species);

        // Base (37) + extraction (1) + mill + bake + spin + thread bowstring
        // + weave + sew tunic + sew leggings + sew boots + sew hat
        // + sew gloves = 48.
        assert_eq!(catalog.len(), 48);

        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);
        assert!(
            workshop
                .iter()
                .any(|r| r.display_name == "Weave Allberry Cloth")
        );
        assert!(
            workshop
                .iter()
                .any(|r| r.display_name == "Sew Allberry Tunic")
        );
        assert!(
            workshop
                .iter()
                .any(|r| r.display_name == "Sew Allberry Leggings")
        );
        assert!(
            workshop
                .iter()
                .any(|r| r.display_name == "Sew Allberry Boots")
        );
        assert!(
            workshop
                .iter()
                .any(|r| r.display_name == "Sew Allberry Hat")
        );
    }

    #[test]
    fn coarse_fiber_does_not_generate_textile_recipes() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Ropevine",
            vec![part(PartType::Fiber, &[PartProperty::FibrousCoarse], 50)],
        )];

        let catalog = build_catalog(&config, &species);

        // No weave or sew recipes for coarse fiber.
        let workshop = catalog.recipes_for_furnishing(FurnishingType::Workshop);
        assert!(
            !workshop.iter().any(|r| r.key.verb == RecipeVerb::Weave),
            "coarse fiber should not generate weave recipes"
        );
        assert!(
            !workshop.iter().any(|r| r.key.verb == RecipeVerb::Sew),
            "coarse fiber should not generate sew recipes"
        );
    }

    #[test]
    fn textile_recipes_with_generated_fruits() {
        // Test with actual procedurally generated fruits across multiple seeds.
        use crate::fruit::generate_fruit_species;
        use elven_canopy_prng::GameRng;

        let config = GameConfig::default();

        for seed in 0..10 {
            let mut rng = GameRng::new(seed);
            let fruit_config = crate::config::FruitConfig::default();
            let fruits = generate_fruit_species(&mut rng, &fruit_config);
            let catalog = build_catalog(&config, &fruits);

            // Count expected textile recipe sets (one per FibrousFine species).
            let mut expected_fine = 0;
            for fruit in &fruits {
                for p in &fruit.parts {
                    if p.properties.contains(&PartProperty::FibrousFine) {
                        expected_fine += 1;
                    }
                }
            }

            let weave_count = catalog
                .iter()
                .filter(|(_, d)| d.key.verb == RecipeVerb::Weave)
                .count();
            let sew_count = catalog
                .iter()
                .filter(|(_, d)| d.key.verb == RecipeVerb::Sew)
                .count();

            assert_eq!(
                weave_count, expected_fine,
                "Seed {}: weave recipe count mismatch",
                seed
            );
            // 5 sew recipes per FibrousFine species (tunic, leggings, boots, hat, gloves).
            assert_eq!(
                sew_count,
                expected_fine * 5,
                "Seed {}: sew recipe count mismatch",
                seed
            );
        }
    }

    #[test]
    fn new_textile_verb_serde_roundtrip() {
        for verb in [RecipeVerb::Weave, RecipeVerb::Sew] {
            let json = serde_json::to_string(&verb).unwrap();
            let parsed: RecipeVerb = serde_json::from_str(&json).unwrap();
            assert_eq!(verb, parsed);
        }
    }

    #[test]
    fn new_textile_item_kind_serde_roundtrip() {
        for kind in [
            ItemKind::Cloth,
            ItemKind::Tunic,
            ItemKind::Leggings,
            ItemKind::Boots,
            ItemKind::Hat,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let parsed: ItemKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn new_recipe_verb_serde_roundtrip() {
        for verb in [RecipeVerb::Spin, RecipeVerb::Twist, RecipeVerb::Bake] {
            let json = serde_json::to_string(&verb).unwrap();
            let parsed: RecipeVerb = serde_json::from_str(&json).unwrap();
            assert_eq!(verb, parsed);
        }
    }

    #[test]
    fn new_item_kind_serde_roundtrip() {
        for kind in [ItemKind::Flour, ItemKind::Thread, ItemKind::Cord] {
            let json = serde_json::to_string(&kind).unwrap();
            let parsed: ItemKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, parsed);
        }
    }

    // --- Wood-type Grow recipe tests ---

    #[test]
    fn grow_verb_serde_roundtrip() {
        let json = serde_json::to_string(&RecipeVerb::Grow).unwrap();
        let parsed: RecipeVerb = serde_json::from_str(&json).unwrap();
        assert_eq!(RecipeVerb::Grow, parsed);
    }

    #[test]
    fn armor_item_kind_serde_roundtrip() {
        for kind in [
            ItemKind::Helmet,
            ItemKind::Breastplate,
            ItemKind::Greaves,
            ItemKind::Gauntlets,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let parsed: ItemKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn wood_type_recipes_generated_for_all_materials() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        let grow_recipes: Vec<_> = catalog
            .iter()
            .filter(|(_, d)| d.key.verb == RecipeVerb::Grow)
            .map(|(_, d)| d)
            .collect();

        // 5 wood types × 7 items (bow, arrow, helmet, breastplate, greaves,
        // gauntlets, boots) = 35 total.
        assert_eq!(grow_recipes.len(), 35);

        // Each wood type should have exactly 7 recipes.
        for wood in &Material::WOOD_TYPES {
            let name = wood.display_name();
            let wood_recipes: Vec<_> = grow_recipes
                .iter()
                .filter(|r| r.display_name.starts_with(&format!("Grow {name}")))
                .collect();
            assert_eq!(
                wood_recipes.len(),
                7,
                "{name} should have 7 grow recipes, got {}",
                wood_recipes.len()
            );
        }
    }

    #[test]
    fn grow_bow_recipe_has_bowstring_input() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        let oak_bow = catalog
            .iter()
            .find(|(_, d)| d.display_name == "Grow Oak Bow")
            .map(|(_, d)| d)
            .expect("Grow Oak Bow recipe should exist");

        assert_eq!(oak_bow.key.verb, RecipeVerb::Grow);
        assert_eq!(oak_bow.inputs.len(), 1);
        assert_eq!(oak_bow.inputs[0].item_kind, ItemKind::Bowstring);
        assert_eq!(oak_bow.inputs[0].quantity, 1);
        assert_eq!(oak_bow.inputs[0].material_filter, MaterialFilter::Any);
        assert_eq!(oak_bow.outputs.len(), 1);
        assert_eq!(oak_bow.outputs[0].item_kind, ItemKind::Bow);
        assert_eq!(oak_bow.outputs[0].quantity, 1);
        assert_eq!(oak_bow.outputs[0].material, Some(Material::Oak));
        assert_eq!(oak_bow.work_ticks, config.grow_recipes.grow_bow_work_ticks);
    }

    #[test]
    fn grow_arrow_recipe_has_no_inputs() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        let oak_arrows = catalog
            .iter()
            .find(|(_, d)| d.display_name == "Grow Oak Arrows")
            .map(|(_, d)| d)
            .expect("Grow Oak Arrows recipe should exist");

        assert_eq!(oak_arrows.inputs.len(), 0);
        assert_eq!(oak_arrows.outputs.len(), 1);
        assert_eq!(oak_arrows.outputs[0].item_kind, ItemKind::Arrow);
        assert_eq!(
            oak_arrows.outputs[0].quantity,
            config.grow_recipes.grow_arrow_output
        );
        assert_eq!(oak_arrows.outputs[0].material, Some(Material::Oak));
    }

    #[test]
    fn grow_armor_recipes_have_no_inputs() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        for (name, item_kind) in [
            ("Grow Oak Helmet", ItemKind::Helmet),
            ("Grow Oak Breastplate", ItemKind::Breastplate),
            ("Grow Oak Greaves", ItemKind::Greaves),
            ("Grow Oak Gauntlets", ItemKind::Gauntlets),
            ("Grow Oak Boots", ItemKind::Boots),
        ] {
            let recipe = catalog
                .iter()
                .find(|(_, d)| d.display_name == name)
                .map(|(_, d)| d)
                .unwrap_or_else(|| panic!("{name} recipe should exist"));

            assert_eq!(recipe.key.verb, RecipeVerb::Grow);
            assert!(recipe.inputs.is_empty(), "{name} should have no inputs");
            assert_eq!(recipe.outputs.len(), 1);
            assert_eq!(recipe.outputs[0].item_kind, item_kind);
            assert_eq!(recipe.outputs[0].quantity, 1);
            assert_eq!(recipe.outputs[0].material, Some(Material::Oak));
        }
    }

    #[test]
    fn grow_armor_recipes_carry_wood_material() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        // Check that each wood type produces armor with that material.
        for wood in &Material::WOOD_TYPES {
            let name = wood.display_name();
            let helmet = catalog
                .iter()
                .find(|(_, d)| d.display_name == format!("Grow {name} Helmet"))
                .map(|(_, d)| d)
                .unwrap_or_else(|| panic!("Grow {name} Helmet should exist"));
            assert_eq!(helmet.outputs[0].material, Some(*wood));
        }
    }

    #[test]
    fn grow_recipes_not_auto_added_on_furnish() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        let defaults = catalog.default_recipes_for_furnishing(FurnishingType::Workshop);
        // Only the bowstring config recipe should be auto-added.
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].display_name, "Bowstring");

        // No grow recipes should be auto-added.
        assert!(
            !defaults.iter().any(|r| r.key.verb == RecipeVerb::Grow),
            "Grow recipes should not be auto-added on furnish"
        );
    }

    #[test]
    fn grow_recipe_categories() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        let bow = catalog
            .iter()
            .find(|(_, d)| d.display_name == "Grow Oak Bow")
            .map(|(_, d)| d)
            .unwrap();
        assert_eq!(
            bow.category,
            vec!["Woodcraft".to_string(), "Weapons".to_string()]
        );

        let helmet = catalog
            .iter()
            .find(|(_, d)| d.display_name == "Grow Oak Helmet")
            .map(|(_, d)| d)
            .unwrap();
        assert_eq!(
            helmet.category,
            vec!["Woodcraft".to_string(), "Armor".to_string()]
        );
    }

    #[test]
    fn grow_recipe_keys_sorted_and_roundtrip() {
        let config = GameConfig::default();
        let catalog = build_catalog(&config, &[]);

        for (key, _def) in catalog
            .iter()
            .filter(|(_, d)| d.key.verb == RecipeVerb::Grow)
        {
            let mut sorted_inputs = key.inputs.clone();
            sorted_inputs.sort();
            assert_eq!(key.inputs, sorted_inputs);
            let mut sorted_outputs = key.outputs.clone();
            sorted_outputs.sort();
            assert_eq!(key.outputs, sorted_outputs);

            let json = key.to_json();
            let restored = RecipeKey::from_json(&json).expect("should deserialize");
            assert_eq!(*key, restored);
        }
    }
}
