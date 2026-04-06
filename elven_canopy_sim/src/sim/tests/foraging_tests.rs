// Tests for F-wild-foraging: arboreal creatures (monkey, squirrel) seek wild
// fruit instead of grazing grass.
//
// See also: `grazing_tests.rs` (grass-grazing behavior), `needs_tests.rs`
// (elf EatFruit behavior), `needs.rs` (resolve_eat_fruit_action).

use super::*;

// -----------------------------------------------------------------------
// Species config sanity checks
// -----------------------------------------------------------------------

#[test]
fn monkey_is_forager_not_grazer() {
    let sim = test_sim(legacy_test_seed());
    let data = &sim.species_table[&Species::Monkey];
    assert!(data.is_forager, "Monkey should be a forager");
    assert!(!data.is_grazer, "Monkey should not be a grazer");
    assert!(
        data.forage_food_restore_pct > 0,
        "Monkey forage_food_restore_pct should be positive"
    );
}

#[test]
fn squirrel_is_forager_not_grazer() {
    let sim = test_sim(legacy_test_seed());
    let data = &sim.species_table[&Species::Squirrel];
    assert!(data.is_forager, "Squirrel should be a forager");
    assert!(!data.is_grazer, "Squirrel should not be a grazer");
    assert!(
        data.forage_food_restore_pct > 0,
        "Squirrel forage_food_restore_pct should be positive"
    );
}

#[test]
fn capybara_is_grazer_not_forager() {
    let sim = test_sim(legacy_test_seed());
    let data = &sim.species_table[&Species::Capybara];
    assert!(data.is_grazer, "Capybara should be a grazer");
    assert!(!data.is_forager, "Capybara should not be a forager");
}

// -----------------------------------------------------------------------
// Forager fruit-seeking behavior
// -----------------------------------------------------------------------

#[test]
fn hungry_monkey_creates_eat_fruit_task_not_graze() {
    let mut sim = test_sim(fresh_test_seed());
    ensure_tree_has_fruit(&mut sim);
    let monkey_id = spawn_creature(&mut sim, Species::Monkey);
    let food_max = sim.species_table[&Species::Monkey].food_max;
    let hunger_threshold_pct = sim.species_table[&Species::Monkey].food_hunger_threshold_pct;

    // Drain food below hunger threshold.
    {
        let mut c = sim.db.creatures.get(&monkey_id).unwrap();
        c.food = food_max * (hunger_threshold_pct - 1) / 100;
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    // Step through a heartbeat to trigger need evaluation.
    let heartbeat = sim.species_table[&Species::Monkey].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let task = sim.db.tasks.iter_all().find(|t| {
        t.restrict_to_creature_id == Some(monkey_id) || {
            sim.db
                .creatures
                .get(&monkey_id)
                .and_then(|c| c.current_task)
                .is_some_and(|tid| tid == t.id)
        }
    });

    assert!(task.is_some(), "Hungry monkey should have a task assigned");
    let task = task.unwrap();
    assert_eq!(
        task.kind_tag,
        TaskKindTag::EatFruit,
        "Hungry monkey should have EatFruit task, got {:?}",
        task.kind_tag
    );
}

#[test]
fn hungry_monkey_does_not_graze() {
    let mut sim = test_sim(fresh_test_seed());
    ensure_tree_has_fruit(&mut sim);
    let monkey_id = spawn_creature(&mut sim, Species::Monkey);
    let food_max = sim.species_table[&Species::Monkey].food_max;
    let hunger_threshold_pct = sim.species_table[&Species::Monkey].food_hunger_threshold_pct;

    {
        let mut c = sim.db.creatures.get(&monkey_id).unwrap();
        c.food = food_max * (hunger_threshold_pct - 1) / 100;
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let heartbeat = sim.species_table[&Species::Monkey].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let monkey_task = sim
        .db
        .creatures
        .get(&monkey_id)
        .and_then(|c| c.current_task)
        .and_then(|tid| sim.db.tasks.get(&tid));

    // Must not graze.
    assert!(
        !monkey_task
            .as_ref()
            .is_some_and(|t| t.kind_tag == TaskKindTag::Graze),
        "Monkey should NOT create a Graze task — it's a forager"
    );
    // With fruit available, must seek fruit instead.
    assert!(
        monkey_task
            .as_ref()
            .is_some_and(|t| t.kind_tag == TaskKindTag::EatFruit),
        "Hungry monkey with fruit available should have an EatFruit task"
    );
}

// -----------------------------------------------------------------------
// Forager food restoration
// -----------------------------------------------------------------------

#[test]
fn eat_fruit_restores_food_using_forage_pct_for_monkey() {
    let mut sim = test_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Monkey].food_max;
    let forage_pct = sim.species_table[&Species::Monkey].forage_food_restore_pct;

    let monkey_id = spawn_creature(&mut sim, Species::Monkey);

    // Place fruit at the monkey's position.
    let monkey_pos = sim.db.creatures.get(&monkey_id).unwrap().position.min;
    let fruit_pos = monkey_pos;
    let tree_id = sim.player_tree_id;
    let species_id = insert_test_fruit_species(&mut sim);
    sim.set_voxel(fruit_pos, VoxelType::Fruit);
    let hz = sim.home_zone_id();
    let _ = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        zone_id: hz,
        tree_id,
        position: VoxelBox::point(fruit_pos),
        species_id,
    });

    // Set food low.
    let initial_food = food_max / 10;
    {
        let mut c = sim.db.creatures.get(&monkey_id).unwrap();
        c.food = initial_food;
        sim.db.update_creature(c).unwrap();
    }

    // Create and resolve an EatFruit task directly.
    let task_id = crate::types::TaskId::new(&mut sim.rng);
    let new_task = Task {
        id: task_id,
        kind: TaskKind::EatFruit { fruit_pos },
        state: TaskState::InProgress,
        location: fruit_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), new_task);
    {
        let mut c = sim.db.creatures.get(&monkey_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_eat_fruit_action(monkey_id, task_id, fruit_pos);

    let food_after = sim.db.creatures.get(&monkey_id).unwrap().food;
    let expected = (initial_food + food_max * forage_pct / 100).min(food_max);
    assert_eq!(
        food_after, expected,
        "Monkey should restore food using forage_food_restore_pct ({forage_pct}%)"
    );
}

// -----------------------------------------------------------------------
// Grazer does not use forager path
// -----------------------------------------------------------------------

#[test]
fn hungry_capybara_does_not_create_eat_fruit_from_forager_path() {
    let mut sim = test_sim(fresh_test_seed());
    ensure_tree_has_fruit(&mut sim);

    // Deplete all grass so the capybara can't graze.
    for x in 0..sim.config.world_size.0 as i32 {
        for z in 0..sim.config.world_size.2 as i32 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .grassless
                .insert(VoxelCoord::new(x, 0, z));
        }
    }

    let capybara_id = spawn_creature(&mut sim, Species::Capybara);
    let food_max = sim.species_table[&Species::Capybara].food_max;
    let hunger_threshold_pct = sim.species_table[&Species::Capybara].food_hunger_threshold_pct;

    {
        let mut c = sim.db.creatures.get(&capybara_id).unwrap();
        c.food = food_max * (hunger_threshold_pct - 1) / 100;
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let heartbeat = sim.species_table[&Species::Capybara].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    // Capybara is ground-only and can't reach canopy fruit anyway, but the
    // forager code path should also not trigger for is_grazer species.
    let task = sim
        .db
        .creatures
        .get(&capybara_id)
        .and_then(|c| c.current_task)
        .and_then(|tid| sim.db.tasks.get(&tid));

    if let Some(task) = task {
        assert_ne!(
            task.kind_tag,
            TaskKindTag::EatFruit,
            "Capybara (grazer) should not create EatFruit via forager path"
        );
    }
    // No assertion if task is None — capybara simply wandered, which is fine
    // when both grass and foraging are unavailable.
}

// -----------------------------------------------------------------------
// Squirrel-specific tests (mirrors monkey tests with 30% restore)
// -----------------------------------------------------------------------

#[test]
fn hungry_squirrel_creates_eat_fruit_task_not_graze() {
    let mut sim = test_sim(fresh_test_seed());
    ensure_tree_has_fruit(&mut sim);
    let squirrel_id = spawn_creature(&mut sim, Species::Squirrel);
    let food_max = sim.species_table[&Species::Squirrel].food_max;
    let hunger_threshold_pct = sim.species_table[&Species::Squirrel].food_hunger_threshold_pct;

    {
        let mut c = sim.db.creatures.get(&squirrel_id).unwrap();
        c.food = food_max * (hunger_threshold_pct - 1) / 100;
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let heartbeat = sim.species_table[&Species::Squirrel].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let squirrel_task = sim
        .db
        .creatures
        .get(&squirrel_id)
        .and_then(|c| c.current_task)
        .and_then(|tid| sim.db.tasks.get(&tid));

    assert!(
        !squirrel_task
            .as_ref()
            .is_some_and(|t| t.kind_tag == TaskKindTag::Graze),
        "Squirrel should NOT create a Graze task — it's a forager"
    );
    assert!(
        squirrel_task
            .as_ref()
            .is_some_and(|t| t.kind_tag == TaskKindTag::EatFruit),
        "Hungry squirrel with fruit available should have an EatFruit task"
    );
}

#[test]
fn eat_fruit_restores_food_using_forage_pct_for_squirrel() {
    let mut sim = test_sim(fresh_test_seed());
    let food_max = sim.species_table[&Species::Squirrel].food_max;
    let forage_pct = sim.species_table[&Species::Squirrel].forage_food_restore_pct;

    let squirrel_id = spawn_creature(&mut sim, Species::Squirrel);
    let squirrel_pos = sim.db.creatures.get(&squirrel_id).unwrap().position.min;
    let fruit_pos = squirrel_pos;
    let tree_id = sim.player_tree_id;
    let species_id = insert_test_fruit_species(&mut sim);
    sim.set_voxel(fruit_pos, VoxelType::Fruit);
    let hz = sim.home_zone_id();
    let _ = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        zone_id: hz,
        tree_id,
        position: VoxelBox::point(fruit_pos),
        species_id,
    });

    let initial_food = food_max / 10;
    {
        let mut c = sim.db.creatures.get(&squirrel_id).unwrap();
        c.food = initial_food;
        sim.db.update_creature(c).unwrap();
    }

    let task_id = crate::types::TaskId::new(&mut sim.rng);
    let new_task = Task {
        id: task_id,
        kind: TaskKind::EatFruit { fruit_pos },
        state: TaskState::InProgress,
        location: fruit_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), new_task);
    {
        let mut c = sim.db.creatures.get(&squirrel_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.resolve_eat_fruit_action(squirrel_id, task_id, fruit_pos);

    let food_after = sim.db.creatures.get(&squirrel_id).unwrap().food;
    let expected = (initial_food + food_max * forage_pct / 100).min(food_max);
    assert_eq!(
        food_after, expected,
        "Squirrel should restore food using forage_food_restore_pct ({forage_pct}%)"
    );
}

// -----------------------------------------------------------------------
// Invariant: no species is both forager and grazer
// -----------------------------------------------------------------------

#[test]
fn no_species_is_both_forager_and_grazer() {
    let sim = test_sim(legacy_test_seed());
    for (species, data) in &sim.species_table {
        assert!(
            !(data.is_forager && data.is_grazer),
            "{species:?} cannot be both is_forager and is_grazer"
        );
    }
}

// -----------------------------------------------------------------------
// Forager with no fruit available does not fall back to grazing
// -----------------------------------------------------------------------

#[test]
fn forager_with_no_fruit_does_not_graze() {
    // Use flat_world_sim — it generates no fruit, so find_nearest_fruit
    // returns None and the forager has nothing to eat.
    let mut sim = flat_world_sim(fresh_test_seed());
    assert_eq!(
        sim.db.tree_fruits.len(),
        0,
        "Flat world should have no fruit"
    );

    let monkey_id = spawn_creature(&mut sim, Species::Monkey);
    let food_max = sim.species_table[&Species::Monkey].food_max;
    let hunger_threshold_pct = sim.species_table[&Species::Monkey].food_hunger_threshold_pct;

    {
        let mut c = sim.db.creatures.get(&monkey_id).unwrap();
        c.food = food_max * (hunger_threshold_pct - 1) / 100;
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let heartbeat = sim.species_table[&Species::Monkey].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let monkey_task = sim
        .db
        .creatures
        .get(&monkey_id)
        .and_then(|c| c.current_task)
        .and_then(|tid| sim.db.tasks.get(&tid));

    assert!(
        !monkey_task
            .as_ref()
            .is_some_and(|t| t.kind_tag == TaskKindTag::Graze),
        "Forager with no fruit should NOT fall back to grazing"
    );
    assert!(
        !monkey_task
            .as_ref()
            .is_some_and(|t| t.kind_tag == TaskKindTag::EatFruit),
        "Forager with no fruit should NOT create an EatFruit task"
    );
}
