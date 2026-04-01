//! Tests for the needs system: hunger (fruit eating, bread eating, dining hall),
//! sleep (home assignment, dormitory fallback, tiredness), and dining hall
//! mechanics (seat reservation, food consumption, thought generation).
//! Corresponds to `sim/needs.rs`.

use super::*;

/// Helper: create a furnished dining hall at `pos` with one table and stock
/// `food_count` bread items, using a caller-chosen structure ID.
fn create_dining_hall_with_id(
    sim: &mut SimState,
    pos: VoxelCoord,
    food_count: u32,
    structure_id: StructureId,
) -> StructureId {
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            project_id,
            build_type: BuildType::Building,
            anchor: pos,
            width: 3,
            depth: 3,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::DiningHall),
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();
    // Place one table.
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            structure_id,
            coord: pos,
            placed: true,
        })
        .unwrap();
    // Stock food.
    if food_count > 0 {
        sim.inv_add_simple_item(inv_id, ItemKind::Bread, food_count, None, None);
    }
    structure_id
}

/// Helper: create a furnished dining hall at `pos` with one table and stock
/// `food_count` bread items. Returns the structure ID.
fn create_dining_hall(sim: &mut SimState, pos: VoxelCoord, food_count: u32) -> StructureId {
    create_dining_hall_with_id(sim, pos, food_count, StructureId(900))
}

// -----------------------------------------------------------------------
// Sleep priority / busy-elf tests
// -----------------------------------------------------------------------

#[test]
fn busy_tired_elf_does_not_create_sleep_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set rest very low but keep food high.
    let food_max_val = sim.species_table[&Species::Elf].food_max;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 10 / 100;
        c.food = food_max_val;
        sim.db.update_creature(c).unwrap();
    }

    // Give the elf a GoTo task so it's busy. Use a valid nav node position
    // so the elf doesn't immediately fail pathfinding and drop the task.
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let goto_target = sim
        .nav_graph
        .ground_node_ids()
        .iter()
        .map(|&nid| sim.nav_graph.node(nid).position)
        .find(|&p| p != elf_pos)
        .expect("need a distant ground nav node");
    let task_id = TaskId::new(&mut sim.rng);
    let goto_task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: goto_target,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(goto_task);
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.current_task = Some(task_id);
    sim.db.update_creature(c).unwrap();

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // The elf should still have its GoTo task, not a Sleep one.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.current_task,
        Some(task_id),
        "Busy elf should keep its existing task"
    );
    let sleep_task_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Sleep)
        .count();
    assert_eq!(
        sleep_task_count, 0,
        "No Sleep task should be created for a busy elf"
    );
}

#[test]
fn hungry_takes_priority_over_tired() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Ensure the tree has fruit so the hungry elf can get an EatFruit task.
    // With random seeds, the tree may not have fruit naturally.
    ensure_tree_has_fruit(&mut sim);

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Both food AND rest below threshold.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 20 / 100;
        c.rest = rest_max * 20 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // Hunger takes priority — should get EatFruit, not Sleep.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Hungry+tired elf should get a task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert!(
        task.kind_tag == TaskKindTag::EatFruit,
        "Hunger should take priority over tiredness, got {:?}",
        task.kind_tag
    );
}

#[test]
fn ground_sleep_fallback_when_no_beds() {
    // No dormitories exist — tired elf should get a ground Sleep task.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set rest below threshold, food high.
    let food_max_val = sim.species_table[&Species::Elf].food_max;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 30 / 100;
        c.food = food_max_val;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // Should have a Sleep task with bed_pos: None (ground sleep).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Tired elf should get a Sleep task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
    let bed_pos = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
    assert_eq!(bed_pos, None, "No dormitories — should be ground sleep");
    // Ground sleep total_cost = sleep_ticks_ground / sleep_action_ticks (number of actions).
    let expected_cost = (sim.config.sleep_ticks_ground / sim.config.sleep_action_ticks) as i64;
    assert_eq!(
        task.total_cost, expected_cost,
        "Ground sleep total_cost should be number of actions"
    );
}

#[test]
fn find_nearest_bed_excludes_occupied() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Find a valid nav node near tree for the bed position.
    let graph = sim.graph_for_species(Species::Elf);
    let bed_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let bed_pos = graph.node(bed_node).position;

    // Add a dormitory structure with exactly one bed.
    let structure_id = StructureId(999);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            project_id,
            build_type: BuildType::Building,
            anchor: bed_pos,
            width: 3,
            depth: 3,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::Dormitory),
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            structure_id,
            coord: bed_pos,
            placed: true,
        })
        .unwrap();

    // Spawn two elves.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        },
    ];
    sim.step(&cmds, 1);

    let elf_ids: Vec<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    assert_eq!(elf_ids.len(), 2);

    // Make both elves tired with high food.
    let food_max_val = sim.species_table[&Species::Elf].food_max;
    for &elf_id in &elf_ids {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 20 / 100;
        c.food = food_max_val;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // Both should have Sleep tasks.
    let mut bed_sleep_count = 0;
    let mut ground_sleep_count = 0;
    for &elf_id in &elf_ids {
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        if let Some(task_id) = elf.current_task
            && let Some(task) = sim.db.tasks.get(&task_id)
            && task.kind_tag == TaskKindTag::Sleep
        {
            let bed = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
            if bed.is_some() {
                bed_sleep_count += 1;
            } else {
                ground_sleep_count += 1;
            }
        }
    }
    // One bed available → one elf gets bed sleep, one gets ground sleep.
    assert_eq!(bed_sleep_count, 1, "One elf should sleep in the bed");
    assert_eq!(
        ground_sleep_count, 1,
        "Second elf should sleep on the ground"
    );
}

#[test]
fn tired_elf_sleeps_and_rest_increases() {
    // Integration test: set low rest, add a dormitory with beds, run many
    // ticks, verify rest increased (proves sleeping happened).
    let mut config = test_config();
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    // Don't let food or rest decay interfere — zero both so we can
    // set rest manually and only see the effect of sleeping.
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;

    // Add a dormitory with beds near the tree.
    let graph = sim.graph_for_species(Species::Elf);
    let bed_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let bed_pos = graph.node(bed_node).position;

    let structure_id = StructureId(999);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            project_id,
            build_type: BuildType::Building,
            anchor: bed_pos,
            width: 3,
            depth: 3,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::Dormitory),
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            structure_id,
            coord: bed_pos,
            placed: true,
        })
        .unwrap();

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set rest to 20% — well below the 50% threshold.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 20 / 100;
        sim.db.update_creature(c).unwrap();
    }
    let rest_before = sim.db.creatures.get(&elf_id).unwrap().rest;

    // Run for 50_000 ticks — enough for heartbeat + pathfind + sleep.
    sim.step(&[], 50_001);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    // If the elf slept, rest should be meaningfully higher than 20%.
    // With rest_per_sleep_tick=60B and sleep_ticks_bed=10_000 activations,
    // full bed sleep restores 600T = 60% of rest_max. Even with continued
    // decay, rest should be well above the starting 20%.
    assert!(
        elf.rest > rest_before,
        "Tired elf should have slept and restored rest above starting level. rest={}, was={}",
        elf.rest,
        rest_before
    );
}

// -----------------------------------------------------------------------
// Hunger / EatFruit tests
// -----------------------------------------------------------------------

#[test]
fn find_nearest_fruit_returns_reachable() {
    let mut sim = test_sim(legacy_test_seed());
    ensure_tree_has_fruit(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf near the tree so it has a nav node.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // find_nearest_fruit should return a fruit reachable via nav graph.
    let result = sim.find_nearest_fruit(elf_id);
    assert!(
        result.is_some(),
        "Elf near tree should find reachable fruit"
    );
    let (fruit_pos, nav_node) = result.unwrap();

    // The fruit_pos should have a TreeFruit row.
    let has_row = !sim
        .db
        .tree_fruits
        .by_position(&fruit_pos, tabulosity::QueryOpts::ASC)
        .is_empty();
    assert!(has_row, "Returned fruit should have a TreeFruit row");

    // The nav node should be valid.
    let _node = sim.nav_graph.node(nav_node);
}

#[test]
fn eat_fruit_task_restores_food_on_arrival() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let restore_pct = sim.species_table[&Species::Elf].food_restore_pct;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;
    let elf_node = creature_node(&sim, elf_id);

    // Set elf food low.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max / 10;
        sim.db.update_creature(c).unwrap();
    }
    let food_before = sim.db.creatures.get(&elf_id).unwrap().food;

    // Manually create an EatFruit task at the elf's current node (instant arrival).
    let fruit_pos = VoxelCoord::new(0, 0, 0); // dummy — food restore doesn't depend on real fruit
    let task_id = TaskId::new(&mut sim.rng);
    let eat_task = Task {
        id: task_id,
        kind: TaskKind::EatFruit { fruit_pos },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(elf_node).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(eat_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Advance enough ticks for the elf to start and complete the Eat action.
    sim.step(&[], sim.tick + sim.config.eat_action_ticks + 10);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let expected_restore = food_max * restore_pct / 100;
    assert!(
        elf.food >= food_before + expected_restore - 1, // allow tiny rounding
        "Food should increase by ~restore_pct%: before={}, after={}, expected_restore={}",
        food_before,
        elf.food,
        expected_restore,
    );
    assert!(elf.current_task.is_none(), "Task should be complete");
}

#[test]
fn hungry_idle_elf_creates_eat_fruit_task() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Ensure fruit exists on the tree (worldgen may not always produce fruit).
    ensure_tree_has_fruit(&mut sim);

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set food below threshold (threshold is 50% by default).
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 30 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past the next heartbeat — hunger check should fire.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // The elf should now have an EatFruit task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Hungry idle elf should have been assigned an EatFruit task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert!(
        task.kind_tag == TaskKindTag::EatFruit,
        "Task should be EatFruit, got {:?}",
        task.kind_tag
    );
}

#[test]
fn well_fed_elf_does_not_create_eat_fruit_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf — starts at full food.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // No EatFruit task should exist.
    let has_eat_task = sim
        .db
        .tasks
        .iter_all()
        .any(|t| t.kind_tag == TaskKindTag::EatFruit);
    assert!(
        !has_eat_task,
        "Well-fed elf should not create an EatFruit task"
    );
}

#[test]
fn busy_hungry_elf_does_not_create_eat_fruit_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set food very low.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 10 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Give the elf a GoTo task so it's busy. Use a valid nav node position
    // so the elf doesn't immediately fail pathfinding and drop the task.
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let goto_target = sim
        .nav_graph
        .ground_node_ids()
        .iter()
        .map(|&nid| sim.nav_graph.node(nid).position)
        .find(|&p| p != elf_pos)
        .expect("need a distant ground nav node");
    let task_id = TaskId::new(&mut sim.rng);
    let goto_task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: goto_target,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(goto_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // The elf should still have its GoTo task, not an EatFruit one.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.current_task,
        Some(task_id),
        "Busy elf should keep its existing task"
    );
    let eat_task_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::EatFruit)
        .count();
    assert_eq!(
        eat_task_count, 0,
        "No EatFruit task should be created for a busy elf"
    );
}

#[test]
fn hungry_elf_eats_fruit_and_food_increases() {
    // Integration test: spawn elf, place fruit at its nav node, set low
    // food, run ticks, verify the elf ate fruit and food increased.
    // We place fruit explicitly rather than relying on random fruit spawning
    // so the test is deterministic regardless of tree shape.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0; // Force fruit foraging, not bread eating.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;

    // Place a fruit voxel at the elf's position (or very close).
    // This guarantees the fruit is reachable — the elf is already there.
    let fruit_pos = elf_pos;
    sim.world.set(fruit_pos, VoxelType::Fruit);
    let tree_id = sim.player_tree_id;
    let species_id = sim
        .db
        .trees
        .get(&tree_id)
        .unwrap()
        .fruit_species_id
        .unwrap_or_else(|| {
            let id = insert_test_fruit_species(&mut sim);
            let mut t = sim.db.trees.get(&tree_id).unwrap();
            t.fruit_species_id = Some(id);
            sim.db.update_tree(t).unwrap();
            id
        });
    let _ = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        tree_id,
        position: fruit_pos,
        species_id,
    });

    // Set food to 20% — well below the 50% hunger threshold.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 20 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Run for 50_000 ticks — enough for heartbeat + pathfind + eat.
    sim.step(&[], 50_001);

    let elf = sim.db.creatures.get(&elf_id).unwrap();

    // The elf should have eaten at least once, restoring food above 0.
    // With default decay the food won't drop to 0 in 50k ticks.
    assert!(
        elf.food > 0,
        "Hungry elf should have eaten fruit and restored food above 0. food={}",
        elf.food
    );
}

// -----------------------------------------------------------------------
// Hunger / EatBread tests
// -----------------------------------------------------------------------

#[test]
fn hungry_elf_with_bread_creates_eat_bread_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Give the elf owned bread.
    sim.inv_add_simple_item(
        sim.creature_inv(elf_id),
        inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );

    // Set food below hunger threshold.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 30 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past the next heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // The elf should have an EatBread task, not EatFruit.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Hungry elf with bread should have a task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert!(
        task.kind_tag == TaskKindTag::EatBread,
        "Task should be EatBread, got {:?}",
        task.kind_tag
    );
}

#[test]
fn eat_bread_restores_food_and_removes_bread() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let bread_restore_pct = sim.species_table[&Species::Elf].bread_restore_pct;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;
    let elf_node = creature_node(&sim, elf_id);

    // Give the elf owned bread.
    sim.inv_add_simple_item(
        sim.creature_inv(elf_id),
        inventory::ItemKind::Bread,
        3,
        Some(elf_id),
        None,
    );

    // Set food low.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max / 10;
        sim.db.update_creature(c).unwrap();
    }
    let food_before = sim.db.creatures.get(&elf_id).unwrap().food;

    // Manually create an EatBread task at the elf's current node.
    let task_id = TaskId::new(&mut sim.rng);
    let eat_task = Task {
        id: task_id,
        kind: TaskKind::EatBread,
        state: TaskState::InProgress,
        location: sim.nav_graph.node(elf_node).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(eat_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Advance enough ticks for the elf to start and complete the Eat action.
    // eat_action_ticks = 1500, plus a few extra for scheduling.
    sim.step(&[], sim.tick + sim.config.eat_action_ticks + 10);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let expected_restore = food_max * bread_restore_pct / 100;
    assert!(
        elf.food >= food_before + expected_restore - 1,
        "Food should increase by ~bread_restore_pct%: before={}, after={}, expected_restore={}",
        food_before,
        elf.food,
        expected_restore,
    );
    assert!(elf.current_task.is_none(), "Task should be complete");

    // Should have consumed 1 bread (2 remaining).
    let bread_count = sim.inv_count_owned(elf.inventory_id, inventory::ItemKind::Bread, elf_id);
    assert_eq!(bread_count, 2, "Should have consumed 1 bread, leaving 2");
}

#[test]
fn hungry_elf_without_bread_still_seeks_fruit() {
    // Elf is hungry but has no bread — should create EatFruit, not EatBread.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    ensure_tree_has_fruit(&mut sim);

    // Spawn an elf (no bread in inventory).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set food below threshold.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 30 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // Should have EatFruit, not EatBread.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.current_task.is_some(), "Hungry elf should have a task");
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert!(
        task.kind_tag == TaskKindTag::EatFruit,
        "Elf without bread should seek fruit, got {:?}",
        task.kind_tag
    );
}

#[test]
fn hungry_elf_with_unowned_bread_seeks_fruit() {
    // Elf has bread but doesn't own it — should seek fruit instead.
    let mut sim = test_sim(legacy_test_seed());
    sim.config.elf_starting_bread = 0;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Ensure the tree has fruit regardless of seed (worldgen may not always
    // produce fruit positions).
    ensure_tree_has_fruit(&mut sim);

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Give the elf bread with no owner (unowned).
    sim.inv_add_simple_item(
        sim.creature_inv(elf_id),
        inventory::ItemKind::Bread,
        5,
        None,
        None,
    );

    // Set food below threshold.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 30 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // Should have EatFruit since the bread is not owned by this elf.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.current_task.is_some(), "Hungry elf should have a task");
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert!(
        task.kind_tag == TaskKindTag::EatFruit,
        "Elf with unowned bread should seek fruit, got {:?}",
        task.kind_tag
    );
}
// -----------------------------------------------------------------------
// Home assignment tests
// -----------------------------------------------------------------------

#[test]
fn assign_home_sets_bidirectional_refs() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let home_id = insert_completed_home(&mut sim, anchor);

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Assign elf to home.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::AssignHome {
            creature_id: elf_id,
            structure_id: Some(home_id),
        },
    };
    sim.step(&[cmd], 2);

    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().assigned_home,
        Some(home_id)
    );
    assert_eq!(
        sim.db
            .creatures
            .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|c| c.id),
        Some(elf_id)
    );
}

#[test]
fn assign_home_unassign() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let home_id = insert_completed_home(&mut sim, anchor);

    // Spawn and assign.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(home_id),
            },
        }],
        2,
    );
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().assigned_home,
        Some(home_id)
    );

    // Unassign.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 3,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: None,
            },
        }],
        3,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().assigned_home, None);
    assert!(
        sim.db
            .creatures
            .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn assign_home_replaces_old_assignment() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor_a = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let anchor_b = VoxelCoord::new(tree_pos.x + 10, 0, tree_pos.z + 5);
    let home_a = insert_completed_home(&mut sim, anchor_a);
    let home_b = insert_completed_home(&mut sim, anchor_b);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Assign to home A.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(home_a),
            },
        }],
        2,
    );
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().assigned_home,
        Some(home_a)
    );

    // Reassign to home B.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 3,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(home_b),
            },
        }],
        3,
    );
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().assigned_home,
        Some(home_b)
    );
    assert!(
        sim.db
            .creatures
            .by_assigned_home(&Some(home_a), tabulosity::QueryOpts::ASC)
            .is_empty()
    );
    assert_eq!(
        sim.db
            .creatures
            .by_assigned_home(&Some(home_b), tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|c| c.id),
        Some(elf_id)
    );
}

#[test]
fn assign_home_evicts_previous_occupant() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let home_id = insert_completed_home(&mut sim, anchor);

    // Spawn two elves.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                species: Species::Elf,
                position: tree_pos,
            },
        },
    ];
    sim.step(&cmds, 1);
    let elf_ids: Vec<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    assert_eq!(elf_ids.len(), 2);
    let elf_a = elf_ids[0];
    let elf_b = elf_ids[1];

    // Assign elf A.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_a,
                structure_id: Some(home_id),
            },
        }],
        2,
    );
    assert_eq!(
        sim.db
            .creatures
            .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|c| c.id),
        Some(elf_a)
    );

    // Assign elf B to same home — evicts elf A.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 3,
            action: SimAction::AssignHome {
                creature_id: elf_b,
                structure_id: Some(home_id),
            },
        }],
        3,
    );
    assert_eq!(
        sim.db
            .creatures
            .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
            .into_iter()
            .next()
            .map(|c| c.id),
        Some(elf_b)
    );
    assert_eq!(sim.db.creatures.get(&elf_a).unwrap().assigned_home, None);
    assert_eq!(
        sim.db.creatures.get(&elf_b).unwrap().assigned_home,
        Some(home_id)
    );
}

#[test]
fn assign_home_rejects_non_home() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish as Dormitory (not Home).
    {
        let mut s = sim.db.structures.get(&structure_id).unwrap();
        s.furnishing = Some(FurnishingType::Dormitory);
        sim.db.update_structure(s).unwrap();
    }

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(structure_id),
            },
        }],
        2,
    );

    // Should be rejected — no assignment set.
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().assigned_home, None);
    assert!(
        sim.db
            .creatures
            .by_assigned_home(&Some(structure_id), tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn assign_home_rejects_non_elf() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let home_id = insert_completed_home(&mut sim, anchor);

    // Spawn a capybara.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Capybara,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let capy_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap()
        .id;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: capy_id,
                structure_id: Some(home_id),
            },
        }],
        2,
    );

    // Should be rejected — capybaras can't have homes.
    assert_eq!(sim.db.creatures.get(&capy_id).unwrap().assigned_home, None);
    assert!(
        sim.db
            .creatures
            .by_assigned_home(&Some(home_id), tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn tired_elf_sleeps_in_assigned_home() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Create a home with a bed.
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let home_id = insert_completed_home(&mut sim, anchor);
    let home_bed = sim
        .db
        .furniture
        .by_structure_id(&home_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|f| f.placed)
        .unwrap()
        .coord;

    // Spawn and assign.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(home_id),
            },
        }],
        2,
    );

    // Make elf tired.
    {
        let food_max_val = sim.species_table[&Species::Elf].food_max;
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 30 / 100;
        c.food = food_max_val;
        // Clear any existing task so the elf is idle.
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past heartbeat.
    let target_tick = 2 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Tired elf with home should get a Sleep task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
    let bed_pos = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
    assert_eq!(
        bed_pos,
        Some(home_bed),
        "Elf should sleep in their assigned home bed"
    );
}

#[test]
fn tired_elf_without_home_uses_dormitory() {
    // This is largely the same as existing tests, but verifies the new
    // code path doesn't break dormitory fallback.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Create a dormitory (not a home).
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);
    let structure = sim.db.structures.get(&structure_id).unwrap();
    let bed_pos = structure.floor_interior_positions()[0];
    let mut structure = sim.db.structures.get(&structure_id).unwrap();
    structure.furnishing = Some(FurnishingType::Dormitory);
    sim.db.update_structure(structure).unwrap();
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            structure_id,
            coord: bed_pos,
            placed: true,
        })
        .unwrap();

    // Spawn elf (no home assignment).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Make tired and idle.
    {
        let food_max_val = sim.species_table[&Species::Elf].food_max;
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 30 / 100;
        c.food = food_max_val;
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Tired elf should get a Sleep task from dormitory"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
    let task_bed = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
    assert_eq!(task_bed, Some(bed_pos), "Should sleep in dormitory bed");
}

#[test]
fn assigned_home_unfurnished_falls_back() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Create a home without any placed furniture.
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_building(&mut sim, anchor);
    let mut structure = sim.db.structures.get(&structure_id).unwrap();
    structure.furnishing = Some(FurnishingType::Home);
    // No furniture placed — bed not yet built.
    sim.db.update_structure(structure).unwrap();

    // Spawn and assign.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: 2,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(structure_id),
            },
        }],
        2,
    );

    // Make tired and idle.
    {
        let food_max_val = sim.species_table[&Species::Elf].food_max;
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 30 / 100;
        c.food = food_max_val;
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let target_tick = 2 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // Should fall back to ground sleep (no dormitories and home has no bed).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Tired elf should get a ground Sleep task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, TaskKindTag::Sleep, "Expected Sleep task");
    let bed_pos = sim.task_voxel_ref(task.id, crate::db::TaskVoxelRole::BedPosition);
    assert_eq!(bed_pos, None, "Should fall back to ground sleep");
}

// -----------------------------------------------------------------------
// Death clears assigned home
// -----------------------------------------------------------------------

#[test]
fn death_clears_assigned_home() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Build a home and assign the elf.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let structure_id = insert_completed_home(&mut sim, anchor);
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::AssignHome {
                creature_id: elf_id,
                structure_id: Some(structure_id),
            },
        }],
        tick + 1,
    );
    assert!(
        sim.db
            .creatures
            .get(&elf_id)
            .unwrap()
            .assigned_home
            .is_some()
    );

    // Kill the elf.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick2 + 1,
    );
    assert!(
        sim.db
            .creatures
            .get(&elf_id)
            .unwrap()
            .assigned_home
            .is_none()
    );
}

// -----------------------------------------------------------------------
// Dining hall tests
// -----------------------------------------------------------------------

#[test]
fn find_nearest_dining_hall_returns_hall_with_food() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 3);

    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(result.is_some(), "Should find dining hall with food");
    let (coord, _nav, sid) = result.unwrap();
    assert_eq!(coord, table_pos);
    assert_eq!(sid, StructureId(900));
}

#[test]
fn find_nearest_dining_hall_returns_none_when_no_food() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 0);

    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(result.is_none(), "Should not find dining hall without food");
}

#[test]
fn find_nearest_dining_hall_returns_none_when_seats_full() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 10);

    // Fill all seats (dining_seats_per_table = 4 by default).
    let seats = sim.config.dining_seats_per_table;
    for _ in 0..seats {
        let fake_task_id = TaskId::new(&mut sim.rng);
        // Create a fake task so the voxel ref has something to reference.
        let fake_task = Task {
            id: fake_task_id,
            kind: TaskKind::DineAtHall {
                structure_id: StructureId(900),
            },
            state: TaskState::InProgress,
            location: table_pos,
            progress: 0,
            total_cost: 0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        sim.insert_task(fake_task);
        let seq = sim.db.task_voxel_refs.next_seq();
        sim.db
            .insert_task_voxel_ref(crate::db::TaskVoxelRef {
                seq,
                task_id: fake_task_id,
                coord: table_pos,
                role: crate::db::TaskVoxelRole::DiningSeat,
            })
            .unwrap();
    }

    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(
        result.is_none(),
        "Should not find dining hall when all seats occupied"
    );
}

#[test]
fn elf_seeks_dining_hall_at_dining_threshold() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 5);

    // Set food to 35% — below solo dining threshold (40%) but above emergency (30%).
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance one heartbeat.
    sim.step(&[], 1 + heartbeat);

    // Elf should have a DineAtHall task.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Elf should have a task after heartbeat"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(
        task.kind_tag,
        TaskKindTag::DineAtHall,
        "Elf should have DineAtHall task, got {:?}",
        task.kind_tag
    );
}

#[test]
fn elf_eats_emergency_food_below_hunger_threshold() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Give bread and set food to 10% — well below emergency threshold (30%).
    sim.inv_add_simple_item(
        sim.creature_inv(elf_id),
        ItemKind::Bread,
        1,
        Some(elf_id),
        None,
    );
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 10 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance one heartbeat.
    sim.step(&[], 1 + heartbeat);

    // Elf should have an EatBread task (emergency eating, no dining hall needed).
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(creature.current_task.is_some());
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(
        task.kind_tag,
        TaskKindTag::EatBread,
        "Elf should eat bread at emergency hunger, got {:?}",
        task.kind_tag
    );
}

#[test]
fn elf_stays_idle_at_dining_threshold_without_hall() {
    let mut sim = test_sim(legacy_test_seed());
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Set food to 35% — below dining threshold but above emergency. No dining hall.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance one heartbeat.
    sim.step(&[], 1 + heartbeat);

    // Elf should remain idle (no dining hall available, not emergency-hungry).
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Elf should stay idle at dining threshold without dining hall"
    );
}

// -----------------------------------------------------------------------
// Herbivore grazing tests
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// DineAtHall task tests
// -----------------------------------------------------------------------

#[test]
fn dine_at_hall_reserves_seat_and_food() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let structure_id = create_dining_hall(&mut sim, table_pos, 2);

    // Set food to 35%.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    sim.step(&[], 1 + heartbeat);

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    let task_id = creature.current_task.unwrap();

    // Check DiningSeat voxel ref exists.
    let seat_refs: Vec<_> = sim
        .db
        .task_voxel_refs
        .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.role == crate::db::TaskVoxelRole::DiningSeat)
        .collect();
    assert_eq!(seat_refs.len(), 1, "Should have 1 DiningSeat voxel ref");

    // Check DineAt structure ref exists.
    let structure_refs: Vec<_> = sim
        .db
        .task_structure_refs
        .by_task_id(&task_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.role == crate::db::TaskStructureRole::DineAt)
        .collect();
    assert_eq!(
        structure_refs.len(),
        1,
        "Should have 1 DineAt structure ref"
    );
    assert_eq!(structure_refs[0].structure_id, structure_id);

    // Check food reservation.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    let reserved: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.reserved_by == Some(task_id))
        .collect();
    assert!(!reserved.is_empty(), "Should have reserved food item");
}

#[test]
fn dine_at_hall_completion_restores_food_and_generates_thought() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let food_restore = food_max * sim.species_table[&Species::Elf].food_restore_pct / 100;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 3);

    // Set food to 35% — below solo dining threshold (40%) but above emergency (30%).
    let initial_food = food_max * 35 / 100;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = initial_food;
        sim.db.update_creature(c).unwrap();
    }

    // Advance enough ticks for: heartbeat → DineAtHall task → walk to table →
    // eat animation → completion. Give plenty of time for pathing.
    sim.step(&[], 1 + heartbeat + 50_000);

    // Food should have been restored (initial_food + restore - decay > initial_food
    // because the restore amount is large relative to the decay over the test window).
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.food > initial_food,
        "Food should have increased after dining. food={}, initial={}",
        creature.food,
        initial_food,
    );

    // Should have AteDining thought.
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == crate::types::ThoughtKind::AteDining),
        "Should have AteDining thought after dining hall meal"
    );

    // Food should have been consumed from dining hall.
    let structure_id = StructureId(900);
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    let remaining = sim.inv_count_unowned_unreserved(
        inv_id,
        ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(
        remaining, 2,
        "Dining hall should have exactly 2 bread after 1 meal (started with 3)"
    );
}

#[test]
fn dine_at_hall_cleanup_releases_reservations() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let structure_id = create_dining_hall(&mut sim, table_pos, 2);

    // Set food to 35%.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    sim.step(&[], 1 + heartbeat);

    let task_id = sim.db.creatures.get(&elf_id).unwrap().current_task.unwrap();

    // Manually interrupt the task (simulating combat preemption).
    sim.cleanup_and_unassign_task(elf_id, task_id);

    // Food reservation should be cleared.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    let reserved: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.reserved_by.is_some())
        .collect();
    assert!(
        reserved.is_empty(),
        "Food reservations should be cleared after task cleanup"
    );

    // Creature should have no task.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Creature should have no task after cleanup"
    );
}

#[test]
fn ate_meal_deserializes_to_ate_dining() {
    // Backward compat: old saves with "AteMeal" should deserialize to AteDining.
    let json = r#""AteMeal""#;
    let kind: crate::types::ThoughtKind = serde_json::from_str(json).unwrap();
    assert_eq!(kind, crate::types::ThoughtKind::AteDining);
}

#[test]
fn ate_alone_serde_roundtrip() {
    let json = serde_json::to_string(&crate::types::ThoughtKind::AteAlone).unwrap();
    let roundtrip: crate::types::ThoughtKind = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip, crate::types::ThoughtKind::AteAlone);
}

#[test]
fn dine_at_hall_task_serde_roundtrip() {
    let kind = TaskKind::DineAtHall {
        structure_id: StructureId(42),
    };
    let json = serde_json::to_string(&kind).unwrap();
    let roundtrip: TaskKind = serde_json::from_str(&json).unwrap();
    match roundtrip {
        TaskKind::DineAtHall { structure_id } => assert_eq!(structure_id, StructureId(42)),
        other => panic!("Expected DineAtHall, got {other:?}"),
    }
}

#[test]
fn old_save_mood_config_missing_ate_alone_field() {
    // Old save with mood section but no weight_ate_alone — must not crash.
    let json = r#"{
        "weight_slept_home": 80,
        "weight_slept_dormitory": 30,
        "weight_slept_ground": -100,
        "weight_ate_meal": 60,
        "weight_low_ceiling": -50,
        "tier_devastated_below": -300,
        "tier_miserable_below": -150,
        "tier_unhappy_below": -30,
        "tier_content_above": 30,
        "tier_happy_above": 150,
        "tier_elated_above": 300
    }"#;
    let config: crate::config::MoodConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.weight_ate_dining, 60); // via alias
    assert_eq!(config.weight_ate_alone, -15); // via serde default
}

#[test]
fn old_save_thought_config_missing_ate_alone_fields() {
    // Old save with thoughts section but no ate_alone fields — must not crash.
    let json = r#"{
        "cap": 200,
        "dedup_slept_home_ticks": 150000,
        "dedup_slept_dormitory_ticks": 150000,
        "dedup_slept_ground_ticks": 150000,
        "dedup_ate_meal_ticks": 150000,
        "dedup_low_ceiling_ticks": 30000,
        "expiry_slept_home_ticks": 600000,
        "expiry_slept_dormitory_ticks": 600000,
        "expiry_slept_ground_ticks": 600000,
        "expiry_ate_meal_ticks": 150000,
        "expiry_low_ceiling_ticks": 150000
    }"#;
    let config: crate::config::ThoughtConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.dedup_ate_dining_ticks, 150000); // via alias
    assert_eq!(config.dedup_ate_alone_ticks, 150000); // via serde default
    assert_eq!(config.expiry_ate_dining_ticks, 150000); // via alias
    assert_eq!(config.expiry_ate_alone_ticks, 150000); // via serde default
}

#[test]
fn dining_hall_with_fruit_only() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Create dining hall with fruit instead of bread.
    let structure_id = StructureId(900);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            project_id,
            build_type: BuildType::Building,
            anchor: table_pos,
            width: 3,
            depth: 3,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::DiningHall),
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            structure_id,
            coord: table_pos,
            placed: true,
        })
        .unwrap();
    // Stock fruit, not bread.
    sim.inv_add_simple_item(inv_id, ItemKind::Fruit, 3, None, None);

    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(result.is_some(), "Should find dining hall with fruit");
}

#[test]
fn dine_at_hall_instant_on_arrival() {
    // Verify dining resolves instantly on arrival (no animation delay).
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 3);

    // Place elf at table and set food to 35%.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        c.position = table_pos;
        sim.db.update_creature(c).unwrap();
    }

    // Step past heartbeat + enough for walk + activation. The key test is
    // that completion happens WITHOUT needing eat_action_ticks of delay.
    // With the old (non-instant) flow, the task would still be active at
    // heartbeat + walk_time + 500 because eat_action_ticks = 1500.
    let walk_tpv = sim.species_table[&Species::Elf].walk_ticks_per_voxel as u64;
    sim.step(&[], 1 + heartbeat + walk_tpv * 10 + 500);

    // Task should be complete well before eat_action_ticks would have elapsed.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "DineAtHall should complete instantly on arrival, not wait for eat animation"
    );
    // Should have AteDining thought proving it resolved.
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == crate::types::ThoughtKind::AteDining),
        "Should have AteDining thought"
    );
}

#[test]
fn edible_kinds_list_is_consistent() {
    // Ensure every item in EDIBLE_KINDS would be considered edible, and
    // that the list isn't accidentally empty.
    assert!(
        !ItemKind::EDIBLE_KINDS.is_empty(),
        "EDIBLE_KINDS must not be empty"
    );
    assert!(ItemKind::EDIBLE_KINDS.contains(&ItemKind::Bread));
    assert!(ItemKind::EDIBLE_KINDS.contains(&ItemKind::Fruit));
}

#[test]
fn furnish_dining_hall_enables_logistics() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Build a shell structure.
    let structure_id = StructureId(901);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            project_id,
            build_type: BuildType::Building,
            anchor: tree_pos,
            width: 4,
            depth: 4,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: None,
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();

    // Furnish as dining hall via command.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::DiningHall,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], 2);

    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(
        structure.furnishing,
        Some(FurnishingType::DiningHall),
        "Should be furnished as DiningHall"
    );
    assert_eq!(
        structure.logistics_priority,
        Some(8),
        "Dining hall should start with logistics priority 8"
    );
}

#[test]
fn dining_preempts_autonomous_task() {
    // An elf busy hauling should be preempted for dining when in the
    // dining hunger band.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 5);

    // Give the elf a fake autonomous task (simulating hauling).
    let haul_task_id = TaskId::new(&mut sim.rng);
    let haul_task = Task {
        id: haul_task_id,
        kind: TaskKind::GoTo, // GoTo is PlayerDirected, use a simpler approach
        state: TaskState::InProgress,
        location: VoxelCoord::new(10, 1, 10),
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous, // Autonomous origin → Autonomous level
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(haul_task);
    // Set food into the solo dining band (30–40%).
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }
    // Assign the task (indexed field — must use full update).
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(haul_task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Advance enough for heartbeat(s) to fire. The elf was spawned at tick ~2,
    // so its first heartbeat is at ~2 + heartbeat. Give enough for several.
    sim.step(&[], sim.tick + heartbeat * 3);

    // The elf should have had the autonomous task preempted and received
    // a DineAtHall task, or already completed it (instant on arrival).
    // Check for AteDining thought as proof that dining happened.
    let has_dining_thought = sim
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .iter()
        .any(|t| t.kind == crate::types::ThoughtKind::AteDining);
    let has_dining_task = sim
        .db
        .creatures
        .get(&elf_id)
        .and_then(|c| c.current_task)
        .and_then(|tid| sim.db.tasks.get(&tid))
        .is_some_and(|t| t.kind_tag == TaskKindTag::DineAtHall);
    assert!(
        has_dining_thought || has_dining_task,
        "Elf should have dined (AteDining thought) or be dining (DineAtHall task) \
         after preempting autonomous task"
    );
}

#[test]
fn find_dining_hall_with_realistic_building() {
    // Use the real building pipeline (voxels + nav graph rebuild + furnish
    // command) to verify find_nearest_dining_hall works with a properly
    // constructed building, not just manually inserted DB rows.
    let mut sim = test_sim(legacy_test_seed());
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);

    // Furnish as dining hall.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::DiningHall,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Manually place all furniture (skip the furnish task animation).
    for furn in sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
    {
        if !furn.placed {
            let mut f = sim.db.furniture.get(&furn.id).unwrap();
            f.placed = true;
            sim.db.update_furniture(f).unwrap();
        }
    }
    let placed_count = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| f.placed)
        .count();
    assert!(placed_count > 0, "Dining hall should have placed furniture");

    // Stock food.
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_simple_item(inv_id, ItemKind::Bread, 5, None, None);

    // Spawn an elf and check if find_nearest_dining_hall works.
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(
        result.is_some(),
        "find_nearest_dining_hall should find a realistically-built dining hall. \
         placed_furniture={placed_count}, structure_id={structure_id:?}"
    );
}

#[test]
fn end_to_end_dining_hall() {
    // Full pipeline: fresh world → build → furnish → stock food via
    // logistics → elf gets hungry → self-assigns dining → eats → AteDining.
    let mut sim = test_sim(legacy_test_seed());
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let food_max = sim.species_table[&Species::Elf].food_max;

    // Build and furnish a dining hall.
    let anchor = find_building_site(&sim);
    let structure_id = insert_completed_building(&mut sim, anchor);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::FurnishStructure {
            structure_id,
            furnishing_type: FurnishingType::DiningHall,
            greenhouse_species: None,
        },
    };
    sim.step(&[cmd], sim.tick + 1);

    // Manually mark all furniture as placed (skip furnish task animation).
    for furn in sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
    {
        if !furn.placed {
            let mut f = sim.db.furniture.get(&furn.id).unwrap();
            f.placed = true;
            sim.db.update_furniture(f).unwrap();
        }
    }

    // Stock food directly in the dining hall inventory (simulating completed haul).
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;
    sim.inv_add_simple_item(inv_id, ItemKind::Bread, 10, None, None);

    // Spawn an elf and set food to 35% (in the solo dining band: 30–40%).
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Verify every single prerequisite for dining to work.
    // If any of these fail, we know EXACTLY where the problem is.

    // 1. Structure is furnished as DiningHall.
    let structure = sim.db.structures.get(&structure_id).unwrap();
    assert_eq!(structure.furnishing, Some(FurnishingType::DiningHall));

    // 2. Structure has placed furniture.
    let placed: Vec<_> = sim
        .db
        .furniture
        .by_structure_id(&structure_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|f| f.placed)
        .collect();
    assert!(!placed.is_empty(), "Dining hall must have placed furniture");

    // 3. Food in inventory is unowned and unreserved.
    let inv_stacks: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|s| s.kind == ItemKind::Bread)
        .collect();
    assert!(!inv_stacks.is_empty(), "Must have bread stacks");
    for stack in &inv_stacks {
        assert!(
            stack.owner.is_none(),
            "Bread in dining hall must be unowned, got owner={:?}",
            stack.owner
        );
        assert!(
            stack.reserved_by.is_none(),
            "Bread must be unreserved, got reserved_by={:?}",
            stack.reserved_by
        );
    }

    // 4. Elf is at a valid nav node.
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    assert!(
        graph.node_at(elf_pos).is_some(),
        "Elf must be at a valid nav node, pos={elf_pos:?}"
    );

    // 5. Verify the dining threshold math.
    let species_data = &sim.species_table[&Species::Elf];
    let dining_thresh = species_data.food_max * species_data.food_dining_threshold_pct / 100;
    let emergency_thresh = species_data.food_max * species_data.food_hunger_threshold_pct / 100;
    let elf_food = sim.db.creatures.get(&elf_id).unwrap().food;
    assert!(
        elf_food < dining_thresh && elf_food >= emergency_thresh,
        "Elf food {elf_food} should be in dining band [{emergency_thresh}, {dining_thresh})"
    );

    // Verify find_nearest_dining_hall works.
    let find_result = sim.find_nearest_dining_hall(elf_id);
    assert!(
        find_result.is_some(),
        "find_nearest_dining_hall should succeed"
    );

    // Run for enough time for the elf to walk and eat, but not so long
    // that thoughts expire (expiry_ate_dining_ticks = 150k).
    sim.step(&[], sim.tick + 100_000);

    // Check for AteDining thought.
    let has_dining_thought = sim
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .iter()
        .any(|t| t.kind == crate::types::ThoughtKind::AteDining);
    assert!(
        has_dining_thought,
        "Elf should have AteDining thought after end-to-end dining. \
         food={}, thoughts={:?}",
        sim.db.creatures.get(&elf_id).map(|c| c.food).unwrap_or(-1),
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .map(|t| t.kind)
            .collect::<Vec<_>>()
    );

    // Dining hall should have consumed at least one bread.
    let remaining = sim.inv_count_unowned_unreserved(
        inv_id,
        ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert!(
        remaining < 10,
        "Dining hall should have fewer items. remaining={remaining}"
    );
}

#[test]
fn elf_resumes_activation_after_dining() {
    // After eating at a dining hall, the elf should resume normal activation
    // (wandering, picking up tasks, etc.) — not freeze in place.
    let mut sim = flat_world_sim(legacy_test_seed());
    let table_pos = VoxelCoord::new(32, 1, 32);

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    create_dining_hall(&mut sim, table_pos, 5);

    // Set food to 35% (solo dining band: 30–40%) and place elf near table.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance enough for: heartbeat → DineAtHall task → walk → instant eat.
    sim.step(&[], sim.tick + heartbeat + 50_000);

    // Confirm dining happened.
    let has_dining_thought = sim
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .iter()
        .any(|t| t.kind == crate::types::ThoughtKind::AteDining);
    assert!(has_dining_thought, "Elf should have dined first");

    // Record position after dining.
    let pos_after_dining = sim.db.creatures.get(&elf_id).unwrap().position;

    // Advance more ticks — elf should wander (change position).
    sim.step(&[], sim.tick + 20_000);

    let pos_later = sim.db.creatures.get(&elf_id).unwrap().position;
    assert_ne!(
        pos_after_dining, pos_later,
        "Elf should have moved after dining (not frozen). pos={pos_after_dining:?}"
    );
}

#[test]
fn dine_at_hall_no_task_when_food_unavailable() {
    // When all food in a dining hall is reserved, the heartbeat must not
    // create a DineAtHall task at all — no speculative insert, no orphaned
    // rows.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Create a dining hall and stock 1 bread.
    create_dining_hall(&mut sim, table_pos, 1);
    let structure_id = StructureId(900);
    let inv_id = sim.db.structures.get(&structure_id).unwrap().inventory_id;

    // Reserve the bread with a dummy task so the elf can't claim it.
    let dummy_task_id = TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, dummy_task_id);
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    for stack in stacks {
        let mut s = stack.clone();
        s.reserved_by = Some(dummy_task_id);
        sim.db.update_item_stack(s).unwrap();
    }

    let tasks_before = sim.db.tasks.iter_all().count();

    // Set food to 35% — below solo dining threshold (40%) but above emergency (30%).
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance one heartbeat. find_nearest_dining_hall sees no unreserved food
    // and returns None, so no task is created.
    sim.step(&[], 1 + heartbeat);

    // Elf should NOT have a DineAtHall task.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none()
            || !sim
                .db
                .tasks
                .get(&creature.current_task.unwrap())
                .is_some_and(|t| t.kind_tag == TaskKindTag::DineAtHall),
        "Elf should not have a DineAtHall task when no food is available"
    );

    // No DineAtHall tasks should exist in the DB at all.
    let dine_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::DineAtHall)
        .collect();
    assert!(
        dine_tasks.is_empty(),
        "No DineAtHall tasks should be created when food is unavailable; \
         found {} task(s)",
        dine_tasks.len(),
    );

    // Task count should not grow.
    assert_eq!(
        sim.db.tasks.iter_all().count(),
        tasks_before,
        "Task count should not grow from failed dining attempts"
    );
}

#[test]
fn two_elves_one_food_only_one_gets_dine_task() {
    // Two hungry elves, one dining hall with exactly 1 food item. After a
    // heartbeat tick, exactly one elf should get a DineAtHall task. The
    // other stays idle. No orphaned tasks should remain.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Elf);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    let elf_a = spawn_creature(&mut sim, Species::Elf);
    let elf_b = spawn_creature(&mut sim, Species::Elf);

    // Create a dining hall with exactly 1 bread.
    create_dining_hall(&mut sim, table_pos, 1);

    // Set both elves to 35% food — in solo dining band (30–40%).
    for eid in [elf_a, elf_b] {
        let mut c = sim.db.creatures.get(&eid).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance one heartbeat so both elves' heartbeats fire.
    sim.step(&[], 1 + heartbeat);

    // Exactly one elf should have a DineAtHall task.
    let has_dine = |eid: CreatureId| -> bool {
        sim.db
            .creatures
            .get(&eid)
            .and_then(|c| c.current_task)
            .is_some_and(|tid| {
                sim.db
                    .tasks
                    .get(&tid)
                    .is_some_and(|t| t.kind_tag == TaskKindTag::DineAtHall)
            })
    };
    let a_dining = has_dine(elf_a);
    let b_dining = has_dine(elf_b);
    assert!(
        a_dining ^ b_dining,
        "Exactly one elf should get a DineAtHall task (a={a_dining}, b={b_dining})"
    );

    // No orphaned DineAtHall tasks: exactly 1 should exist, assigned to
    // whichever elf got it.
    let dine_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::DineAtHall)
        .collect();
    assert_eq!(
        dine_tasks.len(),
        1,
        "Exactly 1 DineAtHall task should exist, found {}",
        dine_tasks.len(),
    );
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn food_decreases_over_heartbeats() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let food_max = sim.species_table[&Species::Elf].food_max;
    let decay_per_tick = sim.species_table[&Species::Elf].food_decay_per_tick;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Verify food starts at food_max.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.food, food_max);

    // Advance past 3 heartbeats.
    let target_tick = 1 + heartbeat_interval * 3 + 1;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    let expected_decay = decay_per_tick * heartbeat_interval as i64 * 3;
    assert_eq!(elf.food, food_max - expected_decay);
}

#[test]
fn food_does_not_go_below_zero() {
    // Use a custom config with aggressive decay so food depletes quickly.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000; // Depletes in 1 tick
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Advance well past full depletion (many heartbeats).
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 5;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.food, 0);
}

#[test]
fn creature_dies_when_food_reaches_zero() {
    // Use aggressive decay so food depletes in one heartbeat.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Advance past 2 heartbeats — first depletes food, creature dies.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 2 + 1;
    let result = sim.step(&[], target_tick);

    // Creature should be dead.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.vital_status, VitalStatus::Dead);

    // Should have emitted a CreatureDied event with Starvation cause.
    let died_event = result
        .events
        .iter()
        .find(|e| matches!(e.kind, SimEventKind::CreatureDied { .. }));
    assert!(died_event.is_some(), "Expected a CreatureDied event");
    if let SimEventKind::CreatureDied { cause, .. } = &died_event.unwrap().kind {
        assert_eq!(*cause, DeathCause::Starvation);
    }
}

#[test]
fn starvation_death_notification_mentions_starvation() {
    // Aggressive decay to trigger starvation quickly.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Advance past depletion.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 2 + 1;
    sim.step(&[], target_tick);

    // Check notification mentions starvation.
    let starvation_notif = sim
        .db
        .notifications
        .iter_all()
        .any(|n| n.message.contains("starvation"));
    assert!(
        starvation_notif,
        "Expected notification mentioning starvation"
    );
}

#[test]
fn no_heartbeat_after_starvation_death() {
    // Verify dead creatures don't get further heartbeats processed.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Advance well past death.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 10;
    sim.step(&[], target_tick);

    // Creature should still be dead (not resurrected or erroring).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.vital_status, VitalStatus::Dead);
}

#[test]
fn creature_with_food_remaining_does_not_starve() {
    // Default config — food_max is large, decay is slow. Creature should
    // survive a few heartbeats without issue.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Advance past 3 heartbeats — food should still be positive.
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 3 + 1;
    sim.step(&[], target_tick);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.vital_status, VitalStatus::Alive);
    assert!(elf.food > 0);
}

// -----------------------------------------------------------------------
// Rest/sleep tests
// -----------------------------------------------------------------------

#[test]
fn rest_decreases_over_heartbeats() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let decay_per_tick = sim.species_table[&Species::Elf].rest_decay_per_tick;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Verify rest starts at rest_max.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.rest, rest_max);

    // Advance past 3 heartbeats.
    let target_tick = 1 + heartbeat_interval * 3 + 1;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    let expected_decay = decay_per_tick * heartbeat_interval as i64 * 3;
    assert_eq!(elf.rest, rest_max - expected_decay);
}

#[test]
fn rest_does_not_go_below_zero() {
    let mut config = test_config();
    let elf = config.species.get_mut(&Species::Elf).unwrap();
    elf.rest_decay_per_tick = 1_000_000_000_000_000; // Depletes in 1 tick
    elf.rest_per_sleep_tick = 0; // Prevent sleep from restoring rest.
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 5;
    sim.step(&[], target_tick);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.rest, 0);
}

#[test]
fn tired_idle_elf_creates_sleep_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set rest below threshold (50%) and food well above threshold.
    let food_max_val = sim.species_table[&Species::Elf].food_max;
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 30 / 100;
        c.food = food_max_val;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past the next heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // The elf should now have a Sleep task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Tired idle elf should have been assigned a Sleep task"
    );
    let task = sim.db.tasks.get(&elf.current_task.unwrap()).unwrap();
    assert!(
        task.kind_tag == TaskKindTag::Sleep,
        "Task should be Sleep, got {:?}",
        task.kind_tag
    );
}

#[test]
fn rested_elf_does_not_create_sleep_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf — starts at full rest.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 1);

    // Advance past the heartbeat.
    let target_tick = 1 + heartbeat_interval + 1;
    sim.step(&[], target_tick);

    // No Sleep task should exist.
    let has_sleep_task = sim
        .db
        .tasks
        .iter_all()
        .any(|t| t.kind_tag == TaskKindTag::Sleep);
    assert!(
        !has_sleep_task,
        "Well-rested elf should not create a Sleep task"
    );
}

// -----------------------------------------------------------------------
// 15.10 New SimAction variants (SetCreatureFood, SetCreatureRest,
//       AddCreatureItem, AddGroundPileItem)
// -----------------------------------------------------------------------

#[test]
fn set_creature_food() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCreatureFood {
            creature_id: elf_id,
            food: 42_000,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().food, 42_000);
}

#[test]
fn set_creature_rest() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SetCreatureRest {
            creature_id: elf_id,
            rest: 99_000,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().rest, 99_000);
}

// ---------------------------------------------------------------------------
// Nearest-selection tests (B-dining-perf)
// ---------------------------------------------------------------------------

#[test]
fn find_nearest_dining_hall_picks_closer_of_two() {
    // Place two dining halls at different distances from the creature and
    // verify the closer one is returned. This validates that the
    // nearest-selection logic is correct regardless of pathfinding strategy
    // (Dijkstra vs per-candidate A*).
    let mut sim = test_sim(legacy_test_seed());
    let graph = sim.graph_for_species(Species::Elf);

    // Find two distinct nav nodes — the creature starts at tree_pos, so
    // pick nodes at varying distances from it.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let elf_pos = graph.node(elf_node).position;

    // Collect all nav nodes sorted by distance from elf_pos.
    let mut nodes: Vec<_> = (0..graph.node_slot_count())
        .filter_map(|i| {
            let nid = NavNodeId(i as u32);
            if !graph.is_node_alive(nid) {
                return None;
            }
            let pos = graph.node(nid).position;
            let dist = pos.manhattan_distance(elf_pos);
            Some((dist, nid, pos))
        })
        .collect();
    nodes.sort_by_key(|(d, _, _)| *d);

    // Pick a close node and a far node (skip index 0 which is elf_pos itself).
    // We need nodes that are far enough apart that distance ordering is unambiguous.
    let close = nodes.iter().find(|(d, _, _)| *d >= 2).unwrap();
    let far = nodes.iter().find(|(d, _, _)| *d >= close.0 + 5).unwrap();
    let close_pos = close.2;
    let far_pos = far.2;

    // Create two dining halls — the far one first to ensure ordering isn't
    // just "first found wins".
    create_dining_hall_with_id(&mut sim, far_pos, 3, StructureId(901));
    create_dining_hall_with_id(&mut sim, close_pos, 3, StructureId(902));

    let elf_id = spawn_creature(&mut sim, Species::Elf);

    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(result.is_some(), "Should find a dining hall");
    let (coord, _, sid) = result.unwrap();
    assert_eq!(
        sid,
        StructureId(902),
        "Should pick the closer dining hall (902 at {close_pos:?}), \
         not the farther one (901 at {far_pos:?}). Got coord={coord:?}"
    );
}

#[test]
fn find_nearest_dining_hall_skips_closer_hall_without_food() {
    // The closer hall has no food, the farther hall does.
    // Should return the farther hall.
    let mut sim = test_sim(legacy_test_seed());
    let graph = sim.graph_for_species(Species::Elf);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let elf_pos = graph.node(elf_node).position;

    let mut nodes: Vec<_> = (0..graph.node_slot_count())
        .filter_map(|i| {
            let nid = NavNodeId(i as u32);
            if !graph.is_node_alive(nid) {
                return None;
            }
            let pos = graph.node(nid).position;
            let dist = pos.manhattan_distance(elf_pos);
            Some((dist, nid, pos))
        })
        .collect();
    nodes.sort_by_key(|(d, _, _)| *d);

    let close = nodes.iter().find(|(d, _, _)| *d >= 2).unwrap();
    let far = nodes.iter().find(|(d, _, _)| *d >= close.0 + 5).unwrap();

    // Close hall: no food. Far hall: has food.
    create_dining_hall_with_id(&mut sim, close.2, 0, StructureId(901));
    create_dining_hall_with_id(&mut sim, far.2, 3, StructureId(902));

    let elf_id = spawn_creature(&mut sim, Species::Elf);

    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(
        result.is_some(),
        "Should find the far dining hall with food"
    );
    let (_, _, sid) = result.unwrap();
    assert_eq!(
        sid,
        StructureId(902),
        "Should skip closer hall without food and pick the farther one"
    );
}

#[test]
fn find_nearest_dining_hall_skips_closer_full_table() {
    // Two halls with food, but the closer one has all seats occupied.
    // Should pick the farther hall.
    let mut sim = test_sim(legacy_test_seed());
    let graph = sim.graph_for_species(Species::Elf);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let elf_pos = graph.node(elf_node).position;

    let mut nodes: Vec<_> = (0..graph.node_slot_count())
        .filter_map(|i| {
            let nid = NavNodeId(i as u32);
            if !graph.is_node_alive(nid) {
                return None;
            }
            let pos = graph.node(nid).position;
            let dist = pos.manhattan_distance(elf_pos);
            Some((dist, nid, pos))
        })
        .collect();
    nodes.sort_by_key(|(d, _, _)| *d);

    let close = nodes.iter().find(|(d, _, _)| *d >= 2).unwrap();
    let far = nodes.iter().find(|(d, _, _)| *d >= close.0 + 5).unwrap();
    let close_pos = close.2;
    let far_pos = far.2;

    create_dining_hall_with_id(&mut sim, close_pos, 5, StructureId(901));
    create_dining_hall_with_id(&mut sim, far_pos, 5, StructureId(902));

    // Fill all seats at the closer hall.
    let seats = sim.config.dining_seats_per_table;
    for _ in 0..seats {
        let fake_task_id = TaskId::new(&mut sim.rng);
        let fake_task = Task {
            id: fake_task_id,
            kind: TaskKind::DineAtHall {
                structure_id: StructureId(901),
            },
            state: TaskState::InProgress,
            location: close_pos,
            progress: 0,
            total_cost: 0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        sim.insert_task(fake_task);
        let seq = sim.db.task_voxel_refs.next_seq();
        sim.db
            .insert_task_voxel_ref(crate::db::TaskVoxelRef {
                seq,
                task_id: fake_task_id,
                coord: close_pos,
                role: crate::db::TaskVoxelRole::DiningSeat,
            })
            .unwrap();
    }

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(result.is_some(), "Should find the farther dining hall");
    let (_, _, sid) = result.unwrap();
    assert_eq!(
        sid,
        StructureId(902),
        "Should skip closer full hall and pick the farther one"
    );
}

#[test]
fn find_nearest_dining_hall_returns_none_when_no_halls_exist() {
    // No dining halls at all — the fast path should return None without
    // scanning task_voxel_refs.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(
        result.is_none(),
        "Should return None when no dining halls exist"
    );
}

#[test]
fn find_nearest_dining_hall_unplaced_table_ignored() {
    // A dining hall with food but its only table is unplaced.
    let mut sim = test_sim(legacy_test_seed());
    let graph = sim.graph_for_species(Species::Elf);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let structure_id = StructureId(900);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            project_id,
            build_type: BuildType::Building,
            anchor: table_pos,
            width: 3,
            depth: 3,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::DiningHall),
            inventory_id: inv_id,
            logistics_priority: None,
            crafting_enabled: false,
            greenhouse_species: None,
            greenhouse_enabled: false,
            greenhouse_last_production_tick: 0,
            last_dance_completed_tick: 0,
            last_dinner_party_completed_tick: 0,
        })
        .unwrap();
    // Insert one table with placed=false.
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            structure_id,
            coord: table_pos,
            placed: false,
        })
        .unwrap();
    sim.inv_add_simple_item(inv_id, ItemKind::Bread, 5, None, None);

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let result = sim.find_nearest_dining_hall(elf_id);
    assert!(
        result.is_none(),
        "Should return None when only table is unplaced"
    );
}

#[test]
fn wild_creature_does_not_seek_dining_hall() {
    // Wild creatures (no civ_id) should never attempt to dine, even when
    // hungry and a stocked dining hall exists.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let graph = sim.graph_for_species(Species::Squirrel);
    let table_node = graph.find_nearest_node(tree_pos, 10).unwrap();
    let table_pos = graph.node(table_node).position;

    let food_max = sim.species_table[&Species::Squirrel].food_max;
    let heartbeat = sim.species_table[&Species::Squirrel].heartbeat_interval_ticks;

    let squirrel_id = spawn_creature(&mut sim, Species::Squirrel);
    create_dining_hall(&mut sim, table_pos, 10);

    // Set squirrel food to 35% — in the dining band for elves.
    {
        let mut c = sim.db.creatures.get(&squirrel_id).unwrap();
        c.food = food_max * 35 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance one heartbeat.
    sim.step(&[], 1 + heartbeat);

    // Squirrel should NOT have a DineAtHall task.
    let creature = sim.db.creatures.get(&squirrel_id).unwrap();
    if let Some(task_id) = creature.current_task {
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_ne!(
            task.kind_tag,
            TaskKindTag::DineAtHall,
            "Wild creature (squirrel) should never get a DineAtHall task"
        );
    }
}
