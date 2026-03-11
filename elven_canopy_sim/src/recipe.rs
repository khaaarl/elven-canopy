// Unified recipe catalog for data-driven crafting.
//
// Replaces the per-building-type crafting systems (kitchen bread baking,
// workshop recipe list) with a single unified model. Recipes are identified
// structurally by `RecipeKey` (verb + sorted inputs/outputs), not by string
// names. The `RecipeCatalog` is built at startup from config recipes and
// dynamically generated fruit extraction recipes (one per fruit species),
// then stored immutably on `SimState`.
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
        "bow" => RecipeVerb::Assemble,
        "arrow" => RecipeVerb::Fletch,
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
    // Determine verb from the recipe ID. Existing recipes are all workshop
    // assembly operations.
    let verb = match recipe.id.as_str() {
        "bowstring" => RecipeVerb::Assemble,
        "bow" => RecipeVerb::Assemble,
        "arrow" => RecipeVerb::Fletch,
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

        // Should have bread + 3 workshop recipes = 4 total.
        assert_eq!(catalog.len(), 4);

        // Kitchen should have exactly 1 recipe (bread).
        let kitchen_recipes = catalog.recipes_for_furnishing(FurnishingType::Kitchen);
        assert_eq!(kitchen_recipes.len(), 1);
        assert_eq!(kitchen_recipes[0].display_name, "Bread");

        // Workshop should have 3 recipes.
        let workshop_recipes = catalog.recipes_for_furnishing(FurnishingType::Workshop);
        assert_eq!(workshop_recipes.len(), 3);
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

        // Should have bread (1) + workshop (3) + extraction (1) = 5.
        assert_eq!(catalog.len(), 5);

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
}
