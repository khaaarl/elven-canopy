// Parameterized recipe templates for the crafting system.
//
// Key types:
// - `Recipe` — fixed enum (24 active variants), each a recipe template.
// - `RecipeParams` — parameter bindings (currently just material).
// - `ResolvedRecipe` — concrete inputs/outputs from `Recipe::resolve()`.
// - `RecipeVerb` — verb enum used for UI grouping via `Recipe::verb()`.
//
// Each `Recipe` variant is one specific transformation (e.g., Mill, SewTunic).
// Combined with `RecipeParams` (material selection) and resolved against
// `GameConfig` + fruit species data to produce concrete inputs/outputs.
//
// See also: `config.rs` for `RecipeInput`/`RecipeOutput` structs,
// `db.rs` for `ActiveRecipe`/`ActiveRecipeTarget` tables,
// `command.rs` for crafting commands, `sim/crafting.rs` for the crafting monitor.

use crate::config::{RecipeInput, RecipeOutput, RecipeSubcomponentRecord};
use crate::inventory::{ItemKind, Material, MaterialFilter};
use crate::types::{FurnishingType, Species};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// RecipeVerb
// ---------------------------------------------------------------------------

/// Distinguishes fundamentally different crafting processes that happen to
/// share the same inputs and outputs (e.g., "husk fruit" vs "press fruit").
///
/// **STABLE ENUM:** Discriminant values are used for UI grouping.
/// Never reorder, never reuse a number. Append new variants at the end
/// with the next sequential discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum RecipeVerb {
    Assemble = 0,
    // 1 = reserved (was Brew)
    // 2 = reserved (was Cook)
    Extract = 3,
    // 4 = reserved (was Fletch)
    // 5 = reserved (was Husk)
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
    /// Press a pigmented fruit component into dye.
    Press = 13,
    // Append new variants here with the next sequential number.
}

// ---------------------------------------------------------------------------
// Recipe enum — parameterized recipe templates (F-recipe-params)
// ---------------------------------------------------------------------------

/// A fixed recipe template. Each variant is one specific transformation
/// (e.g., Mill, SewTunic). Combined with `RecipeParams` (material selection)
/// and resolved against `GameConfig` + fruit species data to produce concrete
/// inputs/outputs via `resolve()`.
///
/// **STABLE ENUM:** Variant names are serialized into save files. Never rename
/// variants. Discriminant values (`#[repr(u16)]`) are for in-memory
/// representation only — never reorder or reuse discriminants.
///
/// See also: `RecipeParams` for parameter bindings, `ResolvedRecipe` for
/// concrete inputs/outputs, the design doc at `docs/drafts/recipe_params.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum Recipe {
    // --- Fruit processing (material param: FruitSpecies) ---
    /// Fruit → components (outputs vary by species part composition).
    Extract = 0,
    /// Starchy component → Flour.
    Mill = 1,
    /// Flour → Bread.
    Bake = 2,
    /// Fine-fiber component → Thread.
    Spin = 3,
    /// Coarse-fiber component → Cord.
    Twist = 4,
    /// Thread → Cloth.
    Weave = 5,
    /// Pigmented component → Dye.
    Press = 6,

    // --- Assembly (material param: FruitSpecies) ---
    /// Thread → Bowstring.
    AssembleThreadBowstring = 7,
    /// Cord → Bowstring.
    AssembleCordBowstring = 8,

    // --- Clothing (material param: FruitSpecies) ---
    /// Cloth → Tunic.
    SewTunic = 9,
    /// Cloth → Leggings.
    SewLeggings = 10,
    // 11 = reserved (was SewBoots, now split into SewSandals + SewShoes)
    /// Cloth → Hat.
    SewHat = 12,
    /// Cloth → Gloves.
    SewGloves = 13,

    // --- Wood equipment (material param: wood type) ---
    /// Bowstring(any) → Bow.
    GrowBow = 14,
    /// (no input) → Arrows.
    GrowArrow = 15,
    /// (no input) → Helmet.
    GrowHelmet = 16,
    /// (no input) → Breastplate.
    GrowBreastplate = 17,
    /// (no input) → Greaves.
    GrowGreaves = 18,
    /// (no input) → Gauntlets.
    GrowGauntlets = 19,
    /// (no input) → Boots.
    GrowBoots = 20,
    /// (no input) → Spear.
    GrowSpear = 21,
    /// (no input) → Club.
    GrowClub = 22,
    /// Cloth → Sandals (light civilian footwear).
    SewSandals = 23,
    /// Cloth → Shoes (standard civilian footwear).
    SewShoes = 24,
    // Future: DyeTunic, DyeLeggings, etc. (F-dye-application)
    // Future: MixDye (F-dye-mixing)
}

/// All Recipe variants in definition order.
pub const ALL_RECIPES: [Recipe; 24] = [
    Recipe::Extract,
    Recipe::Mill,
    Recipe::Bake,
    Recipe::Spin,
    Recipe::Twist,
    Recipe::Weave,
    Recipe::Press,
    Recipe::AssembleThreadBowstring,
    Recipe::AssembleCordBowstring,
    Recipe::SewTunic,
    Recipe::SewLeggings,
    Recipe::SewSandals,
    Recipe::SewShoes,
    Recipe::SewHat,
    Recipe::SewGloves,
    Recipe::GrowBow,
    Recipe::GrowArrow,
    Recipe::GrowHelmet,
    Recipe::GrowBreastplate,
    Recipe::GrowGreaves,
    Recipe::GrowGauntlets,
    Recipe::GrowBoots,
    Recipe::GrowSpear,
    Recipe::GrowClub,
];

/// Parameter bindings for a configured recipe instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeParams {
    /// Material selection. Always `Some(m)` in the initial implementation.
    /// F-recipe-any-mat adds `None` = "any" support.
    pub material: Option<Material>,
}

/// A fully resolved recipe: concrete inputs, outputs, and work cost.
/// Produced by `Recipe::resolve()`.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRecipe {
    pub inputs: Vec<RecipeInput>,
    pub outputs: Vec<RecipeOutput>,
    pub work_ticks: u64,
    pub subcomponent_records: Vec<RecipeSubcomponentRecord>,
}

impl Recipe {
    /// Whether this recipe has a material parameter.
    /// All current recipes return true.
    pub fn has_material_param(&self) -> bool {
        true
    }

    /// Valid materials for this recipe given the current world state.
    ///
    /// Fruit-processing recipes return `FruitSpecies` materials filtered by
    /// whether the species has the required part property. Wood recipes return
    /// `Material::WOOD_TYPES`.
    pub fn valid_materials(&self, fruit_species: &[crate::fruit::FruitSpecies]) -> Vec<Material> {
        use crate::fruit::PartProperty;

        match self {
            // Wood equipment recipes accept any wood type.
            Recipe::GrowBow
            | Recipe::GrowArrow
            | Recipe::GrowHelmet
            | Recipe::GrowBreastplate
            | Recipe::GrowGreaves
            | Recipe::GrowGauntlets
            | Recipe::GrowBoots
            | Recipe::GrowSpear
            | Recipe::GrowClub => Material::WOOD_TYPES.to_vec(),

            // Extract: any fruit species (all have parts to extract).
            Recipe::Extract => fruit_species
                .iter()
                .map(|s| Material::FruitSpecies(s.id))
                .collect(),

            // Mill/Bake: species with a Starchy part.
            Recipe::Mill | Recipe::Bake => fruit_species
                .iter()
                .filter(|s| {
                    s.parts
                        .iter()
                        .any(|p| p.properties.contains(&PartProperty::Starchy))
                })
                .map(|s| Material::FruitSpecies(s.id))
                .collect(),

            // Spin, Weave, SewX, AssembleThreadBowstring: species with FibrousFine.
            Recipe::Spin
            | Recipe::Weave
            | Recipe::AssembleThreadBowstring
            | Recipe::SewTunic
            | Recipe::SewLeggings
            | Recipe::SewSandals
            | Recipe::SewShoes
            | Recipe::SewHat
            | Recipe::SewGloves => fruit_species
                .iter()
                .filter(|s| {
                    s.parts
                        .iter()
                        .any(|p| p.properties.contains(&PartProperty::FibrousFine))
                })
                .map(|s| Material::FruitSpecies(s.id))
                .collect(),

            // Twist, AssembleCordBowstring: species with FibrousCoarse.
            Recipe::Twist | Recipe::AssembleCordBowstring => fruit_species
                .iter()
                .filter(|s| {
                    s.parts
                        .iter()
                        .any(|p| p.properties.contains(&PartProperty::FibrousCoarse))
                })
                .map(|s| Material::FruitSpecies(s.id))
                .collect(),

            // Press: species with a pigmented part.
            Recipe::Press => fruit_species
                .iter()
                .filter(|s| s.parts.iter().any(|p| p.pigment.is_some()))
                .map(|s| Material::FruitSpecies(s.id))
                .collect(),
        }
    }

    /// Which species can perform this recipe. All current recipes are elf-only.
    pub fn required_species(&self) -> Option<Species> {
        Some(Species::Elf)
    }

    /// Resolve this recipe template with the given parameters into concrete
    /// inputs and outputs.
    ///
    /// Returns `None` if the params are invalid for this recipe (e.g., wrong
    /// material category, species lacks required property).
    pub fn resolve(
        &self,
        params: &RecipeParams,
        config: &crate::config::GameConfig,
        fruit_species: &[crate::fruit::FruitSpecies],
    ) -> Option<ResolvedRecipe> {
        let material = params.material?;

        match self {
            Recipe::Extract => {
                let species_id = match material {
                    Material::FruitSpecies(id) => id,
                    _ => return None,
                };
                let species = fruit_species.iter().find(|s| s.id == species_id)?;
                let mat_filter = MaterialFilter::Specific(material);

                let inputs = vec![RecipeInput {
                    item_kind: ItemKind::Fruit,
                    quantity: 1,
                    material_filter: mat_filter,
                }];

                let outputs: Vec<RecipeOutput> = species
                    .parts
                    .iter()
                    .map(|part| RecipeOutput {
                        item_kind: part.part_type.extracted_item_kind(),
                        quantity: part.component_units as u32,
                        material: Some(material),

                        dye_color: None,
                    })
                    .collect();

                Some(ResolvedRecipe {
                    inputs,
                    outputs,
                    work_ticks: config.extract_work_ticks,
                    subcomponent_records: vec![],
                })
            }

            Recipe::Mill => self.resolve_starchy_component(
                material,
                fruit_species,
                |cr, component_item| {
                    let mat_filter = MaterialFilter::Specific(material);
                    ResolvedRecipe {
                        inputs: vec![RecipeInput {
                            item_kind: component_item,
                            quantity: cr.mill_input,
                            material_filter: mat_filter,
                        }],
                        outputs: vec![RecipeOutput {
                            item_kind: ItemKind::Flour,
                            quantity: cr.mill_output,
                            material: Some(material),

                            dye_color: None,
                        }],
                        work_ticks: cr.mill_work_ticks,
                        subcomponent_records: vec![],
                    }
                },
                config,
            ),

            Recipe::Bake => self.resolve_starchy_check(material, fruit_species, || {
                let mat_filter = MaterialFilter::Specific(material);
                let cr = &config.component_recipes;
                ResolvedRecipe {
                    inputs: vec![RecipeInput {
                        item_kind: ItemKind::Flour,
                        quantity: cr.bake_input,
                        material_filter: mat_filter,
                    }],
                    outputs: vec![RecipeOutput {
                        item_kind: ItemKind::Bread,
                        quantity: cr.bake_output,
                        material: Some(material),

                        dye_color: None,
                    }],
                    work_ticks: cr.bake_work_ticks,
                    subcomponent_records: vec![],
                }
            }),

            Recipe::Spin => self.resolve_fine_fiber_component(
                material,
                fruit_species,
                |cr, component_item| {
                    let mat_filter = MaterialFilter::Specific(material);
                    ResolvedRecipe {
                        inputs: vec![RecipeInput {
                            item_kind: component_item,
                            quantity: cr.spin_input,
                            material_filter: mat_filter,
                        }],
                        outputs: vec![RecipeOutput {
                            item_kind: ItemKind::Thread,
                            quantity: cr.spin_output,
                            material: Some(material),

                            dye_color: None,
                        }],
                        work_ticks: cr.spin_work_ticks,
                        subcomponent_records: vec![],
                    }
                },
                config,
            ),

            Recipe::Twist => self.resolve_coarse_fiber_component(
                material,
                fruit_species,
                |cr, component_item| {
                    let mat_filter = MaterialFilter::Specific(material);
                    ResolvedRecipe {
                        inputs: vec![RecipeInput {
                            item_kind: component_item,
                            quantity: cr.twist_input,
                            material_filter: mat_filter,
                        }],
                        outputs: vec![RecipeOutput {
                            item_kind: ItemKind::Cord,
                            quantity: cr.twist_output,
                            material: Some(material),

                            dye_color: None,
                        }],
                        work_ticks: cr.twist_work_ticks,
                        subcomponent_records: vec![],
                    }
                },
                config,
            ),

            Recipe::Weave => self.resolve_fine_fiber_check(material, fruit_species, || {
                let mat_filter = MaterialFilter::Specific(material);
                let cr = &config.component_recipes;
                ResolvedRecipe {
                    inputs: vec![RecipeInput {
                        item_kind: ItemKind::Thread,
                        quantity: cr.weave_input,
                        material_filter: mat_filter,
                    }],
                    outputs: vec![RecipeOutput {
                        item_kind: ItemKind::Cloth,
                        quantity: cr.weave_output,
                        material: Some(material),

                        dye_color: None,
                    }],
                    work_ticks: cr.weave_work_ticks,
                    subcomponent_records: vec![],
                }
            }),

            Recipe::Press => {
                let species_id = match material {
                    Material::FruitSpecies(id) => id,
                    _ => return None,
                };
                let species = fruit_species.iter().find(|s| s.id == species_id)?;

                // Find the pigmented part.
                let pigmented_part = species.parts.iter().find(|p| p.pigment.is_some())?;
                let dye_color = pigmented_part.pigment?.to_item_color();
                let component_item = pigmented_part.part_type.extracted_item_kind();
                let mat_filter = MaterialFilter::Specific(material);
                let cr = &config.component_recipes;

                Some(ResolvedRecipe {
                    inputs: vec![RecipeInput {
                        item_kind: component_item,
                        quantity: cr.press_input,
                        material_filter: mat_filter,
                    }],
                    outputs: vec![RecipeOutput {
                        item_kind: ItemKind::Dye,
                        quantity: cr.press_output,
                        material: Some(material),

                        dye_color: Some(dye_color),
                    }],
                    work_ticks: cr.press_work_ticks,
                    subcomponent_records: vec![],
                })
            }

            Recipe::AssembleThreadBowstring => {
                self.resolve_fine_fiber_check(material, fruit_species, || {
                    let mat_filter = MaterialFilter::Specific(material);
                    let cr = &config.component_recipes;
                    ResolvedRecipe {
                        inputs: vec![RecipeInput {
                            item_kind: ItemKind::Thread,
                            quantity: cr.thread_bowstring_input,
                            material_filter: mat_filter,
                        }],
                        outputs: vec![RecipeOutput {
                            item_kind: ItemKind::Bowstring,
                            quantity: cr.thread_bowstring_output,
                            material: Some(material),

                            dye_color: None,
                        }],
                        work_ticks: cr.thread_bowstring_work_ticks,
                        subcomponent_records: vec![],
                    }
                })
            }

            Recipe::AssembleCordBowstring => {
                self.resolve_coarse_fiber_check(material, fruit_species, || {
                    let mat_filter = MaterialFilter::Specific(material);
                    let cr = &config.component_recipes;
                    ResolvedRecipe {
                        inputs: vec![RecipeInput {
                            item_kind: ItemKind::Cord,
                            quantity: cr.cord_bowstring_input,
                            material_filter: mat_filter,
                        }],
                        outputs: vec![RecipeOutput {
                            item_kind: ItemKind::Bowstring,
                            quantity: cr.cord_bowstring_output,
                            material: Some(material),

                            dye_color: None,
                        }],
                        work_ticks: cr.cord_bowstring_work_ticks,
                        subcomponent_records: vec![],
                    }
                })
            }

            Recipe::SewTunic => self.resolve_sew(material, fruit_species, config, ItemKind::Tunic),
            Recipe::SewLeggings => {
                self.resolve_sew(material, fruit_species, config, ItemKind::Leggings)
            }
            Recipe::SewSandals => {
                self.resolve_sew(material, fruit_species, config, ItemKind::Sandals)
            }
            Recipe::SewShoes => self.resolve_sew(material, fruit_species, config, ItemKind::Shoes),
            Recipe::SewHat => self.resolve_sew(material, fruit_species, config, ItemKind::Hat),
            Recipe::SewGloves => {
                self.resolve_sew(material, fruit_species, config, ItemKind::Gloves)
            }

            Recipe::GrowBow => {
                if !material.is_wood() {
                    return None;
                }
                let gr = &config.grow_recipes;
                Some(ResolvedRecipe {
                    inputs: vec![RecipeInput {
                        item_kind: ItemKind::Bowstring,
                        quantity: 1,
                        material_filter: MaterialFilter::Any,
                    }],
                    outputs: vec![RecipeOutput {
                        item_kind: ItemKind::Bow,
                        quantity: 1,
                        material: Some(material),

                        dye_color: None,
                    }],
                    work_ticks: gr.grow_bow_work_ticks,
                    subcomponent_records: vec![RecipeSubcomponentRecord {
                        input_kind: ItemKind::Bowstring,
                        quantity_per_item: 1,
                    }],
                })
            }

            Recipe::GrowArrow => {
                if !material.is_wood() {
                    return None;
                }
                let gr = &config.grow_recipes;
                Some(ResolvedRecipe {
                    inputs: vec![],
                    outputs: vec![RecipeOutput {
                        item_kind: ItemKind::Arrow,
                        quantity: gr.grow_arrow_output,
                        material: Some(material),

                        dye_color: None,
                    }],
                    work_ticks: gr.grow_arrow_work_ticks,
                    subcomponent_records: vec![],
                })
            }

            Recipe::GrowHelmet => self.resolve_grow_armor(material, config, ItemKind::Helmet),
            Recipe::GrowBreastplate => {
                self.resolve_grow_armor(material, config, ItemKind::Breastplate)
            }
            Recipe::GrowGreaves => self.resolve_grow_armor(material, config, ItemKind::Greaves),
            Recipe::GrowGauntlets => self.resolve_grow_armor(material, config, ItemKind::Gauntlets),
            Recipe::GrowBoots => self.resolve_grow_armor(material, config, ItemKind::Boots),

            Recipe::GrowSpear => self.resolve_grow_weapon(material, config, ItemKind::Spear),
            Recipe::GrowClub => self.resolve_grow_weapon(material, config, ItemKind::Club),
        }
    }

    /// Human-readable name, incorporating material if bound.
    pub fn display_name(
        &self,
        params: &RecipeParams,
        fruit_species: &[crate::fruit::FruitSpecies],
    ) -> String {
        let mat_name = match params.material {
            Some(Material::FruitSpecies(id)) => fruit_species
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.vaelith_name.as_str())
                .unwrap_or("Unknown"),
            Some(m) => m.display_name(),
            None => "Any",
        };

        match self {
            Recipe::Extract => format!("Extract {mat_name}"),
            Recipe::Mill => {
                let component_name = self.starchy_component_name(params, fruit_species);
                format!("Mill {mat_name} {component_name}")
            }
            Recipe::Bake => format!("Bake {mat_name} Bread"),
            Recipe::Spin => {
                let component_name = self.fine_fiber_component_name(params, fruit_species);
                format!("Spin {mat_name} {component_name}")
            }
            Recipe::Twist => {
                let component_name = self.coarse_fiber_component_name(params, fruit_species);
                format!("Twist {mat_name} {component_name}")
            }
            Recipe::Weave => format!("Weave {mat_name} Cloth"),
            Recipe::Press => {
                let (component_name, color_name) =
                    self.pigment_component_info(params, fruit_species);
                format!("Press {mat_name} {component_name} {color_name} Dye")
            }
            Recipe::AssembleThreadBowstring => format!("{mat_name} Thread Bowstring"),
            Recipe::AssembleCordBowstring => format!("{mat_name} Cord Bowstring"),
            Recipe::SewTunic => format!("Sew {mat_name} Tunic"),
            Recipe::SewLeggings => format!("Sew {mat_name} Leggings"),
            Recipe::SewSandals => format!("Sew {mat_name} Sandals"),
            Recipe::SewShoes => format!("Sew {mat_name} Shoes"),
            Recipe::SewHat => format!("Sew {mat_name} Hat"),
            Recipe::SewGloves => format!("Sew {mat_name} Gloves"),
            Recipe::GrowBow => format!("Grow {mat_name} Bow"),
            Recipe::GrowArrow => format!("Grow {mat_name} Arrows"),
            Recipe::GrowHelmet => format!("Grow {mat_name} Helmet"),
            Recipe::GrowBreastplate => format!("Grow {mat_name} Breastplate"),
            Recipe::GrowGreaves => format!("Grow {mat_name} Greaves"),
            Recipe::GrowGauntlets => format!("Grow {mat_name} Gauntlets"),
            Recipe::GrowBoots => format!("Grow {mat_name} Boots"),
            Recipe::GrowSpear => format!("Grow {mat_name} Spear"),
            Recipe::GrowClub => format!("Grow {mat_name} Club"),
        }
    }

    /// Category path for UI hierarchy.
    pub fn category(&self) -> Vec<&'static str> {
        match self {
            Recipe::Extract => vec!["Extraction"],
            Recipe::Mill => vec!["Processing", "Milling"],
            Recipe::Bake => vec!["Processing", "Baking"],
            Recipe::Spin => vec!["Processing", "Spinning"],
            Recipe::Twist => vec!["Processing", "Twisting"],
            Recipe::Weave => vec!["Processing", "Weaving"],
            Recipe::Press => vec!["Processing", "Dye Pressing"],
            Recipe::AssembleThreadBowstring | Recipe::AssembleCordBowstring => {
                vec!["Processing", "Bowstrings"]
            }
            Recipe::SewTunic
            | Recipe::SewLeggings
            | Recipe::SewSandals
            | Recipe::SewShoes
            | Recipe::SewHat
            | Recipe::SewGloves => vec!["Processing", "Tailoring"],
            Recipe::GrowBow | Recipe::GrowArrow | Recipe::GrowSpear | Recipe::GrowClub => {
                vec!["Woodcraft", "Weapons"]
            }
            Recipe::GrowHelmet
            | Recipe::GrowBreastplate
            | Recipe::GrowGreaves
            | Recipe::GrowGauntlets
            | Recipe::GrowBoots => vec!["Woodcraft", "Armor"],
        }
    }

    /// Which furnishing types can use this recipe.
    pub fn furnishing_types(&self) -> Vec<FurnishingType> {
        match self {
            Recipe::Extract | Recipe::Mill | Recipe::Bake | Recipe::Press => {
                vec![FurnishingType::Kitchen]
            }
            Recipe::Spin
            | Recipe::Twist
            | Recipe::Weave
            | Recipe::AssembleThreadBowstring
            | Recipe::AssembleCordBowstring
            | Recipe::SewTunic
            | Recipe::SewLeggings
            | Recipe::SewSandals
            | Recipe::SewShoes
            | Recipe::SewHat
            | Recipe::SewGloves
            | Recipe::GrowBow
            | Recipe::GrowArrow
            | Recipe::GrowHelmet
            | Recipe::GrowBreastplate
            | Recipe::GrowGreaves
            | Recipe::GrowGauntlets
            | Recipe::GrowBoots
            | Recipe::GrowSpear
            | Recipe::GrowClub => vec![FurnishingType::Workshop],
        }
    }

    /// The crafting verb associated with this recipe (for UI grouping).
    pub fn verb(&self) -> RecipeVerb {
        match self {
            Recipe::Extract => RecipeVerb::Extract,
            Recipe::Mill => RecipeVerb::Mill,
            Recipe::Bake => RecipeVerb::Bake,
            Recipe::Spin => RecipeVerb::Spin,
            Recipe::Twist => RecipeVerb::Twist,
            Recipe::Weave => RecipeVerb::Weave,
            Recipe::Press => RecipeVerb::Press,
            Recipe::AssembleThreadBowstring | Recipe::AssembleCordBowstring => RecipeVerb::Assemble,
            Recipe::SewTunic
            | Recipe::SewLeggings
            | Recipe::SewSandals
            | Recipe::SewShoes
            | Recipe::SewHat
            | Recipe::SewGloves => RecipeVerb::Sew,
            Recipe::GrowBow
            | Recipe::GrowArrow
            | Recipe::GrowHelmet
            | Recipe::GrowBreastplate
            | Recipe::GrowGreaves
            | Recipe::GrowGauntlets
            | Recipe::GrowBoots
            | Recipe::GrowSpear
            | Recipe::GrowClub => RecipeVerb::Grow,
        }
    }

    // --- Private resolution helpers ---

    /// Resolve a recipe that needs a Starchy component, passing the component's
    /// ItemKind (from the part that has Starchy) to the builder function.
    fn resolve_starchy_component(
        &self,
        material: Material,
        fruit_species: &[crate::fruit::FruitSpecies],
        builder: impl FnOnce(&crate::config::ComponentRecipeConfig, ItemKind) -> ResolvedRecipe,
        config: &crate::config::GameConfig,
    ) -> Option<ResolvedRecipe> {
        use crate::fruit::PartProperty;
        let species_id = match material {
            Material::FruitSpecies(id) => id,
            _ => return None,
        };
        let species = fruit_species.iter().find(|s| s.id == species_id)?;
        let starchy_part = species
            .parts
            .iter()
            .find(|p| p.properties.contains(&PartProperty::Starchy))?;
        Some(builder(
            &config.component_recipes,
            starchy_part.part_type.extracted_item_kind(),
        ))
    }

    /// Check that a species has Starchy property, then call builder.
    fn resolve_starchy_check(
        &self,
        material: Material,
        fruit_species: &[crate::fruit::FruitSpecies],
        builder: impl FnOnce() -> ResolvedRecipe,
    ) -> Option<ResolvedRecipe> {
        use crate::fruit::PartProperty;
        let species_id = match material {
            Material::FruitSpecies(id) => id,
            _ => return None,
        };
        let species = fruit_species.iter().find(|s| s.id == species_id)?;
        if !species
            .parts
            .iter()
            .any(|p| p.properties.contains(&PartProperty::Starchy))
        {
            return None;
        }
        Some(builder())
    }

    /// Resolve a recipe that needs a FibrousFine component.
    fn resolve_fine_fiber_component(
        &self,
        material: Material,
        fruit_species: &[crate::fruit::FruitSpecies],
        builder: impl FnOnce(&crate::config::ComponentRecipeConfig, ItemKind) -> ResolvedRecipe,
        config: &crate::config::GameConfig,
    ) -> Option<ResolvedRecipe> {
        use crate::fruit::PartProperty;
        let species_id = match material {
            Material::FruitSpecies(id) => id,
            _ => return None,
        };
        let species = fruit_species.iter().find(|s| s.id == species_id)?;
        let fine_part = species
            .parts
            .iter()
            .find(|p| p.properties.contains(&PartProperty::FibrousFine))?;
        Some(builder(
            &config.component_recipes,
            fine_part.part_type.extracted_item_kind(),
        ))
    }

    /// Check that a species has FibrousFine property, then call builder.
    fn resolve_fine_fiber_check(
        &self,
        material: Material,
        fruit_species: &[crate::fruit::FruitSpecies],
        builder: impl FnOnce() -> ResolvedRecipe,
    ) -> Option<ResolvedRecipe> {
        use crate::fruit::PartProperty;
        let species_id = match material {
            Material::FruitSpecies(id) => id,
            _ => return None,
        };
        let species = fruit_species.iter().find(|s| s.id == species_id)?;
        if !species
            .parts
            .iter()
            .any(|p| p.properties.contains(&PartProperty::FibrousFine))
        {
            return None;
        }
        Some(builder())
    }

    /// Resolve a recipe that needs a FibrousCoarse component.
    fn resolve_coarse_fiber_component(
        &self,
        material: Material,
        fruit_species: &[crate::fruit::FruitSpecies],
        builder: impl FnOnce(&crate::config::ComponentRecipeConfig, ItemKind) -> ResolvedRecipe,
        config: &crate::config::GameConfig,
    ) -> Option<ResolvedRecipe> {
        use crate::fruit::PartProperty;
        let species_id = match material {
            Material::FruitSpecies(id) => id,
            _ => return None,
        };
        let species = fruit_species.iter().find(|s| s.id == species_id)?;
        let coarse_part = species
            .parts
            .iter()
            .find(|p| p.properties.contains(&PartProperty::FibrousCoarse))?;
        Some(builder(
            &config.component_recipes,
            coarse_part.part_type.extracted_item_kind(),
        ))
    }

    /// Check that a species has FibrousCoarse property, then call builder.
    fn resolve_coarse_fiber_check(
        &self,
        material: Material,
        fruit_species: &[crate::fruit::FruitSpecies],
        builder: impl FnOnce() -> ResolvedRecipe,
    ) -> Option<ResolvedRecipe> {
        use crate::fruit::PartProperty;
        let species_id = match material {
            Material::FruitSpecies(id) => id,
            _ => return None,
        };
        let species = fruit_species.iter().find(|s| s.id == species_id)?;
        if !species
            .parts
            .iter()
            .any(|p| p.properties.contains(&PartProperty::FibrousCoarse))
        {
            return None;
        }
        Some(builder())
    }

    /// Resolve a Sew recipe for a specific clothing item.
    fn resolve_sew(
        &self,
        material: Material,
        fruit_species: &[crate::fruit::FruitSpecies],
        config: &crate::config::GameConfig,
        output_kind: ItemKind,
    ) -> Option<ResolvedRecipe> {
        self.resolve_fine_fiber_check(material, fruit_species, || {
            let mat_filter = MaterialFilter::Specific(material);
            let cr = &config.component_recipes;
            let (input_qty, output_qty, work_ticks) = match output_kind {
                ItemKind::Tunic => (
                    cr.sew_tunic_input,
                    cr.sew_tunic_output,
                    cr.sew_tunic_work_ticks,
                ),
                ItemKind::Leggings => (
                    cr.sew_leggings_input,
                    cr.sew_leggings_output,
                    cr.sew_leggings_work_ticks,
                ),
                ItemKind::Sandals => (
                    cr.sew_sandals_input,
                    cr.sew_sandals_output,
                    cr.sew_sandals_work_ticks,
                ),
                ItemKind::Shoes => (
                    cr.sew_shoes_input,
                    cr.sew_shoes_output,
                    cr.sew_shoes_work_ticks,
                ),
                ItemKind::Hat => (cr.sew_hat_input, cr.sew_hat_output, cr.sew_hat_work_ticks),
                ItemKind::Gloves => (
                    cr.sew_gloves_input,
                    cr.sew_gloves_output,
                    cr.sew_gloves_work_ticks,
                ),
                _ => unreachable!("resolve_sew called with non-clothing ItemKind"),
            };
            ResolvedRecipe {
                inputs: vec![RecipeInput {
                    item_kind: ItemKind::Cloth,
                    quantity: input_qty,
                    material_filter: mat_filter,
                }],
                outputs: vec![RecipeOutput {
                    item_kind: output_kind,
                    quantity: output_qty,
                    material: Some(material),

                    dye_color: None,
                }],
                work_ticks,
                subcomponent_records: vec![],
            }
        })
    }

    /// Resolve a Grow armor recipe (zero inputs → 1 armor piece).
    fn resolve_grow_armor(
        &self,
        material: Material,
        config: &crate::config::GameConfig,
        output_kind: ItemKind,
    ) -> Option<ResolvedRecipe> {
        if !material.is_wood() {
            return None;
        }
        let gr = &config.grow_recipes;
        let work_ticks = match output_kind {
            ItemKind::Helmet => gr.grow_helmet_work_ticks,
            ItemKind::Breastplate => gr.grow_breastplate_work_ticks,
            ItemKind::Greaves => gr.grow_greaves_work_ticks,
            ItemKind::Gauntlets => gr.grow_gauntlets_work_ticks,
            ItemKind::Boots => gr.grow_boots_work_ticks,
            _ => unreachable!("resolve_grow_armor called with non-armor ItemKind"),
        };
        Some(ResolvedRecipe {
            inputs: vec![],
            outputs: vec![RecipeOutput {
                item_kind: output_kind,
                quantity: 1,
                material: Some(material),

                dye_color: None,
            }],
            work_ticks,
            subcomponent_records: vec![],
        })
    }

    /// Resolve a Grow melee weapon recipe (zero inputs → 1 weapon).
    fn resolve_grow_weapon(
        &self,
        material: Material,
        config: &crate::config::GameConfig,
        output_kind: ItemKind,
    ) -> Option<ResolvedRecipe> {
        if !material.is_wood() {
            return None;
        }
        let gr = &config.grow_recipes;
        let work_ticks = match output_kind {
            ItemKind::Spear => gr.grow_spear_work_ticks,
            ItemKind::Club => gr.grow_club_work_ticks,
            _ => unreachable!("resolve_grow_weapon called with non-weapon ItemKind"),
        };
        Some(ResolvedRecipe {
            inputs: vec![],
            outputs: vec![RecipeOutput {
                item_kind: output_kind,
                quantity: 1,
                material: Some(material),

                dye_color: None,
            }],
            work_ticks,
            subcomponent_records: vec![],
        })
    }

    // --- Display name helpers ---

    /// Get the component name for a starchy recipe's display.
    fn starchy_component_name(
        &self,
        params: &RecipeParams,
        fruit_species: &[crate::fruit::FruitSpecies],
    ) -> &'static str {
        self.find_component_name_by_property(
            params,
            fruit_species,
            crate::fruit::PartProperty::Starchy,
        )
    }

    /// Get the component name for a fine-fiber recipe's display.
    fn fine_fiber_component_name(
        &self,
        params: &RecipeParams,
        fruit_species: &[crate::fruit::FruitSpecies],
    ) -> &'static str {
        self.find_component_name_by_property(
            params,
            fruit_species,
            crate::fruit::PartProperty::FibrousFine,
        )
    }

    /// Get the component name for a coarse-fiber recipe's display.
    fn coarse_fiber_component_name(
        &self,
        params: &RecipeParams,
        fruit_species: &[crate::fruit::FruitSpecies],
    ) -> &'static str {
        self.find_component_name_by_property(
            params,
            fruit_species,
            crate::fruit::PartProperty::FibrousCoarse,
        )
    }

    /// Look up the extracted item kind display name for the first part with
    /// the given property on the species identified by the material param.
    fn find_component_name_by_property(
        &self,
        params: &RecipeParams,
        fruit_species: &[crate::fruit::FruitSpecies],
        property: crate::fruit::PartProperty,
    ) -> &'static str {
        if let Some(Material::FruitSpecies(id)) = params.material
            && let Some(species) = fruit_species.iter().find(|s| s.id == id)
            && let Some(part) = species
                .parts
                .iter()
                .find(|p| p.properties.contains(&property))
        {
            return part.part_type.extracted_item_kind().display_name();
        }
        "Component"
    }

    /// Get the component name and color name for a Press recipe's display.
    fn pigment_component_info(
        &self,
        params: &RecipeParams,
        fruit_species: &[crate::fruit::FruitSpecies],
    ) -> (&'static str, &'static str) {
        if let Some(Material::FruitSpecies(id)) = params.material
            && let Some(species) = fruit_species.iter().find(|s| s.id == id)
            && let Some(part) = species.parts.iter().find(|p| p.pigment.is_some())
        {
            let component = part.part_type.extracted_item_kind().display_name();
            let color = part.pigment.map_or("Unknown", |p| p.display_name());
            return (component, color);
        }
        ("Component", "Unknown")
    }
}

// ---------------------------------------------------------------------------
// Recipe enum tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod recipe_enum_tests {
    use super::*;
    use crate::config::GameConfig;
    use crate::fruit::{
        DyeColor, FruitAppearance, FruitColor, FruitPart, FruitShape, FruitSpecies, GrowthHabitat,
        PartProperty, PartType, Rarity,
    };
    use crate::types::FruitSpeciesId;

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

    fn pigmented_part(
        pt: PartType,
        props: &[PartProperty],
        pigment: DyeColor,
        units: u16,
    ) -> FruitPart {
        FruitPart {
            part_type: pt,
            properties: props.iter().copied().collect(),
            pigment: Some(pigment),
            component_units: units,
        }
    }

    fn make_params(material: Material) -> RecipeParams {
        RecipeParams {
            material: Some(material),
        }
    }

    // --- Serde roundtrip ---

    #[test]
    fn recipe_serde_roundtrip_all_variants() {
        for recipe in &ALL_RECIPES {
            let json = serde_json::to_string(recipe).unwrap();
            let parsed: Recipe = serde_json::from_str(&json).unwrap();
            assert_eq!(*recipe, parsed, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn recipe_serializes_as_variant_name() {
        let json = serde_json::to_string(&Recipe::Extract).unwrap();
        assert_eq!(json, "\"Extract\"");
        let json = serde_json::to_string(&Recipe::SewTunic).unwrap();
        assert_eq!(json, "\"SewTunic\"");
        let json = serde_json::to_string(&Recipe::GrowBow).unwrap();
        assert_eq!(json, "\"GrowBow\"");
    }

    // --- resolve() tests ---

    #[test]
    fn resolve_extract() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Starchi",
            vec![
                part(PartType::Flesh, &[PartProperty::Starchy], 40),
                part(PartType::Seed, &[], 10),
            ],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Extract
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs.len(), 1);
        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Fruit);
        assert_eq!(resolved.inputs[0].quantity, 1);
        assert_eq!(resolved.outputs.len(), 2);
        assert_eq!(resolved.work_ticks, config.extract_work_ticks);
    }

    #[test]
    fn resolve_extract_wrong_material_returns_none() {
        let config = GameConfig::default();
        let params = make_params(Material::Oak);
        assert!(Recipe::Extract.resolve(&params, &config, &[]).is_none());
    }

    #[test]
    fn resolve_extract_unknown_species_returns_none() {
        let config = GameConfig::default();
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(99)));
        assert!(Recipe::Extract.resolve(&params, &config, &[]).is_none());
    }

    #[test]
    fn resolve_mill() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Starchi",
            vec![part(PartType::Flesh, &[PartProperty::Starchy], 40)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Mill
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Pulp);
        assert_eq!(
            resolved.inputs[0].quantity,
            config.component_recipes.mill_input
        );
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Flour);
        assert_eq!(
            resolved.outputs[0].quantity,
            config.component_recipes.mill_output
        );
        assert_eq!(
            resolved.work_ticks,
            config.component_recipes.mill_work_ticks
        );
    }

    #[test]
    fn resolve_mill_seed_uses_seed_item_kind() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Nutfruit",
            vec![part(PartType::Seed, &[PartProperty::Starchy], 25)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Mill
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Seed);
    }

    #[test]
    fn resolve_mill_non_starchy_returns_none() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Sweetberry",
            vec![part(PartType::Flesh, &[PartProperty::Sweet], 40)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        assert!(Recipe::Mill.resolve(&params, &config, &species).is_none());
    }

    #[test]
    fn resolve_bake() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Starchi",
            vec![part(PartType::Flesh, &[PartProperty::Starchy], 40)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Bake
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Flour);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Bread);
        assert_eq!(
            resolved.work_ticks,
            config.component_recipes.bake_work_ticks
        );
    }

    #[test]
    fn resolve_spin() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Spin
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::FruitFiber);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Thread);
    }

    #[test]
    fn resolve_twist() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Ropevine",
            vec![part(PartType::Fiber, &[PartProperty::FibrousCoarse], 50)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Twist
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::FruitFiber);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Cord);
    }

    #[test]
    fn resolve_weave() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Weave
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Thread);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Cloth);
    }

    #[test]
    fn resolve_press() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Redpulp",
            vec![pigmented_part(PartType::Flesh, &[], DyeColor::Red, 50)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::Press
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Pulp);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Dye);
        assert!(resolved.outputs[0].dye_color.is_some());
    }

    #[test]
    fn resolve_press_non_pigmented_returns_none() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Plainberry",
            vec![part(PartType::Flesh, &[], 50)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        assert!(Recipe::Press.resolve(&params, &config, &species).is_none());
    }

    #[test]
    fn resolve_assemble_thread_bowstring() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::AssembleThreadBowstring
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Thread);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Bowstring);
    }

    #[test]
    fn resolve_assemble_cord_bowstring() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Ropevine",
            vec![part(PartType::Fiber, &[PartProperty::FibrousCoarse], 50)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::AssembleCordBowstring
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Cord);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Bowstring);
    }

    #[test]
    fn resolve_sew_tunic() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        let resolved = Recipe::SewTunic
            .resolve(&params, &config, &species)
            .expect("should resolve");

        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Cloth);
        assert_eq!(
            resolved.inputs[0].quantity,
            config.component_recipes.sew_tunic_input
        );
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Tunic);
    }

    #[test]
    fn resolve_sew_all_clothing() {
        let config = GameConfig::default();
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        for (recipe, expected_kind) in [
            (Recipe::SewTunic, ItemKind::Tunic),
            (Recipe::SewLeggings, ItemKind::Leggings),
            (Recipe::SewSandals, ItemKind::Sandals),
            (Recipe::SewShoes, ItemKind::Shoes),
            (Recipe::SewHat, ItemKind::Hat),
            (Recipe::SewGloves, ItemKind::Gloves),
        ] {
            let resolved = recipe
                .resolve(&params, &config, &species)
                .unwrap_or_else(|| panic!("{recipe:?} should resolve"));
            assert_eq!(resolved.outputs[0].item_kind, expected_kind);
        }
    }

    #[test]
    fn resolve_grow_bow() {
        let config = GameConfig::default();
        let params = make_params(Material::Oak);

        let resolved = Recipe::GrowBow
            .resolve(&params, &config, &[])
            .expect("should resolve");

        assert_eq!(resolved.inputs.len(), 1);
        assert_eq!(resolved.inputs[0].item_kind, ItemKind::Bowstring);
        assert_eq!(resolved.inputs[0].material_filter, MaterialFilter::Any);
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Bow);
        assert_eq!(resolved.outputs[0].material, Some(Material::Oak));
        assert_eq!(resolved.work_ticks, config.grow_recipes.grow_bow_work_ticks);
        assert_eq!(resolved.subcomponent_records.len(), 1);
    }

    #[test]
    fn resolve_grow_arrow() {
        let config = GameConfig::default();
        let params = make_params(Material::Yew);

        let resolved = Recipe::GrowArrow
            .resolve(&params, &config, &[])
            .expect("should resolve");

        assert!(resolved.inputs.is_empty());
        assert_eq!(resolved.outputs[0].item_kind, ItemKind::Arrow);
        assert_eq!(
            resolved.outputs[0].quantity,
            config.grow_recipes.grow_arrow_output
        );
        assert_eq!(resolved.outputs[0].material, Some(Material::Yew));
    }

    #[test]
    fn resolve_grow_armor_all_pieces() {
        let config = GameConfig::default();
        let params = make_params(Material::Birch);

        for (recipe, expected_kind) in [
            (Recipe::GrowHelmet, ItemKind::Helmet),
            (Recipe::GrowBreastplate, ItemKind::Breastplate),
            (Recipe::GrowGreaves, ItemKind::Greaves),
            (Recipe::GrowGauntlets, ItemKind::Gauntlets),
            (Recipe::GrowBoots, ItemKind::Boots),
        ] {
            let resolved = recipe
                .resolve(&params, &config, &[])
                .unwrap_or_else(|| panic!("{recipe:?} should resolve"));
            assert!(resolved.inputs.is_empty());
            assert_eq!(resolved.outputs[0].item_kind, expected_kind);
            assert_eq!(resolved.outputs[0].quantity, 1);
            assert_eq!(resolved.outputs[0].material, Some(Material::Birch));
        }
    }

    #[test]
    fn resolve_grow_rejects_non_wood() {
        let config = GameConfig::default();
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));

        assert!(Recipe::GrowBow.resolve(&params, &config, &[]).is_none());
        assert!(Recipe::GrowArrow.resolve(&params, &config, &[]).is_none());
        assert!(Recipe::GrowHelmet.resolve(&params, &config, &[]).is_none());
    }

    #[test]
    fn resolve_none_material_returns_none() {
        let config = GameConfig::default();
        let params = RecipeParams { material: None };

        for recipe in &ALL_RECIPES {
            assert!(
                recipe.resolve(&params, &config, &[]).is_none(),
                "{recipe:?} should return None for None material"
            );
        }
    }

    // --- valid_materials() tests ---

    #[test]
    fn valid_materials_wood_recipes() {
        let species = vec![test_species(
            0,
            "Starchi",
            vec![part(PartType::Flesh, &[PartProperty::Starchy], 40)],
        )];

        let mats = Recipe::GrowBow.valid_materials(&species);
        assert_eq!(mats.len(), 5);
        for m in &mats {
            assert!(m.is_wood());
        }
    }

    #[test]
    fn valid_materials_extract_returns_all_species() {
        let species = vec![
            test_species(0, "A", vec![part(PartType::Flesh, &[], 40)]),
            test_species(1, "B", vec![part(PartType::Seed, &[], 10)]),
        ];

        let mats = Recipe::Extract.valid_materials(&species);
        assert_eq!(mats.len(), 2);
    }

    #[test]
    fn valid_materials_mill_filters_starchy() {
        let species = vec![
            test_species(
                0,
                "Starchy",
                vec![part(PartType::Flesh, &[PartProperty::Starchy], 40)],
            ),
            test_species(
                1,
                "Sweet",
                vec![part(PartType::Flesh, &[PartProperty::Sweet], 40)],
            ),
        ];

        let mats = Recipe::Mill.valid_materials(&species);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0], Material::FruitSpecies(FruitSpeciesId(0)));
    }

    #[test]
    fn valid_materials_spin_filters_fine_fiber() {
        let species = vec![
            test_species(
                0,
                "Fine",
                vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
            ),
            test_species(
                1,
                "Coarse",
                vec![part(PartType::Fiber, &[PartProperty::FibrousCoarse], 30)],
            ),
        ];

        let mats = Recipe::Spin.valid_materials(&species);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0], Material::FruitSpecies(FruitSpeciesId(0)));
    }

    #[test]
    fn valid_materials_twist_filters_coarse_fiber() {
        let species = vec![
            test_species(
                0,
                "Fine",
                vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
            ),
            test_species(
                1,
                "Coarse",
                vec![part(PartType::Fiber, &[PartProperty::FibrousCoarse], 30)],
            ),
        ];

        let mats = Recipe::Twist.valid_materials(&species);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0], Material::FruitSpecies(FruitSpeciesId(1)));
    }

    #[test]
    fn valid_materials_press_filters_pigmented() {
        let species = vec![
            test_species(
                0,
                "Pigmented",
                vec![pigmented_part(PartType::Flesh, &[], DyeColor::Red, 50)],
            ),
            test_species(1, "Plain", vec![part(PartType::Flesh, &[], 50)]),
        ];

        let mats = Recipe::Press.valid_materials(&species);
        assert_eq!(mats.len(), 1);
        assert_eq!(mats[0], Material::FruitSpecies(FruitSpeciesId(0)));
    }

    // --- display_name() tests ---

    #[test]
    fn display_name_extract() {
        let species = vec![test_species(0, "Shinethúni", vec![])];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));
        assert_eq!(
            Recipe::Extract.display_name(&params, &species),
            "Extract Shinethúni"
        );
    }

    #[test]
    fn display_name_mill() {
        let species = vec![test_species(
            0,
            "Starchi",
            vec![part(PartType::Flesh, &[PartProperty::Starchy], 40)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));
        assert_eq!(
            Recipe::Mill.display_name(&params, &species),
            "Mill Starchi Pulp"
        );
    }

    #[test]
    fn display_name_grow_bow() {
        let params = make_params(Material::Oak);
        assert_eq!(Recipe::GrowBow.display_name(&params, &[]), "Grow Oak Bow");
    }

    #[test]
    fn display_name_sew_tunic() {
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));
        assert_eq!(
            Recipe::SewTunic.display_name(&params, &species),
            "Sew Silkweed Tunic"
        );
    }

    #[test]
    fn display_name_press() {
        let species = vec![test_species(
            0,
            "Redpulp",
            vec![pigmented_part(PartType::Flesh, &[], DyeColor::Red, 50)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));
        assert_eq!(
            Recipe::Press.display_name(&params, &species),
            "Press Redpulp Pulp Red Dye"
        );
    }

    #[test]
    fn display_name_assemble_thread_bowstring() {
        let species = vec![test_species(
            0,
            "Silkweed",
            vec![part(PartType::Fiber, &[PartProperty::FibrousFine], 30)],
        )];
        let params = make_params(Material::FruitSpecies(FruitSpeciesId(0)));
        assert_eq!(
            Recipe::AssembleThreadBowstring.display_name(&params, &species),
            "Silkweed Thread Bowstring"
        );
    }

    // --- furnishing_types() tests ---

    #[test]
    fn furnishing_types_kitchen() {
        for recipe in [Recipe::Extract, Recipe::Mill, Recipe::Bake, Recipe::Press] {
            assert_eq!(
                recipe.furnishing_types(),
                vec![FurnishingType::Kitchen],
                "{recipe:?} should be Kitchen"
            );
        }
    }

    #[test]
    fn furnishing_types_workshop() {
        for recipe in [
            Recipe::Spin,
            Recipe::Twist,
            Recipe::Weave,
            Recipe::AssembleThreadBowstring,
            Recipe::AssembleCordBowstring,
            Recipe::SewTunic,
            Recipe::GrowBow,
            Recipe::GrowArrow,
            Recipe::GrowHelmet,
        ] {
            assert_eq!(
                recipe.furnishing_types(),
                vec![FurnishingType::Workshop],
                "{recipe:?} should be Workshop"
            );
        }
    }

    // --- verb() tests ---

    #[test]
    fn verb_mapping() {
        assert_eq!(Recipe::Extract.verb(), RecipeVerb::Extract);
        assert_eq!(Recipe::Mill.verb(), RecipeVerb::Mill);
        assert_eq!(Recipe::Bake.verb(), RecipeVerb::Bake);
        assert_eq!(Recipe::Spin.verb(), RecipeVerb::Spin);
        assert_eq!(Recipe::Twist.verb(), RecipeVerb::Twist);
        assert_eq!(Recipe::Weave.verb(), RecipeVerb::Weave);
        assert_eq!(Recipe::Press.verb(), RecipeVerb::Press);
        assert_eq!(Recipe::AssembleThreadBowstring.verb(), RecipeVerb::Assemble);
        assert_eq!(Recipe::AssembleCordBowstring.verb(), RecipeVerb::Assemble);
        assert_eq!(Recipe::SewTunic.verb(), RecipeVerb::Sew);
        assert_eq!(Recipe::GrowBow.verb(), RecipeVerb::Grow);
    }

    // --- category() tests ---

    #[test]
    fn category_extraction() {
        assert_eq!(Recipe::Extract.category(), vec!["Extraction"]);
    }

    #[test]
    fn category_processing() {
        assert_eq!(Recipe::Mill.category(), vec!["Processing", "Milling"]);
        assert_eq!(Recipe::Bake.category(), vec!["Processing", "Baking"]);
        assert_eq!(Recipe::SewTunic.category(), vec!["Processing", "Tailoring"]);
    }

    #[test]
    fn category_woodcraft() {
        assert_eq!(Recipe::GrowBow.category(), vec!["Woodcraft", "Weapons"]);
        assert_eq!(Recipe::GrowHelmet.category(), vec!["Woodcraft", "Armor"]);
    }

    // --- has_material_param() ---

    #[test]
    fn all_recipes_have_material_param() {
        for recipe in &ALL_RECIPES {
            assert!(recipe.has_material_param(), "{recipe:?}");
        }
    }

    // --- required_species() ---

    #[test]
    fn all_recipes_require_elf() {
        for recipe in &ALL_RECIPES {
            assert_eq!(recipe.required_species(), Some(Species::Elf), "{recipe:?}");
        }
    }

    // --- Cross-check: resolve matches catalog for all generated fruits ---

    #[test]
    fn resolve_matches_catalog_output_items_for_generated_fruits() {
        use crate::fruit::generate_fruit_species;
        use elven_canopy_prng::GameRng;

        let config = GameConfig::default();

        for seed in 0..5 {
            let mut rng = GameRng::new(seed);
            let fruit_config = crate::config::FruitConfig::default();
            let fruits = generate_fruit_species(&mut rng, &fruit_config);

            // For each recipe variant, resolve with each valid material and
            // verify we get Some with the expected output item kind.
            for recipe in &ALL_RECIPES {
                for mat in recipe.valid_materials(&fruits) {
                    let params = make_params(mat);
                    let resolved = recipe.resolve(&params, &config, &fruits);
                    assert!(
                        resolved.is_some(),
                        "Seed {seed}: {recipe:?} with {mat:?} should resolve"
                    );
                    let resolved = resolved.unwrap();
                    assert!(
                        !resolved.outputs.is_empty(),
                        "Seed {seed}: {recipe:?} should have outputs"
                    );
                    for output in &resolved.outputs {
                        assert!(
                            output.quantity >= 1,
                            "Seed {seed}: {recipe:?} output {:?} has zero quantity",
                            output.item_kind,
                        );
                    }
                }
            }
        }
    }
}
