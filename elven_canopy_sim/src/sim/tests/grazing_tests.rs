//! Tests for the wild grazing system: grassy dirt detection, grass
//! regrowth, herbivore food cycle, graze tasks, and grassless mesh.
//! Corresponds to `sim/grazing.rs`.

use super::*;

// ---------------------------------------------------------------------------
// F-wild-grazing: herbivore grazing tests
// ---------------------------------------------------------------------------

#[test]
fn is_grassy_dirt_returns_true_for_exposed_dirt() {
    let sim = test_sim(42);
    // Test world has dirt at y=0, air at y=1 (floor_y=0, terrain_max_height=0).
    // Find a dirt voxel that's not under the tree trunk.
    let coord = VoxelCoord::new(10, 0, 10);
    assert_eq!(sim.world.get(coord), VoxelType::Dirt);
    assert!(sim.is_grassy_dirt(coord));
}

#[test]
fn is_grassy_dirt_returns_false_for_air() {
    let sim = test_sim(42);
    let coord = VoxelCoord::new(10, 1, 10);
    assert!(!sim.is_grassy_dirt(coord));
}

#[test]
fn is_grassy_dirt_returns_false_for_grassless_dirt() {
    let mut sim = test_sim(42);
    let coord = VoxelCoord::new(10, 0, 10);
    assert!(sim.is_grassy_dirt(coord));
    sim.grassless.insert(coord);
    assert!(!sim.is_grassy_dirt(coord));
}

#[test]
fn is_grassy_dirt_returns_false_for_covered_dirt() {
    let mut sim = test_sim(42);
    // Place a solid voxel above dirt to cover it.
    let dirt = VoxelCoord::new(5, 0, 5);
    let above = VoxelCoord::new(5, 1, 5);
    sim.world.set(above, VoxelType::Dirt);
    assert!(!sim.is_grassy_dirt(dirt));
}

#[test]
fn capybara_is_herbivore() {
    let sim = test_sim(42);
    let species_data = &sim.species_table[&Species::Capybara];
    assert!(species_data.is_herbivore);
    assert!(species_data.graze_food_restore_pct > 0);
}

#[test]
fn elf_is_not_herbivore() {
    let sim = test_sim(42);
    let species_data = &sim.species_table[&Species::Elf];
    assert!(!species_data.is_herbivore);
}

#[test]
fn find_nearest_grass_returns_some_for_herbivore() {
    let mut sim = test_sim(42);
    let capybara_id = spawn_creature(&mut sim, Species::Capybara);
    let result = sim.find_nearest_grass(capybara_id);
    assert!(result.is_some(), "Capybara should find nearby grass");
}

#[test]
fn find_nearest_grass_avoids_grassless() {
    let mut sim = test_sim(42);
    let capybara_id = spawn_creature(&mut sim, Species::Capybara);

    // Mark a huge radius of dirt as grassless.
    for x in 0..64 {
        for z in 0..64 {
            sim.grassless.insert(VoxelCoord::new(x, 0, z));
        }
    }
    let result = sim.find_nearest_grass(capybara_id);
    assert!(result.is_none(), "All grass depleted — should find none");
}

#[test]
fn resolve_graze_action_restores_food() {
    let mut sim = test_sim(42);
    let capybara_id = spawn_creature(&mut sim, Species::Capybara);

    // Drain the capybara's food and assign a graze task.
    let grass_pos = VoxelCoord::new(10, 0, 10);
    let task_id = crate::types::TaskId::new(&mut sim.rng);
    let new_task = Task {
        id: task_id,
        kind: TaskKind::Graze { grass_pos },
        state: TaskState::InProgress,
        location: grass_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(new_task);

    if let Some(mut c) = sim.db.creatures.get(&capybara_id) {
        c.food = 100;
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let food_before = sim.db.creatures.get(&capybara_id).unwrap().food;
    assert_eq!(food_before, 100);

    sim.resolve_graze_action(capybara_id, task_id, grass_pos);

    let food_after = sim.db.creatures.get(&capybara_id).unwrap().food;
    assert!(
        food_after > food_before,
        "Food should increase after grazing"
    );

    // Check the grass_pos is now grassless.
    assert!(sim.grassless.contains(&grass_pos));
}

#[test]
fn resolve_graze_action_marks_voxel_grassless() {
    let mut sim = test_sim(42);
    let capybara_id = spawn_creature(&mut sim, Species::Capybara);

    let grass_pos = VoxelCoord::new(8, 0, 8);
    assert!(!sim.grassless.contains(&grass_pos));

    let task_id = crate::types::TaskId::new(&mut sim.rng);
    let new_task = Task {
        id: task_id,
        kind: TaskKind::Graze { grass_pos },
        state: TaskState::InProgress,
        location: grass_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(new_task);

    sim.resolve_graze_action(capybara_id, task_id, grass_pos);

    assert!(sim.grassless.contains(&grass_pos));
    assert!(!sim.is_grassy_dirt(grass_pos));
}

#[test]
fn grass_regrowth_removes_from_grassless() {
    let mut sim = test_sim(42);
    // Insert a grassless voxel.
    let coord = VoxelCoord::new(10, 0, 10);
    sim.grassless.insert(coord);
    assert!(!sim.is_grassy_dirt(coord));

    // Set 100% regrowth chance for deterministic test.
    sim.config.grass_regrowth_chance_pct = 100;
    sim.process_grass_regrowth();

    assert!(!sim.grassless.contains(&coord), "100% chance should regrow");
    assert!(sim.is_grassy_dirt(coord));
}

#[test]
fn grass_regrowth_zero_chance_does_nothing() {
    let mut sim = test_sim(42);
    let coord = VoxelCoord::new(10, 0, 10);
    sim.grassless.insert(coord);

    sim.config.grass_regrowth_chance_pct = 0;
    sim.process_grass_regrowth();

    assert!(sim.grassless.contains(&coord));
}

#[test]
fn hungry_herbivore_creates_graze_task() {
    let mut sim = test_sim(42);
    let capybara_id = spawn_creature(&mut sim, Species::Capybara);

    // Set food just below the hunger threshold so the heartbeat triggers
    // hunger-seeking without risking starvation death.
    let species_data = sim.species_table[&Species::Capybara].clone();
    let threshold = species_data.food_max * species_data.food_hunger_threshold_pct / 100;
    if let Some(mut c) = sim.db.creatures.get(&capybara_id) {
        c.food = threshold / 2; // Well below threshold but not starving.
        c.current_task = None;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Advance enough ticks for the heartbeat to fire.
    let target_tick = sim.tick + species_data.heartbeat_interval_ticks + 100;
    sim.step(&[], target_tick);

    // Verify a Graze task exists in the task table (it may already be
    // InProgress or Complete depending on timing).
    let has_graze_task = sim
        .db
        .tasks
        .iter_all()
        .any(|t| t.kind_tag == TaskKindTag::Graze);
    assert!(
        has_graze_task,
        "A Graze task should have been created for the hungry herbivore"
    );
}

#[test]
fn hungry_elf_does_not_create_graze_task() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Make the elf hungry.
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.food = 1;
        c.current_task = None;
    });

    let species_data = sim.species_table[&Species::Elf].clone();
    let target_tick = sim.tick + species_data.heartbeat_interval_ticks + 100;
    sim.step(&[], target_tick);

    // Check that any task assigned is NOT a Graze task.
    let creature = sim.db.creatures.get(&elf_id);
    if let Some(c) = creature {
        if let Some(task_id) = c.current_task {
            let task = sim.db.tasks.get(&task_id).unwrap();
            assert_ne!(
                task.kind_tag,
                TaskKindTag::Graze,
                "Elf should not get a Graze task"
            );
        }
    }
}

#[test]
fn graze_task_serde_roundtrip() {
    let mut rng = crate::prng::GameRng::new(42);
    let task_id = crate::types::TaskId::new(&mut rng);
    let grass_pos = VoxelCoord::new(10, 0, 10);

    let task = Task {
        id: task_id,
        kind: TaskKind::Graze { grass_pos },
        state: TaskState::InProgress,
        location: grass_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    let json = serde_json::to_string(&task).unwrap();
    let restored: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, task_id);
    match &restored.kind {
        TaskKind::Graze { grass_pos: gp } => assert_eq!(*gp, grass_pos),
        other => panic!("Expected Graze task, got {:?}", other),
    }
    assert_eq!(restored.origin, TaskOrigin::Autonomous);
}

#[test]
fn grassless_set_serde_roundtrip() {
    let mut sim = test_sim(42);
    // Add some grassless entries.
    sim.grassless.insert(VoxelCoord::new(10, 0, 10));
    sim.grassless.insert(VoxelCoord::new(20, 0, 20));

    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    assert!(restored.grassless.contains(&VoxelCoord::new(10, 0, 10)));
    assert!(restored.grassless.contains(&VoxelCoord::new(20, 0, 20)));
    assert_eq!(restored.grassless.len(), 2);
}

#[test]
fn grassless_mesh_color_differs_from_grassy() {
    use crate::mesh_gen::{GRASSLESS_DIRT_COLOR, voxel_color};
    let grassy_color = voxel_color(VoxelType::Dirt);
    assert_ne!(
        grassy_color, GRASSLESS_DIRT_COLOR,
        "Grassless dirt should have a different color than grassy dirt"
    );
}

#[test]
fn graze_preemption_level_is_survival() {
    let level = preemption::preemption_level(TaskKindTag::Graze, TaskOrigin::Autonomous);
    let eat_level = preemption::preemption_level(TaskKindTag::EatFruit, TaskOrigin::Autonomous);
    assert_eq!(
        level, eat_level,
        "Graze should have the same preemption level as EatFruit (Survival)"
    );
}

#[test]
fn resolve_graze_food_capped_at_food_max() {
    let mut sim = test_sim(42);
    let capybara_id = spawn_creature(&mut sim, Species::Capybara);
    let species_data = sim.species_table[&Species::Capybara].clone();

    // Set food to near-max.
    if let Some(mut c) = sim.db.creatures.get(&capybara_id) {
        c.food = species_data.food_max - 1;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let grass_pos = VoxelCoord::new(10, 0, 10);
    let task_id = crate::types::TaskId::new(&mut sim.rng);
    let new_task = Task {
        id: task_id,
        kind: TaskKind::Graze { grass_pos },
        state: TaskState::InProgress,
        location: grass_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(new_task);

    sim.resolve_graze_action(capybara_id, task_id, grass_pos);

    let food_after = sim.db.creatures.get(&capybara_id).unwrap().food;
    assert_eq!(
        food_after, species_data.food_max,
        "Food should be capped at food_max"
    );
}

#[test]
fn herbivore_does_not_fall_back_to_fruit() {
    let mut sim = test_sim(42);
    let capybara_id = spawn_creature(&mut sim, Species::Capybara);

    // Deplete all grass in the world.
    for x in 0..64 {
        for z in 0..64 {
            sim.grassless.insert(VoxelCoord::new(x, 0, z));
        }
    }

    // Make the capybara hungry and idle.
    if let Some(mut c) = sim.db.creatures.get(&capybara_id) {
        c.food = 1;
        c.current_task = None;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Advance past the heartbeat.
    let species_data = sim.species_table[&Species::Capybara].clone();
    let target_tick = sim.tick + species_data.heartbeat_interval_ticks + 100;
    sim.step(&[], target_tick);

    // The capybara should NOT have an EatFruit task.
    if let Some(creature) = sim.db.creatures.get(&capybara_id) {
        if let Some(task_id) = creature.current_task {
            let task = sim.db.tasks.get(&task_id).unwrap();
            assert_ne!(
                task.kind_tag,
                TaskKindTag::EatFruit,
                "Herbivores should not fall back to fruit"
            );
        }
    }
}

#[test]
fn grass_regrowth_event_fires_via_sim_step() {
    let mut sim = test_sim(42);

    // Add a grassless entry.
    let coord = VoxelCoord::new(10, 0, 10);
    sim.grassless.insert(coord);
    sim.config.grass_regrowth_chance_pct = 100;

    // Step past the regrowth interval.
    let target_tick = sim.tick + sim.config.grass_regrowth_interval_ticks + 1;
    sim.step(&[], target_tick);

    assert!(
        !sim.grassless.contains(&coord),
        "GrassRegrowth event should have removed the coord"
    );

    // Verify self-reschedule: add another entry and step again.
    let coord2 = VoxelCoord::new(20, 0, 20);
    sim.grassless.insert(coord2);
    let target_tick2 = target_tick + sim.config.grass_regrowth_interval_ticks + 1;
    sim.step(&[], target_tick2);

    assert!(
        !sim.grassless.contains(&coord2),
        "Second GrassRegrowth sweep should fire after self-reschedule"
    );
}

#[test]
fn two_herbivores_graze_same_tile() {
    let mut sim = test_sim(42);
    let cap1 = spawn_creature(&mut sim, Species::Capybara);
    let cap2 = spawn_creature(&mut sim, Species::Capybara);

    let grass_pos = VoxelCoord::new(10, 0, 10);

    // Create graze tasks for both.
    let tid1 = crate::types::TaskId::new(&mut sim.rng);
    let tid2 = crate::types::TaskId::new(&mut sim.rng);
    for (tid, cid) in [(tid1, cap1), (tid2, cap2)] {
        let new_task = Task {
            id: tid,
            kind: TaskKind::Graze { grass_pos },
            state: TaskState::InProgress,
            location: grass_pos,
            progress: 0,
            total_cost: 0,
            required_species: None,
            origin: TaskOrigin::Autonomous,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        sim.insert_task(new_task);
        if let Some(mut c) = sim.db.creatures.get(&cid) {
            c.food = 100;
            c.current_task = Some(tid);
            let _ = sim.db.creatures.update_no_fk(c);
        }
    }

    // Both resolve — both get food, grassless set has the coord.
    sim.resolve_graze_action(cap1, tid1, grass_pos);
    sim.resolve_graze_action(cap2, tid2, grass_pos);

    assert!(sim.grassless.contains(&grass_pos));
    // Both creatures should have increased food.
    let food1 = sim.db.creatures.get(&cap1).unwrap().food;
    let food2 = sim.db.creatures.get(&cap2).unwrap().food;
    assert!(food1 > 100, "First grazer should gain food");
    assert!(food2 > 100, "Second grazer should gain food (double-graze)");
}

#[test]
fn all_herbivore_species_flagged_correctly() {
    let sim = test_sim(42);
    let herbivores = [
        Species::Capybara,
        Species::Boar,
        Species::Deer,
        Species::Elephant,
        Species::Monkey,
        Species::Squirrel,
    ];
    for species in &herbivores {
        assert!(
            sim.species_table[species].is_herbivore,
            "{:?} should be herbivore",
            species
        );
    }
    let non_herbivores = [Species::Elf, Species::Goblin, Species::Orc, Species::Troll];
    for species in &non_herbivores {
        assert!(
            !sim.species_table[species].is_herbivore,
            "{:?} should NOT be herbivore",
            species
        );
    }
}

#[test]
fn backfill_grass_regrowth_event_on_load() {
    let mut sim = test_sim(42);
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    // The event queue should contain a GrassRegrowth event.
    let has_regrowth = restored
        .event_queue
        .iter()
        .any(|e| matches!(e.kind, crate::event::ScheduledEventKind::GrassRegrowth));
    assert!(has_regrowth, "Loaded sim should have GrassRegrowth event");
}

#[test]
fn set_voxel_exposes_dirt_marks_grassless() {
    let mut sim = test_sim(42);
    let dirt = VoxelCoord::new(10, 0, 10);
    let above = VoxelCoord::new(10, 1, 10);

    // Place a solid voxel above the dirt.
    sim.set_voxel(above, VoxelType::GrownPlatform);
    // Dirt is now covered — not grassy.
    assert!(!sim.is_grassy_dirt(dirt));
    // Covered dirt should not be in grassless (it's invisible anyway).
    assert!(!sim.grassless.contains(&dirt));

    // Remove the platform — exposes the dirt, which starts grassless.
    sim.set_voxel(above, VoxelType::Air);
    assert!(
        sim.grassless.contains(&dirt),
        "Freshly exposed dirt should be grassless"
    );
    assert!(
        !sim.is_grassy_dirt(dirt),
        "Freshly exposed dirt is not yet grassy"
    );
}

#[test]
fn set_voxel_covering_dirt_removes_from_grassless() {
    let mut sim = test_sim(42);
    let dirt = VoxelCoord::new(10, 0, 10);
    let above = VoxelCoord::new(10, 1, 10);

    // Mark the dirt as grassless (e.g., from grazing).
    sim.grassless.insert(dirt);
    assert!(sim.grassless.contains(&dirt));

    // Place a solid voxel above — dirt is now covered, removed from grassless.
    sim.set_voxel(above, VoxelType::GrownPlatform);
    assert!(
        !sim.grassless.contains(&dirt),
        "Covered dirt should be removed from grassless"
    );
}

#[test]
fn set_voxel_non_dirt_below_no_effect() {
    let mut sim = test_sim(42);
    // Place trunk at y=0, then remove solid above at y=1.
    let coord = VoxelCoord::new(10, 1, 10);
    sim.world.set(VoxelCoord::new(10, 0, 10), VoxelType::Trunk);
    sim.set_voxel(coord, VoxelType::GrownPlatform);
    sim.set_voxel(coord, VoxelType::Air);
    // Trunk below should NOT be added to grassless.
    assert!(!sim.grassless.contains(&VoxelCoord::new(10, 0, 10)));
}
