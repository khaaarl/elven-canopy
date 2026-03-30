//! Tests for the greenhouse system: furnishing, fruit production,
//! species validation, display names, and serde roundtrip.
//! Corresponds to `sim/greenhouse.rs`.

use super::*;

// -----------------------------------------------------------------------
// Greenhouse tests
// -----------------------------------------------------------------------

/// Helper: get the first cultivable fruit species from the DB.
fn first_cultivable_species(sim: &SimState) -> Option<FruitSpeciesId> {
    sim.db
        .fruit_species
        .iter_all()
        .find(|f| f.greenhouse_cultivable)
        .map(|f| f.id)
}

/// Helper: get a non-cultivable fruit species from the DB.
fn first_non_cultivable_species(sim: &SimState) -> Option<FruitSpeciesId> {
    sim.db
        .fruit_species
        .iter_all()
        .find(|f| !f.greenhouse_cultivable)
        .map(|f| f.id)
}

#[test]
fn furnish_greenhouse_sets_species_and_creates_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let species_id = first_cultivable_species(&sim)
        .expect("worldgen should produce at least one cultivable fruit");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Greenhouse,
            greenhouse_species: Some(species_id),
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(structure.furnishing, Some(FurnishingType::Greenhouse));
    assert_eq!(structure.greenhouse_species, Some(species_id));
    assert!(structure.greenhouse_enabled);
    assert_eq!(structure.greenhouse_last_production_tick, sim.tick);

    // Should have created a Furnish task.
    let furnish_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == crate::db::TaskKindTag::Furnish)
        .collect();
    assert_eq!(furnish_tasks.len(), 1);
}

#[test]
fn furnish_greenhouse_rejects_non_cultivable_species() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let species_id = first_non_cultivable_species(&sim);
    // If all species happen to be cultivable in this seed, skip.
    if let Some(species_id) = species_id {
        let cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::FurnishStructure {
                structure_id,
                furnishing_type: FurnishingType::Greenhouse,
                greenhouse_species: Some(species_id),
            },
        };
        sim.step(&[cmd], sim.tick + 1);

        let structure = sim.db.structures.get(&structure_id).unwrap();
        assert_eq!(
            structure.furnishing, None,
            "Non-cultivable species should be rejected"
        );
    }
}

#[test]
fn furnish_greenhouse_rejects_missing_species() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // No greenhouse_species provided.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Greenhouse,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(
        structure.furnishing, None,
        "Greenhouse without species should be rejected"
    );
}

#[test]
fn furnish_greenhouse_rejects_unknown_species() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let bogus_id = FruitSpeciesId(9999);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Greenhouse,
            greenhouse_species: Some(bogus_id),
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(
        structure.furnishing, None,
        "Unknown species should be rejected"
    );
}

#[test]
fn greenhouse_produces_fruit_after_interval() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let species_id = first_cultivable_species(&sim).expect("need a cultivable species");

    // Set a short production interval for testing.
    sim.config.greenhouse_base_production_ticks = 1000;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Greenhouse,
            greenhouse_species: Some(species_id),
        },
    };
    sim.step(&[cmd], sim.tick + 1);
    let furnish_tick = sim.tick;

    // The building has 3x3 = 9 interior tiles (floor_interior_positions).
    // Production interval = base / area = 1000 / 9 = 111 ticks.
    let structure = sim.db.structures.get(&structure_id).unwrap();
    let area = structure.floor_interior_positions().len() as u64;
    let interval = sim.config.greenhouse_base_production_ticks / area;

    // Advance past one interval + logistics heartbeat.
    let logistics_interval = sim.config.logistics_heartbeat_interval_ticks;
    let target_tick = furnish_tick + interval + logistics_interval;
    sim.step(&[], target_tick);

    // Check that fruit was produced in the greenhouse's inventory.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    let fruit_count: u32 = sim
        .db
        .item_stacks
        .iter_all()
        .filter(|s| {
            s.inventory_id == inv_id
                && s.kind == inventory::ItemKind::Fruit
                && s.material == Some(inventory::Material::FruitSpecies(species_id))
        })
        .map(|s| s.quantity)
        .sum();
    assert!(
        fruit_count >= 1,
        "Greenhouse should have produced at least 1 fruit, got {fruit_count}"
    );
}

#[test]
fn greenhouse_display_name() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let species_id = first_cultivable_species(&sim).expect("need a cultivable species");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Greenhouse,
            greenhouse_species: Some(species_id),
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    let name = structure.display_name();
    assert!(
        name.starts_with("Greenhouse #"),
        "Expected 'Greenhouse #N', got '{name}'"
    );
}

#[test]
fn greenhouse_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    let species_id = first_cultivable_species(&sim).expect("need a cultivable species");

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::Greenhouse,
            greenhouse_species: Some(species_id),
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Roundtrip through JSON.
    let json = serde_json::to_string(&sim).unwrap();
    let sim2: SimState = serde_json::from_str(&json).unwrap();

    let structure = sim2.db.structures.get(&structure_id).unwrap();
    assert_eq!(structure.furnishing, Some(FurnishingType::Greenhouse));
    assert_eq!(structure.greenhouse_species, Some(species_id));
    assert!(structure.greenhouse_enabled);
}

#[test]
fn greenhouse_fruit_haul_to_extraction_kitchen() {
    let mut sim = test_sim(legacy_test_seed());
    let (kitchen_id, species_id) = setup_extraction_kitchen(&mut sim);

    // Pre-fill bread so the bread recipe doesn't interfere.
    let kitchen_inv = sim.db.structures.get(&kitchen_id).unwrap().inventory_id;
    sim.inv_add_item(
        kitchen_inv,
        inventory::ItemKind::Bread,
        200,
        None,
        None,
        None,
        0,
        None,
        None,
    );

    // Create a greenhouse with the same species.
    let gh_anchor = find_building_site(&sim);
    let gh_id = insert_completed_building(&mut sim, gh_anchor);
    let furnish_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id: gh_id,
            furnishing_type: FurnishingType::Greenhouse,
            greenhouse_species: Some(species_id),
        },
    };
    sim.step(&[furnish_cmd], sim.tick + 1);

    // Verify greenhouse has a logistics priority set.
    let gh = sim.db.structures.get(&gh_id).unwrap();
    assert!(
        gh.logistics_priority.is_some(),
        "greenhouse should have logistics priority"
    );
    assert!(
        gh.logistics_priority.unwrap() < sim.config.kitchen_default_priority,
        "greenhouse priority should be lower than kitchen's"
    );

    // Put fruit in the greenhouse.
    let gh_inv = gh.inventory_id;
    sim.inv_add_item(
        gh_inv,
        inventory::ItemKind::Fruit,
        5,
        None,
        None,
        Some(inventory::Material::FruitSpecies(species_id)),
        0,
        None,
        None,
    );

    // The extraction recipe auto-logistics should generate a want for
    // this species' fruit. Verify the effective wants include it.
    let wants = sim.compute_effective_wants(kitchen_id);
    let fruit_want = wants.iter().find(|w| {
        w.item_kind == inventory::ItemKind::Fruit
            && w.material_filter
                == inventory::MaterialFilter::Specific(inventory::Material::FruitSpecies(
                    species_id,
                ))
    });
    assert!(
        fruit_want.is_some(),
        "kitchen should want fruit of the extraction species"
    );

    // The haul source search should find the greenhouse fruit.
    let source = sim.find_haul_source(
        inventory::ItemKind::Fruit,
        inventory::MaterialFilter::Specific(inventory::Material::FruitSpecies(species_id)),
        1,
        kitchen_id,
        sim.config.kitchen_default_priority,
    );
    assert!(
        source.is_some(),
        "should find greenhouse as haul source for fruit"
    );
}
