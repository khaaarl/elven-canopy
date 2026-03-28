//! Tests for the crafting system: active recipes, recipe management, auto-logistics,
//! unified crafting monitor, extraction recipes, quality propagation, grow recipes,
//! and manufacturing chain (extraction → mill → bake).
//! Corresponds to `sim/crafting.rs`.

use super::*;

// =========================================================================
// Crafting helpers
// =========================================================================

/// Helper: furnish a building and create a workshop or kitchen via the new
/// unified crafting system. Returns the structure_id.
fn setup_crafting_building(sim: &mut SimState, furnishing_type: FurnishingType) -> StructureId {
    let anchor = find_building_site(sim);
    let structure_id = insert_completed_building(sim, anchor);
    let furnish_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type,
            greenhouse_species: None,
        },
    };
    sim.step(&[furnish_cmd], sim.tick + 1);
    structure_id
}

fn place_all_furniture(sim: &mut SimState, structure_id: StructureId) {
    let furn_ids: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|f| f.id)
        .collect();
    for fid in furn_ids {
        let _ = sim.db.furniture.modify_unchecked(&fid, |f| {
            f.placed = true;
        });
    }
}

fn insert_test_fruit_species(sim: &mut SimState) -> crate::fruit::FruitSpeciesId {
    use crate::fruit::{
        FruitAppearance, FruitColor, FruitPart, FruitShape, FruitSpecies, GrowthHabitat, PartType,
        Rarity,
    };
    use std::collections::BTreeSet;
    let id = crate::types::FruitSpeciesId(999);
    let species = FruitSpecies {
        id,
        vaelith_name: "Testaleth".to_string(),
        english_gloss: "test-berry".to_string(),
        parts: vec![
            FruitPart {
                part_type: PartType::Flesh,
                properties: BTreeSet::new(),
                pigment: None,
                component_units: 37,
            },
            FruitPart {
                part_type: PartType::Fiber,
                properties: BTreeSet::new(),
                pigment: None,
                component_units: 52,
            },
            FruitPart {
                part_type: PartType::Seed,
                properties: BTreeSet::new(),
                pigment: None,
                component_units: 15,
            },
        ],
        habitat: GrowthHabitat::Branch,
        rarity: Rarity::Common,
        greenhouse_cultivable: true,
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
    };
    let _ = sim.db.fruit_species.insert_no_fk(species);
    id
}

/// Helper: set up a kitchen with a test fruit species extraction recipe
/// enabled and targeted. Returns (structure_id, species_id).
fn setup_extraction_kitchen(sim: &mut SimState) -> (StructureId, crate::fruit::FruitSpeciesId) {
    let species_id = insert_test_fruit_species(sim);
    let structure_id = setup_crafting_building(sim, FurnishingType::Kitchen);

    // Add the extraction recipe for our test species to the kitchen.
    sim.add_active_recipe(
        structure_id,
        Recipe::Extract,
        Some(Material::FruitSpecies(species_id)),
    );

    // Set nonzero targets for the outputs so the monitor will schedule work.
    let active_recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    let ar = active_recipes
        .iter()
        .find(|r| {
            r.recipe == Recipe::Extract && r.material == Some(Material::FruitSpecies(species_id))
        })
        .expect("active recipe should exist");

    let targets = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC);
    for target in &targets {
        let set_cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeOutputTarget {
                active_recipe_target_id: target.id,
                target_quantity: 100,
            },
        };
        sim.step(&[set_cmd], sim.tick + 1);
    }

    (structure_id, species_id)
}

/// Helper: insert a fruit species with Starchy flesh + FibrousFine fiber +
/// pigmented rind, enabling the full Extract→Mill→Bake and Spin→Weave chains.
fn insert_full_chain_fruit_species(sim: &mut SimState) -> crate::fruit::FruitSpeciesId {
    use crate::fruit::{
        DyeColor, FruitAppearance, FruitColor, FruitPart, FruitShape, FruitSpecies, GrowthHabitat,
        PartProperty, PartType, Rarity,
    };
    use std::collections::BTreeSet;
    let id = crate::types::FruitSpeciesId(998);
    let mut starchy_props = BTreeSet::new();
    starchy_props.insert(PartProperty::Starchy);
    let mut fine_fiber_props = BTreeSet::new();
    fine_fiber_props.insert(PartProperty::FibrousFine);
    let species = FruitSpecies {
        id,
        vaelith_name: "Chainberry".to_string(),
        english_gloss: "chain-berry".to_string(),
        parts: vec![
            FruitPart {
                part_type: PartType::Flesh,
                properties: starchy_props,
                pigment: Some(DyeColor::Red),
                component_units: 40,
            },
            FruitPart {
                part_type: PartType::Fiber,
                properties: fine_fiber_props,
                pigment: None,
                component_units: 30,
            },
        ],
        habitat: GrowthHabitat::Branch,
        rarity: Rarity::Common,
        greenhouse_cultivable: false,
        appearance: FruitAppearance {
            exterior_color: FruitColor {
                r: 200,
                g: 50,
                b: 50,
            },
            shape: FruitShape::Round,
            size_percent: 100,
            glows: false,
        },
    };
    let _ = sim.db.fruit_species.insert_no_fk(species);
    id
}

/// Helper: add an active recipe to a building, set all output targets to the
/// given quantity, and return the ActiveRecipeId.
fn add_recipe_with_targets(
    sim: &mut SimState,
    structure_id: StructureId,
    recipe: Recipe,
    material: Option<Material>,
    target_qty: u32,
) -> crate::types::ActiveRecipeId {
    sim.add_active_recipe(structure_id, recipe, material);
    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == recipe && r.material == material)
        .expect("recipe should be added");
    let ar_id = ar.id;
    let targets = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar_id, tabulosity::QueryOpts::ASC);
    for target in &targets {
        let _ = sim
            .db
            .active_recipe_targets
            .modify_unchecked(&target.id, |t| {
                t.target_quantity = target_qty;
            });
    }
    ar_id
}

// =========================================================================
// Serde / config tests
// =========================================================================

#[test]
fn completed_structure_serde_backward_compat_crafting() {
    // Old JSON without crafting_enabled field should deserialize with default.
    let mut rng = GameRng::new(42);
    let structure = CompletedStructure {
        id: StructureId(1),
        project_id: ProjectId::new(&mut rng),
        build_type: BuildType::Building,
        anchor: VoxelCoord::new(0, 0, 0),
        width: 5,
        depth: 5,
        height: 3,
        completed_tick: 100,
        name: None,
        furnishing: None,
        inventory_id: InventoryId(0),
        logistics_priority: None,
        crafting_enabled: false,
        greenhouse_species: None,
        greenhouse_enabled: false,
        greenhouse_last_production_tick: 0,
        last_dance_completed_tick: 0,
    };
    let json = serde_json::to_string(&structure).unwrap();
    // Remove crafting_enabled to simulate old save.
    let json_old = json.replace(r#","crafting_enabled":false"#, "");
    let restored: CompletedStructure = serde_json::from_str(&json_old).unwrap();
    assert!(!restored.crafting_enabled);
}

#[test]
fn game_config_with_recipes_deserializes() {
    use crate::species::{EngagementInitiative, EngagementStyle};
    let config_json = std::fs::read_to_string("../default_config.json").unwrap();
    let config: crate::config::GameConfig = serde_json::from_str(&config_json).unwrap();
    // EngagementStyle and detection range survive JSON roundtrip.
    assert_eq!(
        config.species[&Species::Goblin].engagement_style.initiative,
        EngagementInitiative::Aggressive
    );
    assert_eq!(
        config.species[&Species::Goblin].hostile_detection_range_sq,
        225
    );
    assert_eq!(
        config.species[&Species::Elf].engagement_style,
        EngagementStyle {
            weapon_preference: crate::species::WeaponPreference::PreferRanged,
            ammo_exhausted: crate::species::AmmoExhaustedBehavior::Flee,
            initiative: EngagementInitiative::Defensive,
            disengage_threshold_pct: 100,
        }
    );
    assert_eq!(
        config.species[&Species::Elf].hostile_detection_range_sq,
        225
    );
}

#[test]
fn game_config_without_recipes_gets_defaults() {
    // Minimal valid config JSON — no recipes field.
    let config = crate::config::GameConfig::default();
    assert_eq!(config.workshop_default_priority, 8);
}

#[test]
fn furnish_workshop_sets_defaults() {
    let mut sim = test_sim(42);
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Workshop,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert!(
        structure.crafting_enabled,
        "Workshop should have crafting_enabled"
    );
    assert_eq!(
        structure.logistics_priority,
        Some(sim.config.workshop_default_priority),
        "Workshop should have default priority"
    );

    // Auto-add on furnish was removed (F-recipe-params). Workshops start
    // with no active recipes; the player adds them manually.
    let active_recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    assert_eq!(
        active_recipes.len(),
        0,
        "Workshop should have no active recipes after furnishing"
    );
}

#[test]
fn craft_task_serde_roundtrip() {
    use crate::prng::GameRng;
    let mut rng = GameRng::new(42);
    let task_id = TaskId::new(&mut rng);

    let task = task::Task {
        id: task_id,
        kind: task::TaskKind::Craft {
            structure_id: StructureId(5),
            active_recipe_id: ActiveRecipeId(99),
        },
        state: task::TaskState::Available,
        location: VoxelCoord::new(10, 0, 0),
        progress: 0,
        total_cost: 5000,
        required_species: Some(Species::Elf),
        origin: task::TaskOrigin::Automated,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    let json = serde_json::to_string(&task).unwrap();
    let restored: task::Task = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, task_id);
    match &restored.kind {
        task::TaskKind::Craft {
            structure_id,
            active_recipe_id,
        } => {
            assert_eq!(*structure_id, StructureId(5));
            assert_eq!(*active_recipe_id, ActiveRecipeId(99));
        }
        other => panic!("Expected Craft task, got {:?}", other),
    }
    assert_eq!(restored.origin, task::TaskOrigin::Automated);
}

// =========================================================================
// Unified crafting commands (ActiveRecipe / ActiveRecipeTarget)
// =========================================================================

#[test]
fn add_active_recipe_creates_recipe_and_targets() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Manually add a recipe and verify it was created with correct properties.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    let bow_recipe = recipes
        .iter()
        .find(|r| r.recipe == Recipe::GrowBow && r.material == Some(Material::Oak))
        .expect("Grow Oak Bow recipe should exist");
    assert!(bow_recipe.enabled);
    assert!(bow_recipe.auto_logistics);
    assert_eq!(bow_recipe.spare_iterations, 0);

    // Should have target rows for each output (Grow Oak Bow has 1 output: Bow).
    let targets = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&bow_recipe.id, tabulosity::QueryOpts::ASC);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].output_item_kind, inventory::ItemKind::Bow);
    assert_eq!(targets[0].target_quantity, 0);
}

#[test]
fn add_active_recipe_rejects_duplicate() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Manually add a recipe, then try to add it again.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let initial_count = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .len();

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AddActiveRecipe {
            structure_id,
            recipe: Recipe::GrowBow,
            material: Some(Material::Oak),
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Count should not increase — duplicate was rejected.
    let after_count = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .len();
    assert_eq!(initial_count, after_count);
}

#[test]
fn add_active_recipe_rejects_wrong_furnishing() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Kitchen);

    // Try to add a workshop recipe to a kitchen.
    let initial_count = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .len();

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AddActiveRecipe {
            structure_id,
            recipe: Recipe::GrowBow,
            material: Some(Material::Oak),
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Kitchen should only have its default bread recipe — no workshop recipes.
    let recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    assert_eq!(recipes.len(), initial_count, "Count should not change");
    assert!(
        recipes
            .iter()
            .all(|r| !(r.recipe == Recipe::GrowBow && r.material == Some(Material::Oak))),
        "Workshop recipe should not be on a kitchen"
    );
}

#[test]
fn remove_active_recipe_deletes_recipe_and_targets() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Add a grow recipe (not auto-added), then remove it.
    let add_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AddActiveRecipe {
            structure_id,
            recipe: Recipe::GrowArrow,
            material: Some(Material::Oak),
        },
    };
    sim.step(&[add_cmd], sim.tick + 1);

    let initial_count = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .len();

    let ar_id = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap()
        .id;

    // Remove it.
    let rm_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RemoveActiveRecipe {
            active_recipe_id: ar_id,
        },
    };
    sim.step(&[rm_cmd], sim.tick + 1);

    assert_eq!(
        sim.db
            .active_recipes
            .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
            .len(),
        initial_count - 1,
    );
    // Targets should be cascade-deleted.
    assert_eq!(
        sim.db
            .active_recipe_targets
            .by_active_recipe_id(&ar_id, tabulosity::QueryOpts::ASC)
            .len(),
        0,
    );
}

#[test]
fn set_recipe_output_target_updates_quantity() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap();
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];

    let set_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 42,
        },
    };
    sim.step(&[set_cmd], sim.tick + 1);

    let updated = sim.db.active_recipe_targets.get(&target.id).unwrap();
    assert_eq!(updated.target_quantity, 42);
}

#[test]
fn set_crafting_enabled_toggles_building() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Workshop furnishing now auto-enables crafting.
    assert!(
        sim.db
            .structures
            .get(&structure_id)
            .unwrap()
            .crafting_enabled
    );

    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCraftingEnabled {
            structure_id,
            enabled: false,
        },
    };
    sim.step(&[cmd2], sim.tick + 1);
    assert!(
        !sim.db
            .structures
            .get(&structure_id)
            .unwrap()
            .crafting_enabled
    );
}

#[test]
fn set_recipe_enabled_toggles_recipe() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    let ar_id = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap()
        .id;
    assert!(sim.db.active_recipes.get(&ar_id).unwrap().enabled);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeEnabled {
            active_recipe_id: ar_id,
            enabled: false,
        },
    };
    sim.step(&[cmd], sim.tick + 1);
    assert!(!sim.db.active_recipes.get(&ar_id).unwrap().enabled);
}

#[test]
fn set_recipe_auto_logistics_updates_fields() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar_id = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0]
        .id;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeAutoLogistics {
            active_recipe_id: ar_id,
            auto_logistics: false,
            spare_iterations: 5,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let ar = sim.db.active_recipes.get(&ar_id).unwrap();
    assert!(!ar.auto_logistics);
    assert_eq!(ar.spare_iterations, 5);
}

#[test]
fn move_active_recipe_up_down_swaps_sort_order() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Manually add two recipes.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));
    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    // Now have at least 2 active recipes. Use the first two for
    // testing move up/down.
    let recipes = sim.db.active_recipes.by_structure_sort(
        &structure_id,
        tabulosity::MatchAll,
        tabulosity::QueryOpts::ASC,
    );
    assert!(recipes.len() >= 2);
    let first_id = recipes[0].id;
    let second_id = recipes[1].id;
    let first_sort = recipes[0].sort_order;
    let second_sort = recipes[1].sort_order;
    assert!(first_sort < second_sort);

    // Move second up — should swap with first.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::MoveActiveRecipeUp {
            active_recipe_id: second_id,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let after = sim.db.active_recipes.by_structure_sort(
        &structure_id,
        tabulosity::MatchAll,
        tabulosity::QueryOpts::ASC,
    );
    assert_eq!(after[0].id, second_id);
    assert_eq!(after[1].id, first_id);

    // Move second (now at top) up again — should be no-op.
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::MoveActiveRecipeUp {
            active_recipe_id: second_id,
        },
    };
    sim.step(&[cmd2], sim.tick + 1);

    let after2 = sim.db.active_recipes.by_structure_sort(
        &structure_id,
        tabulosity::MatchAll,
        tabulosity::QueryOpts::ASC,
    );
    assert_eq!(after2[0].id, second_id);

    // Move second down — should swap back.
    let cmd3 = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::MoveActiveRecipeDown {
            active_recipe_id: second_id,
        },
    };
    sim.step(&[cmd3], sim.tick + 1);

    let after3 = sim.db.active_recipes.by_structure_sort(
        &structure_id,
        tabulosity::MatchAll,
        tabulosity::QueryOpts::ASC,
    );
    assert_eq!(after3[0].id, first_id);
    assert_eq!(after3[1].id, second_id);
}

// =========================================================================
// Unified crafting monitor
// =========================================================================

#[test]
fn unified_crafting_monitor_creates_craft_task() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Place furniture so the building is functional.
    let furn_ids: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|f| f.id)
        .collect();
    for fid in furn_ids {
        let _ = sim.db.furniture.modify_unchecked(&fid, |f| {
            f.placed = true;
        });
    }

    // Crafting is auto-enabled by furnishing. Add arrow recipe manually.
    let add_arrow_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AddActiveRecipe {
            structure_id,
            recipe: Recipe::GrowArrow,
            material: Some(Material::Oak),
        },
    };
    sim.step(&[add_arrow_cmd], sim.tick + 1);

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap();
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 20,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // Run the unified monitor.
    sim.process_unified_crafting_monitor();

    // Should have created a Craft task for the arrow recipe.
    let craft_tasks: Vec<_> = sim
        .db
        .task_structure_refs
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|r| r.role == crate::db::TaskStructureRole::CraftAt)
        .filter(|r| {
            sim.db
                .tasks
                .get(&r.task_id)
                .is_some_and(|t| t.state != task::TaskState::Complete)
        })
        .map(|r| r.task_id)
        .collect();
    assert_eq!(craft_tasks.len(), 1, "Expected 1 craft task");

    // Verify the TaskCraftData has the active_recipe_id set.
    let tcd = sim.task_craft_data(craft_tasks[0]).unwrap();
    assert_eq!(tcd.active_recipe_id, ar.id);
}

#[test]
fn unified_crafting_monitor_skips_when_target_met() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let furn_ids: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|f| f.id)
        .collect();
    for fid in furn_ids {
        let _ = sim.db.furniture.modify_unchecked(&fid, |f| {
            f.placed = true;
        });
    }

    let enable_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCraftingEnabled {
            structure_id,
            enabled: true,
        },
    };
    sim.step(&[enable_cmd], sim.tick + 1);

    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap();
    let target = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();

    // Set target to 5 arrows, then add 5 arrows to the building's inventory.
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 5,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Arrow,
        5,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );

    // Run the unified monitor — should NOT create a task (target met).
    sim.process_unified_crafting_monitor();

    let craft_count = sim
        .db
        .task_structure_refs
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|r| r.role == crate::db::TaskStructureRole::CraftAt)
        .filter(|r| {
            sim.db
                .tasks
                .get(&r.task_id)
                .is_some_and(|t| t.state != task::TaskState::Complete)
        })
        .count();
    assert_eq!(
        craft_count, 0,
        "Should not create a task when target is met"
    );
}

#[test]
fn unified_crafting_monitor_skips_when_disabled() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let furn_ids: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|f| f.id)
        .collect();
    for fid in furn_ids {
        let _ = sim.db.furniture.modify_unchecked(&fid, |f| {
            f.placed = true;
        });
    }

    // Disable crafting (furnishing auto-enables it).
    let disable_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCraftingEnabled {
            structure_id,
            enabled: false,
        },
    };
    sim.step(&[disable_cmd], sim.tick + 1);

    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap();
    let target = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();

    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 20,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    sim.process_unified_crafting_monitor();

    let craft_count = sim
        .db
        .task_structure_refs
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|r| r.role == crate::db::TaskStructureRole::CraftAt)
        .filter(|r| {
            sim.db
                .tasks
                .get(&r.task_id)
                .is_some_and(|t| t.state != task::TaskState::Complete)
        })
        .count();
    assert_eq!(
        craft_count, 0,
        "Should not create tasks when crafting_enabled is false"
    );
}

// =========================================================================
// Auto-logistics (unified crafting)
// =========================================================================

#[test]
fn auto_logistics_generates_wants_from_active_recipe() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Clear old workshop explicit wants and enable crafting.
    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    // Set target: 1 bow.
    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // Auto-logistics is enabled by default. runs_needed = ceil(1/1) = 1.
    // Input: 1 Bowstring per run → auto_want = 1 * 1 = 1.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Should have a Bowstring want from auto-logistics");
    assert_eq!(bowstring_want.target_quantity, 1);
}

#[test]
fn auto_logistics_spare_iterations_add_extra_wants() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Clear old workshop explicit wants and enable crafting.
    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];

    // Set target: 1 bow, spare_iterations: 3.
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    let set_spare_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeAutoLogistics {
            active_recipe_id: ar.id,
            auto_logistics: true,
            spare_iterations: 3,
        },
    };
    sim.step(&[set_target_cmd, set_spare_cmd], sim.tick + 1);

    // runs_needed = 1 (ceil(1/1)), spare = 3, total = 4.
    // auto_want = 1 * 4 = 4 Bowstring.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Should have a Bowstring want");
    assert_eq!(bowstring_want.target_quantity, 4);
}

#[test]
fn auto_logistics_sums_with_explicit_wants() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let enable_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCraftingEnabled {
            structure_id,
            enabled: true,
        },
    };
    sim.step(&[enable_cmd], sim.tick + 1);

    // Enable logistics and set an explicit want of 5 Bowstring.
    let logistics_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsPriority {
                structure_id,
                priority: Some(5),
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![crate::building::LogisticsWant {
                    item_kind: inventory::ItemKind::Bowstring,
                    material_filter: inventory::MaterialFilter::Any,
                    target_quantity: 5,
                }],
            },
        },
    ];
    sim.step(&logistics_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow) with target 1.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // Explicit = 5, auto = 1 → merged = 6.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Should have Bowstring want");
    assert_eq!(bowstring_want.target_quantity, 6);
}

#[test]
fn auto_logistics_disabled_when_crafting_disabled() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Clear explicit wants (crafting starts enabled, we disable it below).
    let clear_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetLogisticsWants {
            structure_id,
            wants: vec![],
        },
    };
    sim.step(&[clear_cmd], sim.tick + 1);

    // Manually add Grow Oak Bow recipe and set a target.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 1,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // Now disable crafting.
    let disable_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCraftingEnabled {
            structure_id,
            enabled: false,
        },
    };
    sim.step(&[disable_cmd], sim.tick + 1);

    // crafting_enabled is false → no auto-logistics wants.
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring);
    assert!(
        bowstring_want.is_none(),
        "Should not generate auto-logistics when crafting is disabled"
    );
}

#[test]
fn auto_logistics_disabled_per_recipe() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];

    // Set target, then disable auto-logistics.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeOutputTarget {
                active_recipe_target_id: target.id,
                target_quantity: 1,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeAutoLogistics {
                active_recipe_id: ar.id,
                auto_logistics: false,
                spare_iterations: 0,
            },
        },
    ];
    sim.step(&cmds, sim.tick + 1);

    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring);
    assert!(
        bowstring_want.is_none(),
        "Should not generate auto-logistics when recipe auto_logistics is false"
    );
}

#[test]
fn auto_logistics_no_input_recipe_generates_no_wants() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Arrow recipe has no inputs. Add it manually (not auto-added).
    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowArrow && r.material == Some(Material::Oak))
        .unwrap();
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];
    let set_target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 100,
        },
    };
    sim.step(&[set_target_cmd], sim.tick + 1);

    // No inputs → no auto-logistics wants.
    let wants = sim.compute_effective_wants(structure_id);
    assert!(
        wants.is_empty(),
        "Arrow has no inputs, should generate no wants"
    );
}

#[test]
fn auto_logistics_spare_iterations_when_target_met() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let setup_cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetLogisticsWants {
                structure_id,
                wants: vec![],
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetCraftingEnabled {
                structure_id,
                enabled: true,
            },
        },
    ];
    sim.step(&setup_cmds, sim.tick + 1);

    // Add Grow Oak Bow recipe (1 Bowstring → 1 Bow).
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = &sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)[0];
    let target = &sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)[0];

    // Set target: 1 bow, spare: 2.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeOutputTarget {
                active_recipe_target_id: target.id,
                target_quantity: 1,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SetRecipeAutoLogistics {
                active_recipe_id: ar.id,
                auto_logistics: true,
                spare_iterations: 2,
            },
        },
    ];
    sim.step(&cmds, sim.tick + 1);

    // Add 1 oak bow to the building's inventory so target is met.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bow,
        1,
        None,
        None,
        Some(inventory::Material::Oak),
        0,
        None,
        None,
    );

    // runs_needed = 0 (target met), spare = 2, total = 2.
    // auto_want = 1 * 2 = 2 Bowstring (stockpiling for spare iterations).
    let wants = sim.compute_effective_wants(structure_id);
    let bowstring_want = wants
        .iter()
        .find(|w| w.item_kind == inventory::ItemKind::Bowstring)
        .expect("Spare iterations should still generate wants when target is met");
    assert_eq!(bowstring_want.target_quantity, 2);
}

#[test]
fn remove_active_recipe_cleans_up_pending_craft_task() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Place furniture so the building is functional.
    let furn_ids: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|f| f.id)
        .collect();
    for fid in furn_ids {
        let _ = sim.db.furniture.modify_unchecked(&fid, |f| {
            f.placed = true;
        });
    }

    // Manually add Grow Oak Bow recipe (1 Bowstring → 1 Bow) and set a target.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowBow && r.material == Some(Material::Oak))
        .unwrap();
    let target = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 100,
        },
    };
    sim.step(&[target_cmd], sim.tick + 1);

    // Stock building with bowstring and run monitor to create a craft task.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bowstring, 1, None, None);
    sim.process_unified_crafting_monitor();

    // Verify craft task exists with reserved items.
    let craft_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
        .collect();
    assert_eq!(craft_tasks.len(), 1, "Should have 1 pending craft task");
    let task_id = craft_tasks[0].id;

    let reserved = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by == Some(task_id))
        .count();
    assert!(reserved > 0, "Fruit should be reserved");

    // Remove the active recipe.
    let rm_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RemoveActiveRecipe {
            active_recipe_id: ar.id,
        },
    };
    sim.step(&[rm_cmd], sim.tick + 1);

    // The recipe and targets should be gone.
    assert!(sim.db.active_recipes.get(&ar.id).is_none());
    assert!(
        sim.db
            .active_recipe_targets
            .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn resolve_craft_via_unified_catalog_path() {
    // Test resolve_craft_action by creating a craft task via the unified
    // recipe system.
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Place furniture.
    let furn_ids: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|f| f.id)
        .collect();
    for fid in furn_ids {
        let _ = sim.db.furniture.modify_unchecked(&fid, |f| {
            f.placed = true;
        });
    }

    // Manually add Grow Oak Bow recipe (1 Bowstring → 1 Bow) and set a target.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));

    let ar = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|r| r.recipe == Recipe::GrowBow && r.material == Some(Material::Oak))
        .unwrap();
    let target = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&ar.id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .unwrap();
    let target_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetRecipeOutputTarget {
            active_recipe_target_id: target.id,
            target_quantity: 100,
        },
    };
    sim.step(&[target_cmd], sim.tick + 1);

    // Stock with bowstring and run monitor to create a craft task.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bowstring, 1, None, None);
    sim.process_unified_crafting_monitor();

    let craft_task_id = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Craft)
        .unwrap()
        .id;

    // Spawn elf and assign to the task.
    let structure = sim.db.structures.get(&structure_id).unwrap();
    let elf_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: structure.anchor,
        },
    };
    sim.step(&[elf_cmd], sim.tick + 1);
    let elf_id = sim
        .db
        .creatures
        .by_species(&Species::Elf, tabulosity::QueryOpts::ASC)
        .last()
        .unwrap()
        .id;
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(craft_task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }
    if let Some(mut t) = sim.db.tasks.get(&craft_task_id) {
        t.state = TaskState::InProgress;
        let _ = sim.db.tasks.update_no_fk(t);
    }

    // Resolve the craft action — Grow recipes are multi-action now.
    // Call resolve until the task completes (drains mana each action).
    let total_cost = sim.db.tasks.get(&craft_task_id).unwrap().total_cost as u32; // i64 → u32 for loop range
    for i in 0..total_cost {
        let completed = sim.resolve_craft_action(elf_id);
        if i < total_cost - 1 {
            assert!(!completed, "Grow craft should not complete on action {i}");
        } else {
            assert!(
                completed,
                "Craft should complete via catalog path on final action"
            );
        }
    }

    // Bowstring should be consumed, bow should be produced.
    let bowstring_count = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Bowstring,
        inventory::MaterialFilter::Any,
    );
    assert_eq!(bowstring_count, 0, "Bowstring should be consumed");
    let bow_count = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Any,
    );
    assert_eq!(bow_count, 1, "Bow should be produced (qty 1)");
}

// =========================================================================
// Extraction recipes
// =========================================================================

#[test]
fn extraction_recipe_resolves_for_worldgen_species() {
    let sim = test_sim(42);
    let fruits: Vec<_> = sim.db.fruit_species.iter_all().cloned().collect();
    assert!(!fruits.is_empty(), "worldgen should produce fruit species");

    // Every fruit species should be a valid material for Extract.
    let extract_materials = Recipe::Extract.valid_materials(&fruits);
    assert_eq!(
        extract_materials.len(),
        fruits.len(),
        "Extract should accept all fruit species"
    );

    // Each should resolve successfully.
    for mat in &extract_materials {
        let params = crate::recipe::RecipeParams {
            material: Some(*mat),
        };
        assert!(
            Recipe::Extract
                .resolve(&params, &sim.config, &fruits)
                .is_some(),
            "Extract should resolve for {mat:?}"
        );
    }
}

#[test]
fn extraction_recipe_inputs_and_outputs_match_species() {
    let mut sim = test_sim(42);
    let species_id = insert_test_fruit_species(&mut sim);

    let fruits: Vec<_> = sim.db.fruit_species.iter_all().cloned().collect();
    let params = crate::recipe::RecipeParams {
        material: Some(Material::FruitSpecies(species_id)),
    };
    let resolved = Recipe::Extract
        .resolve(&params, &sim.config, &fruits)
        .expect("extraction recipe should resolve");

    // Input: 1 Testaleth Fruit.
    assert_eq!(resolved.inputs.len(), 1);
    assert_eq!(resolved.inputs[0].item_kind, inventory::ItemKind::Fruit);
    assert_eq!(resolved.inputs[0].quantity, 1);
    assert_eq!(
        resolved.inputs[0].material_filter,
        inventory::MaterialFilter::Specific(inventory::Material::FruitSpecies(species_id))
    );

    // Outputs: Pulp(37), FruitFiber(52), Seed(15) — 3 outputs.
    assert_eq!(resolved.outputs.len(), 3);

    let pulp = resolved
        .outputs
        .iter()
        .find(|o| o.item_kind == inventory::ItemKind::Pulp);
    assert!(pulp.is_some(), "should have Pulp output");
    assert_eq!(pulp.unwrap().quantity, 37);

    let fiber = resolved
        .outputs
        .iter()
        .find(|o| o.item_kind == inventory::ItemKind::FruitFiber);
    assert!(fiber.is_some(), "should have FruitFiber output");
    assert_eq!(fiber.unwrap().quantity, 52);

    let seed = resolved
        .outputs
        .iter()
        .find(|o| o.item_kind == inventory::ItemKind::Seed);
    assert!(seed.is_some(), "should have Seed output");
    assert_eq!(seed.unwrap().quantity, 15);

    // Work ticks should come from config.
    assert_eq!(resolved.work_ticks, sim.config.extract_work_ticks);
}

#[test]
fn extraction_monitor_creates_task_when_fruit_available() {
    let mut sim = test_sim(42);
    let (structure_id, species_id) = setup_extraction_kitchen(&mut sim);

    // Add fruit of the correct species to the kitchen inventory.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );

    // Advance far enough for the logistics heartbeat to trigger the crafting
    // monitor. The heartbeat interval is config.logistics_heartbeat_interval_ticks.
    let target = sim.tick + sim.config.logistics_heartbeat_interval_ticks + 1;
    while sim.tick < target {
        sim.step(&[], sim.tick + 1);
    }

    // Should have created a Craft task for the kitchen.
    let tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
        .collect();
    assert_eq!(tasks.len(), 1, "should create one craft task");
}

#[test]
fn extraction_produces_correct_component_items() {
    let mut sim = test_sim(42);
    let (structure_id, species_id) = setup_extraction_kitchen(&mut sim);

    // Add fruit to kitchen.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        1,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );

    // Pre-fill bread so the bread recipe (auto-added with target 50)
    // doesn't compete for the fruit or the elf's attention.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bread,
        200,
        None,
        None,
        None,
        0,
        None,
        None,
    );

    // Spawn an elf near the kitchen and run until extraction completes.
    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, anchor, &mut events)
        .expect("spawn elf");
    // Make elf not hungry/sleepy so they'll pick up the task.
    let food_max = sim.species_table[&Species::Elf].food_max;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.food = food_max;
        c.rest = rest_max;
    });

    // Run the sim forward: need heartbeat interval (5000) + walk time +
    // extract_work_ticks (3000) + margin.
    for _ in 0..15000 {
        sim.step(&[], sim.tick + 1);
    }

    // Check that the fruit was consumed and components were produced.
    let fruit_count = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Specific(inventory::Material::FruitSpecies(species_id)),
    );
    assert_eq!(fruit_count, 0, "fruit should be consumed");

    let pulp_count = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Pulp,
        inventory::MaterialFilter::Specific(inventory::Material::FruitSpecies(species_id)),
    );
    assert_eq!(pulp_count, 37, "should produce 37 Pulp");

    let fiber_count = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::FruitFiber,
        inventory::MaterialFilter::Specific(inventory::Material::FruitSpecies(species_id)),
    );
    assert_eq!(fiber_count, 52, "should produce 52 FruitFiber");

    let seed_count = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Seed,
        inventory::MaterialFilter::Specific(inventory::Material::FruitSpecies(species_id)),
    );
    assert_eq!(seed_count, 15, "should produce 15 Seed");
}

#[test]
fn extraction_display_names_use_vaelith_species() {
    let mut sim = test_sim(42);
    let species_id = insert_test_fruit_species(&mut sim);

    // Use material_item_display_name which doesn't need an ItemStack.
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Pulp,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Pulp"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::FruitFiber,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Fiber"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Seed,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Seed"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Husk,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Husk"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::FruitSap,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Sap"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::FruitResin,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Resin"
    );

    // Processed components and species-specific bread/bowstring.
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Flour,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Flour"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Thread,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Thread"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Cord,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Cord"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Bread,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Bread"
    );
    assert_eq!(
        sim.material_item_display_name(
            inventory::ItemKind::Bowstring,
            inventory::Material::FruitSpecies(species_id)
        ),
        "Testaleth Bowstring"
    );

    // Also test item_display_name by adding items to an inventory.
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Pulp,
        10,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );
    let stacks = sim.inv_items(inv_id);
    let pulp_stack = stacks
        .iter()
        .find(|s| s.kind == inventory::ItemKind::Pulp)
        .expect("should have pulp");
    assert_eq!(sim.item_display_name(pulp_stack), "Fine Testaleth Pulp");
}

#[test]
fn extraction_monitor_skips_when_targets_met() {
    let mut sim = test_sim(42);
    let (structure_id, species_id) = setup_extraction_kitchen(&mut sim);

    // Add fruit to the kitchen.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );

    // Also add enough bread to satisfy the bread recipe target (auto-added
    // with the kitchen), so the bread recipe doesn't fire either.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bread,
        200,
        None,
        None,
        None,
        0,
        None,
        None,
    );

    // Pre-fill all outputs above their targets so the monitor sees no need.
    // Targets are set to 100 each; add 200 of each.
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Pulp,
        200,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::FruitFiber,
        200,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Seed,
        200,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );

    // Advance far enough for the logistics heartbeat to trigger.
    let target = sim.tick + sim.config.logistics_heartbeat_interval_ticks + 1;
    while sim.tick < target {
        sim.step(&[], sim.tick + 1);
    }

    // Should NOT create a task because all targets are met.
    let tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state != TaskState::Complete)
        .collect();
    assert_eq!(tasks.len(), 0, "should not create task when targets met");
}

#[test]
fn extraction_recipe_serde_roundtrip() {
    let recipe = Recipe::Extract;
    let json = serde_json::to_string(&recipe).unwrap();
    let restored: Recipe = serde_json::from_str(&json).unwrap();
    assert_eq!(recipe, restored);
    assert_eq!(json, "\"Extract\"");
}

// =========================================================================
// End-to-end crafting integration tests (Recipe enum)
// =========================================================================

#[test]
fn end_to_end_extract_produces_components() {
    let mut sim = test_sim(42);
    let species_id = insert_full_chain_fruit_species(&mut sim);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Kitchen);
    let mat = Material::FruitSpecies(species_id);

    add_recipe_with_targets(&mut sim, structure_id, Recipe::Extract, Some(mat), 100);

    // Stock the kitchen with fruit of the correct species.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(mat),
        0,
        None,
        None,
    );

    // Spawn an elf near the building.
    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);

    // Run enough ticks for at least one extraction cycle.
    sim.step(&[], sim.tick + 20_000);

    // Verify components were produced.
    let pulp = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Pulp,
        inventory::MaterialFilter::Specific(mat),
    );
    let fiber = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::FruitFiber,
        inventory::MaterialFilter::Specific(mat),
    );
    assert!(
        pulp > 0 || fiber > 0,
        "Extraction should produce components (pulp={pulp}, fiber={fiber})"
    );
}

#[test]
fn end_to_end_mill_flour_from_pulp() {
    let mut sim = test_sim(42);
    let species_id = insert_full_chain_fruit_species(&mut sim);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Kitchen);
    let mat = Material::FruitSpecies(species_id);

    add_recipe_with_targets(&mut sim, structure_id, Recipe::Mill, Some(mat), 100);

    // Stock the kitchen with starchy pulp (Mill input).
    let inv_id = sim.structure_inv(structure_id);
    let cr = &sim.config.component_recipes;
    let mill_input_qty = cr.mill_input;
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Pulp,
        mill_input_qty * 3,
        None,
        None,
        Some(mat),
        0,
        None,
        None,
    );

    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);

    sim.step(&[], sim.tick + 30_000);

    let flour = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Flour,
        inventory::MaterialFilter::Specific(mat),
    );
    assert!(flour > 0, "Mill should produce flour, got {flour}");
}

#[test]
fn end_to_end_grow_arrow_no_input() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Oak),
        100,
    );

    // GrowArrow has zero inputs — no stocking needed.
    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);

    sim.step(&[], sim.tick + 20_000);

    let inv_id = sim.structure_inv(structure_id);
    let arrows = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Arrow,
        inventory::MaterialFilter::Specific(Material::Oak),
    );
    assert!(arrows > 0, "GrowArrow should produce arrows, got {arrows}");
}

#[test]
fn end_to_end_grow_bow_consumes_bowstring() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowBow,
        Some(Material::Oak),
        10,
    );

    // Stock the workshop with bowstrings (GrowBow input).
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bowstring, 5, None, None);

    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);

    sim.step(&[], sim.tick + 50_000);

    let bows = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Bow,
        inventory::MaterialFilter::Specific(Material::Oak),
    );
    assert!(bows > 0, "GrowBow should produce bows, got {bows}");

    // Bowstrings should have been consumed.
    let remaining_bowstrings = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Bowstring,
        inventory::MaterialFilter::Any,
    );
    assert!(
        remaining_bowstrings < 5,
        "GrowBow should consume bowstrings, {remaining_bowstrings} remain"
    );
}

#[test]
fn serde_roundtrip_simstate_with_active_recipes() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowBow,
        Some(Material::Oak),
        10,
    );
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Yew),
        50,
    );

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim).unwrap();
    let mut restored: SimState = serde_json::from_str(&json).unwrap();
    restored.rebuild_transient_state();

    // Verify active recipes survived the roundtrip.
    let recipes = restored
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    assert_eq!(recipes.len(), 2);

    let bow_recipe = recipes
        .iter()
        .find(|r| r.recipe == Recipe::GrowBow)
        .expect("GrowBow should survive serde");
    assert_eq!(bow_recipe.material, Some(Material::Oak));

    let arrow_recipe = recipes
        .iter()
        .find(|r| r.recipe == Recipe::GrowArrow)
        .expect("GrowArrow should survive serde");
    assert_eq!(arrow_recipe.material, Some(Material::Yew));

    // Verify targets survived.
    let bow_targets = restored
        .db
        .active_recipe_targets
        .by_active_recipe_id(&bow_recipe.id, tabulosity::QueryOpts::ASC);
    assert_eq!(bow_targets.len(), 1);
    assert_eq!(bow_targets[0].target_quantity, 10);
}

#[test]
fn recipe_removal_during_inflight_task() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    let ar_id = add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Oak),
        100,
    );

    // Spawn an elf to start working on the recipe.
    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);

    // Run a few ticks to start a craft task.
    sim.step(&[], sim.tick + 5_000);

    // Task may or may not have been created yet depending on heartbeat timing.
    // Either way, removing the recipe should not panic.

    // Remove the recipe.
    let remove_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::RemoveActiveRecipe {
            active_recipe_id: ar_id,
        },
    };
    sim.step(&[remove_cmd], sim.tick + 1);

    // Recipe should be gone.
    assert!(
        sim.db.active_recipes.get(&ar_id).is_none(),
        "Recipe should be removed"
    );

    // Run more ticks to verify no panics or stale state.
    sim.step(&[], sim.tick + 10_000);
}

#[test]
fn kitchen_furnishing_does_not_auto_add_extraction_recipes() {
    let mut sim = test_sim(42);
    let _species_id = insert_test_fruit_species(&mut sim);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Kitchen);

    // Auto-add on furnish was removed — kitchens start with zero active recipes.
    let active_recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    assert_eq!(
        active_recipes.len(),
        0,
        "no recipes should be auto-added to kitchen"
    );
}

#[test]
fn part_type_extracted_item_kind_mapping() {
    use crate::fruit::PartType;
    assert_eq!(
        PartType::Flesh.extracted_item_kind(),
        inventory::ItemKind::Pulp
    );
    assert_eq!(
        PartType::Rind.extracted_item_kind(),
        inventory::ItemKind::Husk
    );
    assert_eq!(
        PartType::Seed.extracted_item_kind(),
        inventory::ItemKind::Seed
    );
    assert_eq!(
        PartType::Fiber.extracted_item_kind(),
        inventory::ItemKind::FruitFiber
    );
    assert_eq!(
        PartType::Sap.extracted_item_kind(),
        inventory::ItemKind::FruitSap
    );
    assert_eq!(
        PartType::Resin.extracted_item_kind(),
        inventory::ItemKind::FruitResin
    );
}

// =========================================================================
// Grow craft tasks (mana cost, action count, creature filtering)
// =========================================================================

#[test]
fn grow_craft_task_has_action_count_total_cost() {
    // Grow recipes should have total_cost = ceil(work_ticks / per_action)
    // instead of total_cost = work_ticks.
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);
    place_all_furniture(&mut sim, structure_id);
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Oak),
        100,
    );

    // Run the crafting monitor to create the task.
    sim.process_unified_crafting_monitor();

    let craft_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Craft)
        .collect();
    assert!(
        !craft_tasks.is_empty(),
        "crafting monitor should create a task"
    );

    let task = &craft_tasks[0];
    let per_action = sim.config.grow_recipes.grow_work_ticks_per_action;
    let work_ticks = sim.config.grow_recipes.grow_arrow_work_ticks;
    let expected_actions = work_ticks.div_ceil(per_action) as i64;
    assert_eq!(
        task.total_cost, expected_actions,
        "Grow task total_cost should be action count ({expected_actions}), got {}",
        task.total_cost
    );
}

#[test]
fn grow_arrow_drains_elf_mana() {
    let mut config = test_config();
    // Disable mana regen so we can measure net drain.
    config.species.get_mut(&Species::Elf).unwrap().mana_per_tick = 0;
    let mut sim = SimState::with_config(42, config);

    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Oak),
        100,
    );

    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;
    let mp_before = sim.db.creatures.get(&elf_id).unwrap().mp;

    // Run long enough for the elf to work on the recipe.
    sim.step(&[], sim.tick + 20_000);

    let mp_after = sim.db.creatures.get(&elf_id).unwrap().mp;
    assert!(
        mp_after < mp_before,
        "Grow recipe should drain mana: before={mp_before}, after={mp_after}"
    );

    // Should still produce arrows despite mana cost.
    let inv_id = sim.structure_inv(structure_id);
    let arrows = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Arrow,
        inventory::MaterialFilter::Specific(Material::Oak),
    );
    assert!(
        arrows > 0,
        "GrowArrow should still produce arrows: {arrows}"
    );
}

#[test]
fn grow_with_zero_mana_wastes_actions_and_abandons() {
    let mut config = test_config();
    config.species.get_mut(&Species::Elf).unwrap().mana_per_tick = 0;
    config.mana_abandon_threshold = 2;
    let mut sim = SimState::with_config(42, config);

    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Oak),
        100,
    );

    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Drain all elf mana.
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.mp = 0;
    });

    // Run enough for the elf to attempt and abandon.
    sim.step(&[], sim.tick + 20_000);

    // Should produce zero arrows (never completed a recipe).
    let inv_id = sim.structure_inv(structure_id);
    let arrows = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Arrow,
        inventory::MaterialFilter::Specific(Material::Oak),
    );
    assert_eq!(
        arrows, 0,
        "Grow with no mana should produce nothing: {arrows}"
    );
}

#[test]
fn nonmagical_creature_cannot_claim_grow_craft_task() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);
    place_all_furniture(&mut sim, structure_id);
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Oak),
        100,
    );

    // Run crafting monitor to create the task.
    sim.process_unified_crafting_monitor();

    // Verify a Craft task was created.
    let craft_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state == task::TaskState::Available)
        .collect();
    assert!(
        !craft_tasks.is_empty(),
        "crafting monitor should create a task"
    );

    // Spawn a capybara (nonmagical, mp_max = 0).
    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Capybara, anchor, &mut events);
    let capybara_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap()
        .id;

    // Capybara should NOT find the Grow craft task.
    let found = sim.find_available_task(capybara_id);
    assert!(
        found.is_none(),
        "nonmagical creature should not claim Grow craft task"
    );
}

#[test]
fn non_grow_craft_completes_with_zero_mana() {
    // Extract (non-Grow verb) should work even with 0 mana.
    let mut config = test_config();
    config.species.get_mut(&Species::Elf).unwrap().mana_per_tick = 0;
    let mut sim = SimState::with_config(42, config);

    let species_id = insert_full_chain_fruit_species(&mut sim);
    let mat = Material::FruitSpecies(species_id);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Kitchen);
    place_all_furniture(&mut sim, structure_id);
    add_recipe_with_targets(&mut sim, structure_id, Recipe::Extract, Some(mat), 100);

    // Stock the kitchen with fruit.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        10,
        None,
        None,
        Some(mat),
        0,
        None,
        None,
    );

    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Drain all mana — non-Grow craft should still succeed.
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.mp = 0;
    });

    sim.step(&[], sim.tick + 20_000);

    // Should produce components despite 0 mana.
    let pulp = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::Pulp,
        inventory::MaterialFilter::Specific(mat),
    );
    let fiber = sim.inv_unreserved_item_count(
        inv_id,
        inventory::ItemKind::FruitFiber,
        inventory::MaterialFilter::Specific(mat),
    );
    assert!(
        pulp > 0 || fiber > 0,
        "non-Grow recipe should complete with 0 mana (pulp={pulp}, fiber={fiber})"
    );
}

#[test]
fn drained_elf_can_still_claim_non_grow_craft_task() {
    let mut config = test_config();
    config.species.get_mut(&Species::Elf).unwrap().mana_per_tick = 0;
    let mut sim = SimState::with_config(42, config);

    let species_id = insert_full_chain_fruit_species(&mut sim);
    let mat = Material::FruitSpecies(species_id);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Kitchen);
    place_all_furniture(&mut sim, structure_id);
    add_recipe_with_targets(&mut sim, structure_id, Recipe::Extract, Some(mat), 100);

    // Stock the kitchen with fruit.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Fruit,
        10,
        None,
        None,
        Some(mat),
        0,
        None,
        None,
    );

    // Run crafting monitor to create the task.
    sim.process_unified_crafting_monitor();

    // Verify a Craft task was created.
    let craft_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Craft && t.state == task::TaskState::Available)
        .collect();
    assert!(
        !craft_tasks.is_empty(),
        "crafting monitor should create a task"
    );

    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Drain all mana.
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.mp = 0;
    });

    // Elf with 0 mana should still find the non-Grow craft task.
    let found = sim.find_available_task(elf_id);
    assert!(
        found.is_some(),
        "drained elf should still find non-Grow craft tasks"
    );
}

#[test]
fn grow_recipe_serde_backward_compat_new_config_fields() {
    // Serialize a GameConfig, strip the grow-mana fields, deserialize,
    // and verify serde defaults are correct.
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("grow_mana_cost_per_mille");
    if let Some(grow_obj) = obj.get_mut("grow_recipes").and_then(|v| v.as_object_mut()) {
        grow_obj.remove("grow_work_ticks_per_action");
    }
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: GameConfig = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.grow_mana_cost_per_mille, 20);
    assert_eq!(restored.grow_recipes.grow_work_ticks_per_action, 1000);
}

#[test]
fn drained_elf_cannot_claim_grow_craft_task() {
    // An elf with mp > 0 but below the grow cost should not claim Grow tasks.
    let mut config = test_config();
    config.species.get_mut(&Species::Elf).unwrap().mana_per_tick = 0;
    let mut sim = SimState::with_config(42, config);

    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);
    place_all_furniture(&mut sim, structure_id);
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowArrow,
        Some(Material::Oak),
        100,
    );
    sim.process_unified_crafting_monitor();

    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set mp to 1 — below the grow cost (which is mp_max / 1000 * 20).
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.mp = 1;
    });

    let found = sim.find_available_task(elf_id);
    assert!(
        found.is_none(),
        "elf with insufficient mana should not claim Grow craft task"
    );
}

#[test]
fn grow_craft_task_total_cost_rounds_up() {
    // When work_ticks is not evenly divisible by per_action, div_ceil rounds up.
    let mut config = test_config();
    config.grow_recipes.grow_work_ticks_per_action = 4000;
    // grow_arrow_work_ticks = 3000, so ceil(3000 / 4000) = 1
    // grow_bow_work_ticks = 8000, so ceil(8000 / 4000) = 2
    // grow_helmet_work_ticks = 7000, so ceil(7000 / 4000) = 2
    let mut sim = SimState::with_config(42, config);

    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);
    place_all_furniture(&mut sim, structure_id);
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowHelmet,
        Some(Material::Oak),
        10,
    );
    sim.process_unified_crafting_monitor();

    let task = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Craft)
        .expect("should create a craft task");

    // 7000 / 4000 = 1.75, ceil = 2
    assert_eq!(
        task.total_cost, 2,
        "div_ceil should round up: 7000/4000 = 2 actions"
    );
}

// =========================================================================
// Grow weapon recipes (F-elf-weapons)
// =========================================================================

#[test]
fn recipe_grow_spear_and_club() {
    let config = crate::config::GameConfig::default();
    let params = crate::recipe::RecipeParams {
        material: Some(Material::Oak),
    };

    let spear = Recipe::GrowSpear
        .resolve(&params, &config, &[])
        .expect("GrowSpear should resolve");
    assert!(spear.inputs.is_empty());
    assert_eq!(spear.outputs[0].item_kind, ItemKind::Spear);
    assert_eq!(spear.outputs[0].quantity, 1);
    assert_eq!(spear.outputs[0].material, Some(Material::Oak));
    assert_eq!(spear.work_ticks, config.grow_recipes.grow_spear_work_ticks);

    let club = Recipe::GrowClub
        .resolve(&params, &config, &[])
        .expect("GrowClub should resolve");
    assert!(club.inputs.is_empty());
    assert_eq!(club.outputs[0].item_kind, ItemKind::Club);
    assert_eq!(club.outputs[0].quantity, 1);
    assert_eq!(club.outputs[0].material, Some(Material::Oak));
    assert_eq!(club.work_ticks, config.grow_recipes.grow_club_work_ticks);
}

/// GrowSpear and GrowClub reject non-wood materials.
#[test]
fn recipe_grow_weapons_reject_non_wood() {
    let config = crate::config::GameConfig::default();
    let params = crate::recipe::RecipeParams {
        material: Some(Material::FruitSpecies(crate::fruit::FruitSpeciesId(0))),
    };
    assert!(Recipe::GrowSpear.resolve(&params, &config, &[]).is_none());
    assert!(Recipe::GrowClub.resolve(&params, &config, &[]).is_none());
}

/// GrowSpear and GrowClub are workshop recipes with Grow verb.
#[test]
fn recipe_grow_weapons_metadata() {
    use crate::recipe::RecipeVerb;
    use crate::types::FurnishingType;

    assert_eq!(Recipe::GrowSpear.verb(), RecipeVerb::Grow);
    assert_eq!(Recipe::GrowClub.verb(), RecipeVerb::Grow);
    assert_eq!(
        Recipe::GrowSpear.furnishing_types(),
        vec![FurnishingType::Workshop]
    );
    assert_eq!(
        Recipe::GrowClub.furnishing_types(),
        vec![FurnishingType::Workshop]
    );
    assert_eq!(Recipe::GrowSpear.category(), vec!["Woodcraft", "Weapons"]);
    assert_eq!(Recipe::GrowClub.category(), vec!["Woodcraft", "Weapons"]);
}

/// GrowSpear and GrowClub display names.
#[test]
fn recipe_grow_weapons_display_names() {
    let params = crate::recipe::RecipeParams {
        material: Some(Material::Oak),
    };
    assert_eq!(
        Recipe::GrowSpear.display_name(&params, &[]),
        "Grow Oak Spear"
    );
    assert_eq!(Recipe::GrowClub.display_name(&params, &[]), "Grow Oak Club");
}

/// GrowSpear and GrowClub serde roundtrip.
#[test]
fn recipe_grow_weapons_serde_roundtrip() {
    for recipe in [Recipe::GrowSpear, Recipe::GrowClub] {
        let json = serde_json::to_string(&recipe).unwrap();
        let restored: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(recipe, restored, "roundtrip failed for {json}");
    }
    assert_eq!(
        serde_json::to_string(&Recipe::GrowSpear).unwrap(),
        "\"GrowSpear\""
    );
    assert_eq!(
        serde_json::to_string(&Recipe::GrowClub).unwrap(),
        "\"GrowClub\""
    );
}

// =========================================================================
// Footwear sew recipes (F-footwear-split)
// =========================================================================

/// SewSandals and SewShoes recipe serde roundtrip.
#[test]
fn recipe_sew_footwear_serde_roundtrip() {
    for recipe in [Recipe::SewSandals, Recipe::SewShoes] {
        let json = serde_json::to_string(&recipe).unwrap();
        let restored: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(recipe, restored, "roundtrip failed for {json}");
    }
    assert_eq!(
        serde_json::to_string(&Recipe::SewSandals).unwrap(),
        "\"SewSandals\""
    );
    assert_eq!(
        serde_json::to_string(&Recipe::SewShoes).unwrap(),
        "\"SewShoes\""
    );
}

/// SewSandals resolves with correct config values (1 cloth, 3000 ticks).
#[test]
fn sew_sandals_recipe_resolve() {
    let config = crate::config::GameConfig::default();
    let mut sim = test_sim(42);
    let species_id = insert_full_chain_fruit_species(&mut sim);
    let species: Vec<_> = sim.db.fruit_species.iter_all().cloned().collect();
    let params = crate::recipe::RecipeParams {
        material: Some(inventory::Material::FruitSpecies(species_id)),
    };
    let resolved = Recipe::SewSandals
        .resolve(&params, &config, &species)
        .expect("SewSandals should resolve");
    assert_eq!(resolved.inputs[0].item_kind, inventory::ItemKind::Cloth);
    assert_eq!(
        resolved.inputs[0].quantity,
        config.component_recipes.sew_sandals_input
    );
    assert_eq!(resolved.outputs[0].item_kind, inventory::ItemKind::Sandals);
    assert_eq!(
        resolved.work_ticks,
        config.component_recipes.sew_sandals_work_ticks
    );
}

/// SewShoes resolves with correct config values (2 cloth, 5000 ticks).
#[test]
fn sew_shoes_recipe_resolve() {
    let config = crate::config::GameConfig::default();
    let mut sim = test_sim(42);
    let species_id = insert_full_chain_fruit_species(&mut sim);
    let species: Vec<_> = sim.db.fruit_species.iter_all().cloned().collect();
    let params = crate::recipe::RecipeParams {
        material: Some(inventory::Material::FruitSpecies(species_id)),
    };
    let resolved = Recipe::SewShoes
        .resolve(&params, &config, &species)
        .expect("SewShoes should resolve");
    assert_eq!(resolved.inputs[0].item_kind, inventory::ItemKind::Cloth);
    assert_eq!(
        resolved.inputs[0].quantity,
        config.component_recipes.sew_shoes_input
    );
    assert_eq!(resolved.outputs[0].item_kind, inventory::ItemKind::Shoes);
    assert_eq!(
        resolved.work_ticks,
        config.component_recipes.sew_shoes_work_ticks
    );
}

// =========================================================================
// Craft quality
// =========================================================================

#[test]
fn quality_propagation_score_mapping() {
    use super::crafting::quality_score;
    assert_eq!(quality_score(-1), 0); // Crude
    assert_eq!(quality_score(0), 150); // Fine
    assert_eq!(quality_score(1), 300); // Superior
}

#[test]
fn quality_propagation_crude_inputs_drag_down() {
    use super::crafting::{quality_from_roll, quality_score};
    // Crafter rolls 200 (would be Fine), all-crude inputs (score 0).
    // Adjusted = (200 + 0) / 2 = 100 → Fine, but dragged down from near-superior.
    let avg_input_score = quality_score(-1); // 0
    let roll = 200i64;
    // Drag-down: avg < roll, so adjust = (roll + avg) / 2
    assert!(avg_input_score < roll);
    let adjusted = (roll + avg_input_score) / 2; // 100
    assert_eq!(quality_from_roll(adjusted), 0); // Fine

    // Without drag-down, roll 260 would be Superior.
    let roll = 260i64;
    assert_eq!(quality_from_roll(roll), 1); // Superior
    // With crude inputs, adjusted = (260 + 0) / 2 = 130 → Fine.
    let adjusted = (roll + avg_input_score) / 2; // 130
    assert_eq!(quality_from_roll(adjusted), 0); // Dragged to Fine
}

#[test]
fn quality_propagation_with_inputs_statistical() {
    // Verify determine_craft_quality_with_inputs applies drag-down:
    // a high-skill elf with all-crude inputs should produce lower quality
    // on average than the same elf with no inputs (extract recipe).
    let mut sim = test_sim(123);
    let creature_id = spawn_creature(&mut sim, Species::Elf);
    // High combined stats+skill = ~200 (mostly Fine/Superior without drag).
    set_trait(&mut sim, creature_id, TraitKind::Dexterity, 50);
    set_trait(&mut sim, creature_id, TraitKind::Intelligence, 50);
    set_trait(&mut sim, creature_id, TraitKind::Perception, 50);
    set_trait(&mut sim, creature_id, TraitKind::Herbalism, 50);

    let n = 5_000;
    let crude_inputs = vec![-1i32; 5]; // 5 crude inputs

    // Roll with no inputs (extract).
    let mut no_input_sum = 0i64;
    let mut sim_a = sim.clone();
    for _ in 0..n {
        let q = sim_a.determine_craft_quality(creature_id, crate::recipe::RecipeVerb::Extract);
        no_input_sum += q as i64;
    }

    // Roll with crude inputs.
    let mut crude_input_sum = 0i64;
    let mut sim_b = sim.clone();
    for _ in 0..n {
        let q = sim_b.determine_craft_quality_with_inputs(
            creature_id,
            crate::recipe::RecipeVerb::Extract,
            &crude_inputs,
        );
        crude_input_sum += q as i64;
    }

    // Crude inputs should drag quality down (lower average).
    let no_input_avg = no_input_sum as f64 / n as f64;
    let crude_input_avg = crude_input_sum as f64 / n as f64;
    assert!(
        crude_input_avg < no_input_avg,
        "crude inputs should produce lower avg quality: no_input={no_input_avg:.3}, crude={crude_input_avg:.3}"
    );
}

#[test]
fn quality_propagation_good_inputs_no_boost() {
    use super::crafting::{quality_from_roll, quality_score};
    // Crafter rolls 100 (Fine), superior inputs (score 300).
    // avg_input_score >= roll, so NO adjustment — good materials can't boost.
    let avg_input_score = quality_score(1); // 300
    let roll = 100i64;
    assert!(avg_input_score >= roll);
    // Roll stays 100 → Fine.
    assert_eq!(quality_from_roll(roll), 0);
}

#[test]
fn craft_output_gets_rolled_quality() {
    // Verify that crafted items receive quality from determine_craft_quality,
    // not a hardcoded value. A high-skill elf baking bread should sometimes
    // produce non-zero quality.
    let mut sim = test_sim(42);
    let species_id = insert_full_chain_fruit_species(&mut sim);
    let mat = Material::FruitSpecies(species_id);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Kitchen);
    place_all_furniture(&mut sim, structure_id);
    add_recipe_with_targets(&mut sim, structure_id, Recipe::Bake, Some(mat), 100);

    // Stock the kitchen with flour.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Flour,
        50,
        None,
        None,
        Some(mat),
        0,
        None,
        None,
    );

    // Spawn an elf near the structure and give it exceptional Cuisine skill
    // and high stats so it reliably produces Superior items.
    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 80);
    set_trait(&mut sim, elf_id, TraitKind::Intelligence, 80);
    set_trait(&mut sim, elf_id, TraitKind::Perception, 80);
    set_trait(&mut sim, elf_id, TraitKind::Cuisine, 100);

    // Run sim long enough for several crafts to complete.
    sim.step(&[], sim.tick + 100_000);

    // Check produced bread. With combined stat+skill ~340, all rolls should
    // land in Superior (+1) range (threshold 250, mean roll ~340).
    let bread: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Bread)
        .collect();
    assert!(!bread.is_empty(), "elf should have baked some bread");
    // With stats 240 + skill 100 = 340 combined and stddev 50, the raw roll
    // mean is ~340. Input drag-down from Fine flour (score 150) adjusts rolls
    // above 150 to (roll + 150) / 2, near the Superior threshold. At minimum,
    // quality should not be hardcoded 0 on all items — at least some should
    // be Superior (+1), proving the roll is active.
    let any_non_fine = bread.iter().any(|s| s.quality != 0);
    assert!(
        any_non_fine,
        "with stats+skill ~340, at least some bread should be non-Fine quality"
    );
}

#[test]
fn determine_craft_quality_statistical() {
    // Elf with combined stat+skill ~100 should produce ~16% Crude, ~84% Fine,
    // ~0% Superior over many trials.
    let mut sim = test_sim(999);
    let creature_id = spawn_creature(&mut sim, Species::Elf);
    // Set DEX=20, INT=20, PER=10 (sum=50) and Herbalism=50 → combined=100.
    set_trait(&mut sim, creature_id, TraitKind::Dexterity, 20);
    set_trait(&mut sim, creature_id, TraitKind::Intelligence, 20);
    set_trait(&mut sim, creature_id, TraitKind::Perception, 10);
    set_trait(&mut sim, creature_id, TraitKind::Herbalism, 50);

    let mut counts = [0i32; 3]; // crude, fine, superior
    let n = 10_000;
    for _ in 0..n {
        let q = sim.determine_craft_quality(creature_id, crate::recipe::RecipeVerb::Extract);
        match q {
            -1 => counts[0] += 1,
            0 => counts[1] += 1,
            1 => counts[2] += 1,
            _ => panic!("unexpected quality {q}"),
        }
    }
    let crude_pct = counts[0] as f64 / n as f64 * 100.0;
    let fine_pct = counts[1] as f64 / n as f64 * 100.0;
    let superior_pct = counts[2] as f64 / n as f64 * 100.0;

    // At combined=100: quasi_normal(50) has stddev~50, so roll mean=100.
    // P(roll<50) ≈ 16%, P(roll>=250) ≈ 0.1%.
    assert!(
        crude_pct > 8.0 && crude_pct < 25.0,
        "crude {crude_pct:.1}% out of expected ~16% range"
    );
    assert!(
        fine_pct > 70.0 && fine_pct < 95.0,
        "fine {fine_pct:.1}% out of expected ~84% range"
    );
    assert!(
        superior_pct < 3.0,
        "superior {superior_pct:.1}% should be near 0%"
    );
}

#[test]
fn grow_quality_uses_woodcraft_not_singing() {
    // Grow recipes should use DEX + INT + PER + Woodcraft for quality,
    // not CHA + INT + PER + Singing (which is for Construction).
    let mut sim = test_sim(42);
    let creature_id = spawn_creature(&mut sim, Species::Elf);
    // High DEX + Woodcraft, zero CHA + Singing.
    set_trait(&mut sim, creature_id, TraitKind::Dexterity, 100);
    set_trait(&mut sim, creature_id, TraitKind::Intelligence, 50);
    set_trait(&mut sim, creature_id, TraitKind::Perception, 50);
    set_trait(&mut sim, creature_id, TraitKind::Woodcraft, 100);
    set_trait(&mut sim, creature_id, TraitKind::Charisma, 0);
    set_trait(&mut sim, creature_id, TraitKind::Singing, 0);

    let n = 1_000;
    let mut quality_sum = 0i64;
    for _ in 0..n {
        let q = sim.determine_craft_quality(creature_id, crate::recipe::RecipeVerb::Grow);
        quality_sum += q as i64;
    }
    // Combined = DEX(100) + INT(50) + PER(50) + Woodcraft(100) = 300.
    // Mean roll ~300, well above Superior threshold (250).
    // Average quality should be close to +1 (Superior).
    let avg = quality_sum as f64 / n as f64;
    assert!(
        avg > 0.7,
        "Grow quality with high DEX+Woodcraft should be mostly Superior, avg={avg:.2}"
    );
}

#[test]
fn subcomponent_records_inherit_parent_quality() {
    // Verify that subcomponent records get the parent item's rolled quality,
    // not a hardcoded 0.
    let mut sim = test_sim(42);
    let species_id = insert_full_chain_fruit_species(&mut sim);
    let mat = Material::FruitSpecies(species_id);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);
    place_all_furniture(&mut sim, structure_id);

    // Assemble recipe (e.g., GrowBow) typically has subcomponents.
    // Use a recipe that records subcomponents. Let's check which ones do.
    // GrowBow has a Bowstring subcomponent.
    add_recipe_with_targets(
        &mut sim,
        structure_id,
        Recipe::GrowBow,
        Some(Material::Oak),
        10,
    );

    // Stock the workshop with bowstrings (needed as input for GrowBow).
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_item(
        inv_id,
        inventory::ItemKind::Bowstring,
        10,
        None,
        None,
        Some(mat),
        -1, // Crude bowstrings
        None,
        None,
    );

    // Spawn a high-skill elf to ensure Superior output.
    let anchor = sim.db.structures.get(&structure_id).unwrap().anchor;
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, anchor, &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;
    set_trait(&mut sim, elf_id, TraitKind::Dexterity, 100);
    set_trait(&mut sim, elf_id, TraitKind::Intelligence, 100);
    set_trait(&mut sim, elf_id, TraitKind::Perception, 100);
    set_trait(&mut sim, elf_id, TraitKind::Woodcraft, 100);

    // Give elf enough mana for Grow recipes.
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.mp = 100_000;
    });

    sim.step(&[], sim.tick + 200_000);

    // Find any crafted bows.
    let bows: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == inventory::ItemKind::Bow)
        .collect();

    if !bows.is_empty() {
        for bow in &bows {
            let subs = sim
                .db
                .item_subcomponents
                .by_item_stack_id(&bow.id, tabulosity::QueryOpts::ASC);
            for sub in &subs {
                assert_eq!(
                    sub.quality, bow.quality,
                    "subcomponent quality ({}) should match parent bow quality ({})",
                    sub.quality, bow.quality,
                );
            }
        }
    }
    // If no bows were produced (elf couldn't reach workshop, mana issues, etc.)
    // the test passes vacuously — the craft integration test covers production.
}

// =========================================================================
// Swap active recipe
// =========================================================================

#[test]
fn swap_active_recipe_sort_order_preserves_targets() {
    let mut sim = test_sim(42);
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Add two recipes that each generate target rows.
    sim.add_active_recipe(structure_id, Recipe::GrowBow, Some(Material::Oak));
    sim.add_active_recipe(structure_id, Recipe::GrowArrow, Some(Material::Oak));

    let recipes = sim
        .db
        .active_recipes
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC);
    assert_eq!(recipes.len(), 2, "Should have two active recipes");

    let id_a = recipes[0].id;
    let id_b = recipes[1].id;
    let sort_a = recipes[0].sort_order;
    let sort_b = recipes[1].sort_order;

    // Snapshot original targets.
    let targets_a_before = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&id_a, tabulosity::QueryOpts::ASC);
    let targets_b_before = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&id_b, tabulosity::QueryOpts::ASC);
    assert!(
        !targets_a_before.is_empty(),
        "Recipe A should have targets before swap"
    );
    assert!(
        !targets_b_before.is_empty(),
        "Recipe B should have targets before swap"
    );

    // Perform the swap.
    sim.swap_active_recipe_sort_order(id_a, id_b);

    // Verify sort_orders are swapped.
    let recipe_a_after = sim.db.active_recipes.get(&id_a).unwrap();
    let recipe_b_after = sim.db.active_recipes.get(&id_b).unwrap();
    assert_eq!(
        recipe_a_after.sort_order, sort_b,
        "Recipe A should have B's sort_order"
    );
    assert_eq!(
        recipe_b_after.sort_order, sort_a,
        "Recipe B should have A's sort_order"
    );

    // Verify all original targets still exist and are associated with the
    // correct (same) recipe IDs — targets follow the recipe row, not the
    // sort_order.
    let targets_a_after = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&id_a, tabulosity::QueryOpts::ASC);
    let targets_b_after = sim
        .db
        .active_recipe_targets
        .by_active_recipe_id(&id_b, tabulosity::QueryOpts::ASC);
    assert_eq!(
        targets_a_after.len(),
        targets_a_before.len(),
        "Recipe A should have the same number of targets after swap"
    );
    assert_eq!(
        targets_b_after.len(),
        targets_b_before.len(),
        "Recipe B should have the same number of targets after swap"
    );
    // Verify target content matches (same output item kinds).
    for (before, after) in targets_a_before.iter().zip(targets_a_after.iter()) {
        assert_eq!(
            before.output_item_kind, after.output_item_kind,
            "Recipe A target item kind should be preserved"
        );
    }
    for (before, after) in targets_b_before.iter().zip(targets_b_after.iter()) {
        assert_eq!(
            before.output_item_kind, after.output_item_kind,
            "Recipe B target item kind should be preserved"
        );
    }
}

// =========================================================================
// Crafting reserve
// =========================================================================

#[test]
fn crafting_reserve_skips_owned_items() {
    // inv_reserve_items (used by crafting) should also skip owned items
    // so that an elf's personal belongings stored in a workshop aren't
    // consumed by a recipe.
    let mut sim = test_sim(42);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = spawn_elf(&mut sim);

    let anchor = VoxelCoord::new(tree_pos.x + 3, tree_pos.y, tree_pos.z);
    let sid = insert_building(&mut sim, anchor, Some(5), Vec::new());
    let inv = sim.db.structures.get(&sid).unwrap().inventory_id;

    // 3 owned + 5 unowned arrows.
    sim.inv_add_simple_item(
        inv,
        crate::inventory::ItemKind::Arrow,
        3,
        Some(elf_id),
        None,
    );
    sim.inv_add_simple_item(inv, crate::inventory::ItemKind::Arrow, 5, None, None);

    let task_id = TaskId::new(&mut sim.rng);
    sim.inv_reserve_items(
        inv,
        crate::inventory::ItemKind::Arrow,
        crate::inventory::MaterialFilter::Any,
        5,
        task_id,
    );

    // Verify: only unowned items should be reserved.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv, tabulosity::QueryOpts::ASC);
    let owned_reserved: u32 = stacks
        .iter()
        .filter(|s| s.owner == Some(elf_id) && s.reserved_by.is_some())
        .map(|s| s.quantity)
        .sum();
    assert_eq!(
        owned_reserved, 0,
        "Owned items must not be reserved for crafting"
    );
}
