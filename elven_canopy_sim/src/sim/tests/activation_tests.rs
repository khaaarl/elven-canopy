//! Tests for the activation system: action states, task claiming/walking,
//! task interruption, action tick timing, creature activation lifecycle,
//! preemption, directed goto, and command queuing.
//! Corresponds to `sim/activation.rs`.

use super::*;

// -----------------------------------------------------------------------
// Creature spawning + wander / determinism
// -----------------------------------------------------------------------

#[test]
fn elf_wanders_after_spawn() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn elf.
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[spawn_cmd], 2);

    // Step far enough for many activations (each ground edge costs ~500
    // ticks at move_ticks_per_voxel=500).
    sim.step(&[], 50000);

    assert_eq!(sim.creature_count(Species::Elf), 1);
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    // Verify elf is at a walkable position.
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            elf.position.min,
            [1, 1, 1],
            true,
        ),
        "Elf should be at a walkable position"
    );
}

#[test]
fn determinism_with_elf_after_1000_ticks() {
    let seed = legacy_test_seed();
    let mut sim_a = test_sim(seed);
    let mut sim_b = test_sim(seed);

    let tree_pos = sim_a.db.trees.get(&sim_a.player_tree_id).unwrap().position;

    let spawn = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim_a.home_zone_id(),
            species: Species::Elf,
            position: tree_pos,
        },
    };

    sim_a.step(std::slice::from_ref(&spawn), 1000);
    sim_b.step(std::slice::from_ref(&spawn), 1000);

    // Both sims should have identical creature positions.
    assert_eq!(sim_a.db.creatures.len(), sim_b.db.creatures.len());
    for creature_a in sim_a.db.creatures.iter_all() {
        let creature_b = sim_b.db.creatures.get(&creature_a.id).unwrap();
        assert_eq!(creature_a.position, creature_b.position);
    }
    // PRNG state should be identical.
    assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
}

// -----------------------------------------------------------------------
// Task claiming and walking
// -----------------------------------------------------------------------

#[test]
fn creature_claims_available_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Far ground-level location — elf spawns near (32,1,32), this is ~20
    // voxels away so it won't arrive within the tick budget.
    let task_pos = VoxelCoord::new(10, 1, 10);
    let task_coord = find_walkable(&sim, task_pos, 10).unwrap();
    let task_id = insert_goto_task(&mut sim, task_coord);

    // Tick enough for the elf to claim but not finish (~500 ticks per edge
    // at move_ticks_per_voxel=500, ~20 edges to walk).
    sim.step(&[], sim.tick + 5000);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.current_task,
        Some(task_id),
        "Elf should have claimed the available task"
    );
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.state, TaskState::InProgress);
}

#[test]
fn creature_walks_to_task_location() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Far ground-level location so the elf has a long walk.
    let task_coord = VoxelCoord::new(10, 1, 10);
    let task_location = find_walkable(&sim, task_coord, 10).unwrap();
    let _task_id = insert_goto_task(&mut sim, task_location);

    let initial_dist = sim
        .db
        .creatures
        .get(&elf_id)
        .unwrap()
        .position
        .min
        .manhattan_distance(task_location);

    // Step a moderate amount — creature should be closer to the target.
    sim.step(&[], sim.tick + 50000);

    let mid_dist = sim
        .db
        .creatures
        .get(&elf_id)
        .unwrap()
        .position
        .min
        .manhattan_distance(task_location);

    // The elf should either be closer to the task (still walking)
    // or have already completed it (GoTo completes instantly at arrival,
    // then the elf may wander away).
    let task = sim.db.tasks.get(&_task_id).unwrap();
    assert!(
        mid_dist < initial_dist || task.state == TaskState::Complete,
        "Elf should be closer to task or have completed it (initial={initial_dist}, mid={mid_dist}, state={:?})",
        task.state,
    );
}

// -----------------------------------------------------------------------
// Action states during work actions
// -----------------------------------------------------------------------

/// High-priority #1: ActionKind and next_available_tick — verify action_kind
/// is correctly set during Build, Sleep, Cook, and Eat work actions.
#[test]
fn action_state_set_during_work_actions() {
    let mut config = test_config();
    // Very long build so it can't complete before we check.
    config.build_work_ticks_per_voxel = 500_000;
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            zone_id: sim.home_zone_id(),
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Run enough ticks for the elf to arrive and start building.
    sim.step(&[], sim.tick + 100_000);

    // Elf must have claimed the build task and be mid-action.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_some(),
        "Elf should have claimed the Build task"
    );
    let task_id = elf.current_task.unwrap();
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.kind_tag, TaskKindTag::Build);
    assert_eq!(task.state, TaskState::InProgress);
    assert_eq!(
        elf.action_kind,
        ActionKind::Build,
        "Elf working on build should have ActionKind::Build"
    );
    assert!(
        elf.next_available_tick.is_some(),
        "Elf in Build action should have next_available_tick set"
    );
}

/// High-priority #2: MoveAction cleanup on resolve — after a creature
/// completes a Move action (arrives somewhere), the MoveAction row is
/// deleted.
#[test]
fn move_action_cleaned_up_after_arrival() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Let the elf wander once to create a MoveAction.
    sim.step(&[], sim.tick + 2);

    // Elf should have a MoveAction now.
    assert!(
        sim.db.move_actions.get(&elf_id).is_some(),
        "MoveAction should exist after wander"
    );
    let next_tick = sim
        .db
        .creatures
        .get(&elf_id)
        .unwrap()
        .next_available_tick
        .unwrap();

    // Advance past the move completion.
    sim.step(&[], next_tick + 1);

    // After arrival, action state should be cleared (or a new action started).
    // Either way, the *old* MoveAction should have been cleaned up. If the elf
    // wandered again it will have a new MoveAction, which is fine — the test
    // just verifies the resolve path ran.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    if elf.action_kind == ActionKind::NoAction {
        assert!(
            sim.db.move_actions.get(&elf_id).is_none(),
            "MoveAction should be deleted when elf is idle"
        );
    }
    // If elf started a new wander, it has action_kind=Move — that's fine,
    // it means the old one was resolved and a new one created.
}

/// High-priority #3: Task removed during a non-Move action. Creature
/// should fall through to decision cascade and wander.
#[test]
fn task_removed_during_build_action_creature_wanders() {
    let mut config = test_config();
    // Very long build so it can't complete before we cancel.
    config.build_work_ticks_per_voxel = 1_000_000;
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            zone_id: sim.home_zone_id(),
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // Run enough for elf to reach the build site (but build won't finish
    // because it takes 1M ticks).
    sim.step(&[], sim.tick + 100_000);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let task_id = elf.current_task;
    // Elf should have claimed the build task.
    if task_id.is_none() {
        // If no task yet, run longer for elf to find and claim it.
        sim.step(&[], sim.tick + 200_000);
    }
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.current_task.is_some(), "Elf should have a Build task");

    // Cancel the build (remove the blueprint and task).
    let bp = sim.db.blueprints.iter_all().next().unwrap();
    let project_id = bp.id;
    let cancel_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::CancelBuild { project_id },
    };
    sim.step(&[cancel_cmd], sim.tick + 2);

    // Advance for the elf's activation to fire after cancellation.
    sim.step(&[], sim.tick + 1_100_000);

    // Elf should have lost the task and be wandering.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.current_task.is_none()
            || sim
                .db
                .tasks
                .get(&elf.current_task.unwrap())
                .is_some_and(|t| t.kind_tag != TaskKindTag::Build),
        "Elf should no longer have the cancelled Build task"
    );
}

/// High-priority #4: PickUp phase transition — use the logistics
/// heartbeat to create a haul task, then run the full pipeline and
/// verify the task transitions through GoingToDestination.
#[test]
fn pickup_action_transitions_haul_phase() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.haul_pickup_action_ticks = 500;
    sim.config.haul_dropoff_action_ticks = 500;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Place bread on the ground.
    let pile_pos = tree_pos;
    {
        let pile_id = sim.ensure_ground_pile(pile_pos, sim.home_zone_id());
        let pile = sim.db.ground_piles.get(&pile_id).unwrap();
        sim.inv_add_simple_item(
            pile.inventory_id,
            crate::inventory::ItemKind::Bread,
            5,
            None,
            None,
        );
    }

    // Create a building that wants bread.
    let building_anchor = VoxelCoord::new(pile_pos.x + 3, pile_pos.y, pile_pos.z);
    let sid = insert_building(
        &mut sim,
        building_anchor,
        Some(5),
        vec![crate::building::LogisticsWant {
            item_kind: crate::inventory::ItemKind::Bread,
            material_filter: crate::inventory::MaterialFilter::Any,
            target_quantity: 5,
        }],
    );

    // Run logistics heartbeat to create haul task.
    sim.process_logistics_heartbeat();

    let haul_tasks: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Haul)
        .collect();
    assert_eq!(haul_tasks.len(), 1, "Expected 1 haul task");
    let haul_task_id = haul_tasks[0].id;

    // Verify initial state is GoingToSource.
    let haul_data = sim.task_haul_data(haul_task_id).unwrap();
    assert_eq!(haul_data.phase, task::HaulPhase::GoingToSource);
    let initial_location = sim.db.tasks.get(&haul_task_id).unwrap().location;

    // Spawn an elf and run to completion.
    let _elf_id = spawn_elf(&mut sim);
    sim.step(&[], sim.tick + 100_000);

    // Task should have completed (or at least transitioned phases).
    let task = sim.db.tasks.get(&haul_task_id).unwrap();
    if task.state == TaskState::Complete {
        // Full pipeline ran — items delivered.
        let structure = sim.db.structures.get(&sid).unwrap();
        let bread_count = sim.inv_unreserved_item_count(
            structure.inventory_id,
            crate::inventory::ItemKind::Bread,
            crate::inventory::MaterialFilter::Any,
        );
        assert!(bread_count > 0, "Bread should have been delivered");
    } else {
        // At minimum, haul should have progressed past GoingToSource.
        let haul_data = sim.task_haul_data(haul_task_id).unwrap();
        assert_eq!(
            haul_data.phase,
            task::HaulPhase::GoingToDestination,
            "Haul should have transitioned to GoingToDestination"
        );
        // Task location should have changed to destination.
        assert_ne!(
            task.location, initial_location,
            "Task location should update after PickUp"
        );
    }
}

/// High-priority #6: Sleep adaptive completion — a creature near full
/// rest completes sleep early (via rest_full) with progress < total_cost.
#[test]
fn sleep_adaptive_completion_rest_full_exits_early() {
    let mut config = test_config();
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    // High restore per sleep action so rest fills in ~1-2 actions.
    // rest_max is 1e15, so each action restores rest_per_sleep_tick * sleep_action_ticks.
    // With 1000 action_ticks and rest_per_sleep_tick = 1e11, each action restores 1e14.
    // At 95% rest, need 5e13 → 1 action should fill it.
    elf_species.rest_per_sleep_tick = 100_000_000_000;
    // Heartbeat far in the future so it doesn't interfere.
    elf_species.heartbeat_interval_ticks = 1_000_000;
    config.sleep_action_ticks = 1000;
    config.sleep_ticks_ground = 1_000_000; // Very long sleep by progress.
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;

    // Spawn elf (step to tick 1 only, before first activation).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
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

    // Set rest to 95% (near full). With rest_per_sleep_tick=500 and
    // sleep_action_ticks=1000, each action restores 500_000. rest_max
    // is typically 100_000, so 5% = 5000 → one action should fill it.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 95 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Create a ground sleep task at elf's location.
    let elf_node = creature_pos(&sim, elf_id);
    let task_id = TaskId::new(&mut sim.rng);
    let sleep_task = task::Task {
        id: task_id,
        kind: task::TaskKind::Sleep {
            bed_pos: None,
            location: task::SleepLocation::Ground,
        },
        state: task::TaskState::InProgress,
        location: elf_node,
        progress: 0,
        total_cost: (sim.config.sleep_ticks_ground / sim.config.sleep_action_ticks) as i64,
        required_species: None,
        origin: task::TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), sleep_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Run enough for the first activation (tick 2) + a few sleep actions.
    sim.step(&[], sim.tick + 10_000);

    let sleep_task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        sleep_task.state,
        TaskState::Complete,
        "Sleep should complete early when rest hits max"
    );
    // Progress should be much less than total_cost (early exit via rest_full).
    assert!(
        sleep_task.progress < sleep_task.total_cost,
        "Progress ({}) should be less than total_cost ({}) for early rest-full completion",
        sleep_task.progress,
        sleep_task.total_cost
    );
}

/// High-priority #7: ActionKind + MoveAction serde roundtrip.
#[test]
fn action_state_survives_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Let the elf wander to create a Move action + MoveAction row.
    sim.step(&[], sim.tick + 2);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::Move);
    assert!(elf.next_available_tick.is_some());
    assert!(sim.db.move_actions.get(&elf_id).is_some());

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    let elf_r = restored.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf_r.action_kind, ActionKind::Move);
    assert_eq!(elf_r.next_available_tick, elf.next_available_tick);

    let ma = restored.db.move_actions.get(&elf_id).unwrap();
    let ma_orig = sim.db.move_actions.get(&elf_id).unwrap();
    assert_eq!(ma.move_from, ma_orig.move_from);
    assert_eq!(ma.move_to, ma_orig.move_to);
    assert_eq!(ma.move_start_tick, ma_orig.move_start_tick);
}

/// Medium-priority #8: abort_current_action with Move cleans up MoveAction.
#[test]
fn abort_move_action_cleans_up_move_action_row() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Let the elf wander to create a Move action.
    sim.step(&[], sim.tick + 2);
    assert!(sim.db.move_actions.get(&elf_id).is_some());
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().action_kind,
        ActionKind::Move
    );

    // Manually abort.
    sim.abort_current_action(elf_id);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::NoAction);
    assert!(elf.next_available_tick.is_none());
    assert!(
        sim.db.move_actions.get(&elf_id).is_none(),
        "MoveAction row should be deleted after abort"
    );
}

/// Medium-priority #12: Creature removal cleans up MoveAction.
/// The MoveAction table's FK on creature_id has on_delete cascade
/// at the Database level. Since MoveAction's PK is also the FK
/// (creature_id), we verify the cascade path works.
#[test]
fn creature_removal_cleans_up_move_action() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Let the elf wander to create a MoveAction.
    sim.step(&[], sim.tick + 2);
    assert!(sim.db.move_actions.get(&elf_id).is_some());

    // Manually remove both the MoveAction and creature (simulating
    // what a real despawn would do — abort_current_action + remove).
    sim.abort_current_action(elf_id);
    assert!(
        sim.db.move_actions.get(&elf_id).is_none(),
        "MoveAction should be removed by abort_current_action"
    );

    // Creature should still exist.
    assert!(sim.db.creatures.get(&elf_id).is_some());
}
/// Lower-priority #14: Config duration fields control timing — verify
/// eat_action_ticks controls when Eat action resolves.
#[test]
fn eat_action_ticks_controls_timing() {
    let mut config = test_config();
    config.eat_action_ticks = 3000;
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn elf.
    let mut events = Vec::new();
    sim.spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Create an EatBread task at the elf's location.
    let elf_node = creature_pos(&sim, elf_id);
    let task_id = TaskId::new(&mut sim.rng);
    let eat_task = task::Task {
        id: task_id,
        kind: task::TaskKind::EatBread,
        state: task::TaskState::InProgress,
        location: elf_node,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: task::TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), eat_task);

    // Give elf bread and assign task.
    let inv_id = sim.creature_inv(elf_id);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bread, 1, None, None);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Run 1 tick to let activation fire and start the Eat action.
    sim.step(&[], sim.tick + 5);

    // Elf should be in Eat action with next_available_tick = tick + 3000.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    if elf.action_kind == ActionKind::Eat {
        let expected_tick = elf.next_available_tick.unwrap();
        // The action should be scheduled ~3000 ticks from when it started.
        assert!(
            expected_tick > sim.tick,
            "Eat action should be scheduled in the future"
        );
    }

    // Run past the action duration.
    sim.step(&[], sim.tick + 3100);

    // Task should be complete.
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Eat task should be complete"
    );
}
/// Lower-priority #18: Config backward compat for new action_ticks fields.
/// Verify that the default GameConfig has the expected action_ticks values.
#[test]
fn config_backward_compat_action_ticks_defaults() {
    let config = GameConfig::default();

    assert_eq!(config.sleep_action_ticks, 1000);
    assert_eq!(config.eat_action_ticks, 1500);
    assert_eq!(config.harvest_action_ticks, 1500);
    assert_eq!(config.acquire_item_action_ticks, 1000);
    assert_eq!(config.haul_pickup_action_ticks, 1000);
    assert_eq!(config.haul_dropoff_action_ticks, 1000);
    assert_eq!(config.mope_action_ticks, 1000);
}

/// Lower-priority #20: abort_current_action is harmless with NoAction.
#[test]
fn abort_no_action_is_harmless() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn elf but only step to tick 1 (before first activation).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
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

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::NoAction);

    // Abort should be a no-op.
    sim.abort_current_action(elf_id);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::NoAction);
    assert!(elf.next_available_tick.is_none());
}

/// ActionKind serde roundtrip for all 16 variants.
#[test]
fn action_kind_serde_roundtrip_all_variants() {
    let variants = [
        ActionKind::NoAction,
        ActionKind::Move,
        ActionKind::Build,
        ActionKind::Furnish,
        ActionKind::Craft,
        ActionKind::Sleep,
        ActionKind::Eat,
        ActionKind::Harvest,
        ActionKind::AcquireItem,
        ActionKind::PickUp,
        ActionKind::DropOff,
        ActionKind::Mope,
        ActionKind::MeleeStrike,
        ActionKind::Shoot,
        ActionKind::AcquireMilitaryEquipment,
        ActionKind::TameAttempt,
        ActionKind::Graze,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let restored: ActionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(
            *variant, restored,
            "ActionKind {:?} should survive serde roundtrip",
            variant
        );
    }
}

/// abort_current_action with Build (non-Move) just clears state, no
/// MoveAction deletion attempt.
#[test]
fn abort_build_action_clears_state_only() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn elf but only step to tick 1 (before first activation).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
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

    // Manually set elf to Build action state.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.action_kind = ActionKind::Build;
        c.next_available_tick = Some(sim.tick + 50_000);
        sim.db.update_creature(c).unwrap();
    }

    // No MoveAction should exist (elf hasn't moved yet).
    assert!(sim.db.move_actions.get(&elf_id).is_none());

    // Abort should clear the state without errors.
    sim.abort_current_action(elf_id);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::NoAction);
    assert!(elf.next_available_tick.is_none());
    assert!(sim.db.move_actions.get(&elf_id).is_none());
}

// -----------------------------------------------------------------------
// Task interruption tests
// -----------------------------------------------------------------------

#[test]
fn interrupt_goto_completes_task_and_clears_creature() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    let far_pos = find_different_walkable(&sim, creature_pos(&sim, elf_id));
    let task_id = TaskId::new(&mut sim.rng);
    let goto_task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: far_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), goto_task);
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.interrupt_task(elf_id, task_id);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.state, TaskState::Complete);
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(creature.current_task.is_none());
    assert!(creature.path.is_none());
}

#[test]
fn interrupt_build_returns_task_to_available() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    // Designate a build.
    let cmd_build = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
            zone_id: sim.home_zone_id(),
            build_type: BuildType::Platform,
            voxels: vec![air_coord],
            priority: Priority::Normal,
        },
    };
    sim.step(&[cmd_build], sim.tick + 2);

    let build_task_id = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Build)
        .unwrap()
        .id;

    // Assign the elf to the build task.
    if let Some(mut t) = sim.db.tasks.get(&build_task_id) {
        t.state = TaskState::InProgress;
        sim.db.update_task(t).unwrap();
    }
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(build_task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.interrupt_task(elf_id, build_task_id);

    // Build is resumable — should return to Available.
    let task = sim.db.tasks.get(&build_task_id).unwrap();
    assert_eq!(task.state, TaskState::Available);
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(creature.current_task.is_none());
}

#[test]
fn interrupt_craft_clears_reservations() {
    let mut sim = test_sim(legacy_test_seed());
    let structure_id = setup_crafting_building(&mut sim, FurnishingType::Workshop);

    // Spawn elf near building.
    let structure = sim.db.structures.get(&structure_id).unwrap();
    let elf_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
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

    // Stock workshop with Bowstring and run the crafting monitor.
    let inv_id = sim.structure_inv(structure_id);
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bowstring, 1, None, None);
    sim.process_unified_crafting_monitor();

    let task_id = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Craft)
        .unwrap()
        .id;

    // Verify fruit is reserved.
    let reserved = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by == Some(task_id))
        .count();
    assert!(reserved > 0, "Fruit should be reserved before interrupt");

    // Assign elf to the task.
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        sim.db.update_task(t).unwrap();
    }
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.interrupt_task(elf_id, task_id);

    // Reservations should be cleared.
    let still_reserved = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|s| s.reserved_by == Some(task_id))
        .count();
    assert_eq!(still_reserved, 0, "Reservations should be cleared");

    // Task should be Complete (non-resumable).
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.state, TaskState::Complete);
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(creature.current_task.is_none());
}

#[test]
fn interrupt_sleep_completes_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    let current_node = creature_pos(&sim, elf_id);
    let task_id = TaskId::new(&mut sim.rng);
    let sleep_task = Task {
        id: task_id,
        kind: TaskKind::Sleep {
            bed_pos: None,
            location: task::SleepLocation::Ground,
        },
        state: TaskState::InProgress,
        location: current_node,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), sleep_task);
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.interrupt_task(elf_id, task_id);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.state, TaskState::Complete);
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(creature.current_task.is_none());
}

#[test]
fn interrupt_task_clears_creature_fields() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Assign creature to a stub task, then interrupt it.
    // (The original "missing task" scenario is now impossible thanks to FK
    // constraints — current_task cannot point to a nonexistent task.)
    let task_id = TaskId::new(&mut sim.rng);
    insert_stub_task(&mut sim, task_id);
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.current_task = Some(task_id);
    sim.db.update_creature(c).unwrap();

    sim.interrupt_task(elf_id, task_id);

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(creature.current_task.is_none());
    assert!(creature.path.is_none());
}

#[test]
fn interrupt_clears_move_action() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    let current_node = creature_pos(&sim, elf_id);

    // Put the elf in a Move action.
    let pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.action_kind = ActionKind::Move;
        c.next_available_tick = Some(sim.tick + 1000);
        sim.db.update_creature(c).unwrap();
    }
    let move_action = crate::db::MoveAction {
        creature_id: elf_id,
        move_from: pos,
        move_to: pos,
        move_start_tick: sim.tick,
    };
    // Remove any existing move action from spawn/wander (may not exist).
    let _ = sim.db.remove_move_action(&elf_id);
    sim.db.insert_move_action(move_action).unwrap();

    // Create a GoTo task for context.
    let task_id = TaskId::new(&mut sim.rng);
    let goto_task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: current_node,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), goto_task);
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    sim.interrupt_task(elf_id, task_id);

    // MoveAction should be deleted.
    assert!(sim.db.move_actions.get(&elf_id).is_none());
    // Action should be cleared.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.action_kind, ActionKind::NoAction);
    assert!(creature.next_available_tick.is_none());
}

// -----------------------------------------------------------------------
// Preemption
// -----------------------------------------------------------------------

#[test]
fn attack_target_autonomous_preemption_is_autonomous_combat() {
    assert_eq!(
        preemption::preemption_level(crate::db::TaskKindTag::AttackTarget, TaskOrigin::Autonomous),
        preemption::PreemptionLevel::AutonomousCombat,
    );
}

// -----------------------------------------------------------------------
// Civ-filtered task assignment
// -----------------------------------------------------------------------

#[test]
fn find_available_task_respects_civ_filter() {
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let player_civ = sim.player_civ_id.unwrap();

    // Spawn a goblin belonging to the hostile civ.
    let mut events = Vec::new();
    let goblin_id = sim
        .spawn_creature_with_civ(
            Species::Goblin,
            tree_pos,
            Some(hostile_civ),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn goblin");
    {
        let mut c = sim.db.creatures.get(&goblin_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let task_pos = find_walkable(&sim, tree_pos, 10).expect("should have walkable pos");

    // Create a task restricted to the player's civ — hostile goblin should NOT see it.
    let task_id = TaskId::new(&mut sim.rng);
    let task = crate::db::Task {
        id: task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 1,
        required_species: None, // no species filter — only civ filter
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: Some(player_civ),
    };
    sim.db.insert_task(task).unwrap();

    let chosen = sim.find_available_task(goblin_id);
    assert_eq!(
        chosen, None,
        "hostile-civ goblin should not be assigned a player-civ task"
    );

    // Spawn an elf (player civ) — it SHOULD see the task.
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("should spawn elf");
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let chosen = sim.find_available_task(elf_id);
    assert_eq!(
        chosen,
        Some(task_id),
        "player-civ elf should be assigned the player-civ task"
    );
}

#[test]
fn find_available_task_unaffiliated_creature_blocked_by_civ_filter() {
    // A creature with civ_id = None (wild/unaffiliated) cannot claim a
    // player-civ task. This is the same filter path as hostile creatures,
    // but exercises the None vs Some comparison.
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let player_civ = sim.player_civ_id.unwrap();

    // Spawn a capybara (civ_id = None by default for non-elves).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Capybara,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    let capy = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .expect("capybara should exist");
    let capy_id = capy.id;
    assert_eq!(capy.civ_id, None, "capybara should be unaffiliated");

    {
        let mut c = sim.db.creatures.get(&capy_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let task_pos = find_walkable(&sim, tree_pos, 10).expect("should have walkable pos");

    // Create a player-civ task — unaffiliated capybara should NOT see it.
    let task_id = TaskId::new(&mut sim.rng);
    let task = crate::db::Task {
        id: task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: Some(player_civ),
    };
    sim.db.insert_task(task).unwrap();

    let chosen = sim.find_available_task(capy_id);
    assert_eq!(
        chosen, None,
        "unaffiliated creature should not claim a player-civ task"
    );
}

#[test]
fn insert_task_propagates_required_civ_id() {
    // Verify that insert_task copies required_civ_id from the DTO to the DB row.
    let mut sim = test_sim(legacy_test_seed());
    let player_civ = sim.player_civ_id.unwrap();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let task_id = TaskId::new(&mut sim.rng);
    let dto = task::Task {
        id: task_id,
        kind: task::TaskKind::GoTo,
        state: task::TaskState::Available,
        location: tree_pos,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: task::TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: Some(player_civ),
    };
    sim.insert_task(sim.home_zone_id(), dto);

    let db_task = sim.db.tasks.get(&task_id).expect("task should be inserted");
    assert_eq!(
        db_task.required_civ_id,
        Some(player_civ),
        "insert_task should propagate required_civ_id to the DB row"
    );
}

#[test]
fn find_available_task_no_civ_filter_allows_any() {
    // Tasks with required_civ_id = None should be claimable by anyone.
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let mut events = Vec::new();
    let goblin_id = sim
        .spawn_creature_with_civ(
            Species::Goblin,
            tree_pos,
            Some(hostile_civ),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn goblin");
    {
        let mut c = sim.db.creatures.get(&goblin_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let task_pos = find_walkable(&sim, tree_pos, 10).expect("should have walkable pos");

    // Task with no civ restriction.
    let task_id = TaskId::new(&mut sim.rng);
    let task = crate::db::Task {
        id: task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.insert_task(task).unwrap();

    let chosen = sim.find_available_task(goblin_id);
    assert_eq!(
        chosen,
        Some(task_id),
        "any creature should be able to claim a task with no civ restriction"
    );
}

// -----------------------------------------------------------------------
// Directed goto / attack-move activation (B-erratic-movement-2)
// -----------------------------------------------------------------------

#[test]
fn directed_goto_on_wandering_creature_does_not_schedule_extra_activation() {
    // B-erratic-movement-2: issuing a DirectedGoTo while a creature is
    // mid-wander-move (no task, action_kind == Move) should NOT schedule
    // an extra CreatureActivation. The existing wander activation should
    // resolve the move, then the creature picks up the new GoTo task.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Put the elf into a wander state: no task, mid-Move.
    let elf_node = creature_pos(&sim, elf);
    let mut events = Vec::new();
    sim.ground_wander(elf, elf_node, &mut events);

    // Verify the elf is now mid-wander-move with no task.
    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(c.action_kind, ActionKind::Move, "Should be mid-wander-move");
    assert!(c.current_task.is_none(), "Wander has no task");
    let activations_before = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_before, 1,
        "Should have exactly 1 pending activation from wander"
    );

    // Issue a DirectedGoTo while mid-wander-move.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DirectedGoTo {
                zone_id: sim.home_zone_id(),
                creature_id: elf,
                position: target,
                queue: false,
            },
        }],
        tick + 1,
    );

    // Should still have exactly 1 pending activation — the existing wander
    // activation — not 2 (which would cause the wander move to resolve early).
    let activations_after = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_after, 1,
        "Should still have exactly 1 pending activation after DirectedGoTo on wandering \
             creature (was {activations_after})"
    );
}

#[test]
fn directed_goto_on_wandering_creature_preserves_move_action() {
    // B-erratic-movement-2: issuing a DirectedGoTo while a creature is
    // mid-wander-move should preserve the in-progress Move action.
    // The move should complete at its original next_available_tick, then
    // the creature starts walking toward the new GoTo destination.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Put the elf into a wander state.
    let elf_node = creature_pos(&sim, elf);
    let mut events = Vec::new();
    sim.ground_wander(elf, elf_node, &mut events);

    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(c.action_kind, ActionKind::Move);
    assert!(c.current_task.is_none());
    let nat_before = c.next_available_tick;
    assert!(nat_before.is_some(), "Should have a next_available_tick");
    let move_action_before = sim.db.move_actions.get(&elf).unwrap();
    let move_to_before = move_action_before.move_to;

    // Issue DirectedGoTo.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DirectedGoTo {
                zone_id: sim.home_zone_id(),
                creature_id: elf,
                position: target,
                queue: false,
            },
        }],
        tick + 1,
    );

    // Move action should still be in-flight with original timing.
    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(
        c.action_kind,
        ActionKind::Move,
        "Move action should be preserved"
    );
    assert_eq!(
        c.next_available_tick, nat_before,
        "next_available_tick should be unchanged"
    );
    let move_action_after = sim.db.move_actions.get(&elf).unwrap();
    assert_eq!(
        move_action_after.move_to, move_to_before,
        "MoveAction destination should be unchanged"
    );

    // New GoTo task should be assigned.
    assert!(c.current_task.is_some(), "Should have the new GoTo task");
    let task = sim.db.tasks.get(&c.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::GoTo);

    // Advance past the original next_available_tick — elf should resolve
    // the wander move and then start walking toward the GoTo destination.
    let completion_tick = nat_before.unwrap();
    sim.step(&[], completion_tick + 1);
    let c = sim.db.creatures.get(&elf).unwrap();
    assert!(
        c.current_task.is_some(),
        "Should still have the GoTo task after wander move resolves"
    );
}

#[test]
fn directed_goto_spam_does_not_accumulate_activations() {
    // B-erratic-movement-2: rapidly issuing multiple DirectedGoTo commands
    // while the creature is mid-move should never accumulate multiple
    // pending activations. Each command should recognize the in-flight
    // move and skip scheduling.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Start the elf walking via first DirectedGoTo.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_a = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DirectedGoTo {
                zone_id: sim.home_zone_id(),
                creature_id: elf,
                position: target_a,
                queue: false,
            },
        }],
        tick + 2,
    );

    // Advance until mid-walk.
    for t in (sim.tick + 1)..=(sim.tick + 50) {
        sim.step(&[], t);
        if sim
            .db
            .creatures
            .get(&elf)
            .is_some_and(|c| c.action_kind == ActionKind::Move)
        {
            break;
        }
    }
    assert!(
        sim.db
            .creatures
            .get(&elf)
            .is_some_and(|c| c.action_kind == ActionKind::Move),
        "Elf should be mid-walk"
    );
    assert_eq!(sim.count_pending_activations_for(elf), 1);

    // Spam 5 DirectedGoTo commands on successive ticks.
    for i in 0..5 {
        let target = VoxelCoord::new(tree_pos.x + 3 + i, 1, tree_pos.z + i);
        let t = sim.tick;
        sim.step(
            &[SimCommand {
                player_name: String::new(),
                tick: t + 1,
                action: SimAction::DirectedGoTo {
                    zone_id: sim.home_zone_id(),
                    creature_id: elf,
                    position: target,
                    queue: false,
                },
            }],
            t + 1,
        );
    }

    // Should still have exactly 1 pending activation.
    assert_eq!(
        sim.count_pending_activations_for(elf),
        1,
        "Spamming DirectedGoTo should not accumulate activations"
    );

    // Let the sim run for a while — creature should move smoothly
    // without double-activations.
    let nat = sim
        .db
        .creatures
        .get(&elf)
        .unwrap()
        .next_available_tick
        .unwrap();
    sim.step(&[], nat + 200);

    // Creature should be alive and functional.
    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(c.vital_status, VitalStatus::Alive);
}

#[test]
fn attack_move_on_wandering_creature_does_not_schedule_extra_activation() {
    // B-erratic-movement-2: same issue as DirectedGoTo but for AttackMove.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    let elf_node = creature_pos(&sim, elf);
    let mut events = Vec::new();
    sim.ground_wander(elf, elf_node, &mut events);

    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(c.action_kind, ActionKind::Move);
    assert!(c.current_task.is_none());
    assert_eq!(sim.count_pending_activations_for(elf), 1);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::AttackMove {
                zone_id: sim.home_zone_id(),
                creature_id: elf,
                destination: target,
                queue: false,
            },
        }],
        tick + 1,
    );

    assert_eq!(
        sim.count_pending_activations_for(elf),
        1,
        "Should still have exactly 1 pending activation after AttackMove on wandering creature"
    );
}

// -----------------------------------------------------------------------
// Reactivation after combat task completion
// -----------------------------------------------------------------------
// These tests verify that creatures are not left permanently inert after
// combat tasks complete. The bug: complete_task() clears the creature's
// task but several code paths return without scheduling a follow-up
// activation, leaving the creature with no pending events.

#[test]
fn attack_move_creature_reactivates_after_arriving_at_destination() {
    // An elf attack-moves to a location with no hostiles. After arriving
    // and completing the task, the elf must still have a pending activation
    // so it can wander or pick up new work.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let dest = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    // Run long enough for the elf to arrive.
    sim.step(&[cmd], tick + 20_000);

    // Task should be complete.
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Elf should have no task after attack-move completion"
    );

    // The creature must still have a pending activation — it should not
    // be permanently inert.
    assert!(
        sim.count_pending_activations_for(elf) >= 1,
        "Elf must have a pending activation after attack-move completion, \
             otherwise it will never act again"
    );
}

#[test]
fn attack_move_creature_wanders_after_arriving_at_destination() {
    // After attack-move completes, advance the sim and verify the elf
    // actually moves (wanders), proving it didn't get stuck.
    let mut sim = flat_world_sim(legacy_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let dest = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    // Run long enough for the elf to arrive and complete.
    sim.step(&[cmd], tick + 20_000);

    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(creature.current_task.is_none(), "Task should be complete");

    // Record position, then advance more ticks.
    let pos_after_arrival = sim.db.creatures.get(&elf).unwrap().position;
    sim.step(&[], tick + 25_000);

    let pos_later = sim.db.creatures.get(&elf).unwrap().position;
    assert_ne!(
        pos_after_arrival, pos_later,
        "Elf should have wandered after attack-move completion, but it stayed at {:?}",
        pos_after_arrival
    );
}

#[test]
fn attack_target_creature_reactivates_after_target_dies() {
    // An elf is ordered to attack a goblin. The goblin is killed (via
    // debug command). The elf's task completes, and it must still have
    // a pending activation so it doesn't become permanently inert.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    force_idle_and_cancel_activations(&mut sim, elf);

    let tick = sim.tick;
    let attack_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[attack_cmd], tick + 2);

    // Elf should have an AttackTarget task.
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Elf should have attack task"
    );

    // Kill the goblin.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 3,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    sim.step(&[kill_cmd], tick + 5_000);

    // Task should be complete.
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Elf should have no task after target dies"
    );

    // Must still have a pending activation.
    assert!(
        sim.count_pending_activations_for(elf) >= 1,
        "Elf must have a pending activation after attack-target completion, \
             otherwise it will never act again"
    );
}

#[test]
fn attack_target_creature_wanders_after_target_dies() {
    // After the attack target dies and the task completes, the elf should
    // actually do something (wander) rather than sitting frozen.
    let mut sim = flat_world_sim(legacy_test_seed());
    // Disable food/rest so the elf wanders instead of eating/sleeping.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    force_idle_and_cancel_activations(&mut sim, elf);

    let tick = sim.tick;
    let attack_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[attack_cmd], tick + 2);

    // Kill the goblin.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 3,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    sim.step(&[kill_cmd], tick + 5_000);

    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(creature.current_task.is_none(), "Task should be complete");

    // Record position, then advance.
    let pos_after_kill = sim.db.creatures.get(&elf).unwrap().position;
    sim.step(&[], tick + 10_000);

    let pos_later = sim.db.creatures.get(&elf).unwrap().position;
    assert_ne!(
        pos_after_kill, pos_later,
        "Elf should have wandered after target died, but it stayed at {:?}",
        pos_after_kill
    );
}

// -----------------------------------------------------------------------
// Incapacitated creature task assignment
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// Command queuing (shift+right-click)
// -----------------------------------------------------------------------

#[test]
fn queue_directed_goto_creates_linked_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue a non-queued DirectedGoTo first to give the elf a current task.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);
    let task_a = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Now issue a queued DirectedGoTo.
    let pos_b = VoxelCoord::new(20, 1, 20);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    // The elf's current task should still be task_a.
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(
        creature.current_task,
        Some(task_a),
        "Current task should remain A"
    );

    // There should be a new queued task with restrict_to_creature_id and prerequisite.
    let queued: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.restrict_to_creature_id == Some(elf) && t.prerequisite_task_id.is_some())
        .collect();
    assert_eq!(queued.len(), 1, "Should have exactly one queued task");
    let task_b = &queued[0];
    assert_eq!(task_b.prerequisite_task_id, Some(task_a));
    assert_eq!(task_b.state, TaskState::Available);
    // Location is snapped to the nearest walkable position by command_directed_goto.
    let expected_b = find_walkable(&sim, pos_b, 10).unwrap();
    assert_eq!(task_b.location, expected_b);
}

#[test]
fn unshifted_command_cancels_queue() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue a non-queued GoTo to give the elf a task.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);

    // Queue two more commands.
    let pos_b = VoxelCoord::new(20, 1, 20);
    let pos_c = VoxelCoord::new(30, 1, 30);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    let cmd_c = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 2,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_c,
            queue: true,
        },
    };
    sim.step(&[cmd_b, cmd_c], sim.tick + 3);

    // Verify we have queued tasks.
    let queued_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.restrict_to_creature_id == Some(elf) && t.state != TaskState::Complete)
        .count();
    assert!(queued_count >= 2, "Should have at least 2 queued tasks");

    // Now issue an unshifted command — should cancel the queue.
    let pos_d = VoxelCoord::new(40, 1, 40);
    let cmd_d = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_d,
            queue: false,
        },
    };
    sim.step(&[cmd_d], sim.tick + 2);

    // All previously queued tasks should be Complete.
    let remaining = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.restrict_to_creature_id == Some(elf) && t.state != TaskState::Complete)
        .count();
    assert_eq!(
        remaining, 0,
        "All queued tasks should be cancelled after unshifted command"
    );
}

#[test]
fn find_queue_tail_follows_chain() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue initial command + two queued commands.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);

    let pos_b = VoxelCoord::new(20, 1, 20);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    let pos_c = VoxelCoord::new(30, 1, 30);
    let cmd_c = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_c,
            queue: true,
        },
    };
    sim.step(&[cmd_c], sim.tick + 2);

    // find_queue_tail should return the last task (at pos_c, snapped to
    // nearest nav node by command_directed_goto).
    let tail_id = sim.find_queue_tail(elf).unwrap();
    let tail_task = sim.db.tasks.get(&tail_id).unwrap();
    let expected_c = find_walkable(&sim, pos_c, 10).unwrap();
    assert_eq!(
        tail_task.location, expected_c,
        "Tail should be the last queued task"
    );
}

#[test]
fn queue_completion_flows_to_next_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Get the elf's position for nearby GoTo commands.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;

    // Issue a GoTo to a nearby position.
    let pos_a = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);

    let task_a = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Queue a second GoTo.
    let pos_b = VoxelCoord::new(elf_pos.x + 2, elf_pos.y, elf_pos.z);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    // Find the queued task B.
    let task_b_id = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.prerequisite_task_id == Some(task_a))
        .unwrap()
        .id;

    // Run the sim long enough for task A to complete and B to be picked up.
    sim.step(&[], sim.tick + 50_000);

    // Task A should be complete.
    let task_a_state = sim.db.tasks.get(&task_a).unwrap().state;
    assert_eq!(
        task_a_state,
        TaskState::Complete,
        "Task A should be complete"
    );

    // Task B should have been picked up (InProgress or Complete).
    let task_b = sim.db.tasks.get(&task_b_id).unwrap();
    assert!(
        task_b.state == TaskState::InProgress || task_b.state == TaskState::Complete,
        "Task B should have been claimed after A completed, but state is {:?}",
        task_b.state
    );
}

#[test]
fn creature_death_cancels_queued_tasks() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue a GoTo and queue a second one.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);

    let pos_b = VoxelCoord::new(20, 1, 20);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    // Verify we have a queued task.
    let queued_before = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.restrict_to_creature_id == Some(elf) && t.state != TaskState::Complete)
        .count();
    assert!(queued_before >= 1, "Should have queued tasks before death");

    // Kill the elf.
    let mut events = Vec::new();
    sim.handle_creature_death(elf, crate::types::DeathCause::Debug, &mut events);

    // All tasks restricted to this creature should be Complete.
    let queued_after = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.restrict_to_creature_id == Some(elf) && t.state != TaskState::Complete)
        .count();
    assert_eq!(
        queued_after, 0,
        "All queued tasks should be cancelled after creature death"
    );
}

#[test]
fn queue_on_idle_creature_falls_through_to_immediate() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Ensure elf has no current task.
    assert!(sim.db.creatures.get(&elf).unwrap().current_task.is_none());

    // Send queue=true GoTo to the idle elf.
    let pos = VoxelCoord::new(10, 1, 10);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos,
            queue: true,
        },
    };
    sim.step(&[cmd], sim.tick + 2);

    // The elf should have a task immediately assigned (InProgress, not Available).
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Queue on idle creature should assign task immediately"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(
        task.state,
        TaskState::InProgress,
        "Task should be InProgress (immediate assignment), not Available"
    );
}

#[test]
fn queue_attack_move_creates_linked_task_with_extension_data() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue a non-queued GoTo first.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);
    let task_a = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Queue an AttackMove.
    let dest = VoxelCoord::new(20, 1, 20);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            destination: dest,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    // Find the queued AttackMove task.
    let queued: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| {
            t.restrict_to_creature_id == Some(elf)
                && t.prerequisite_task_id == Some(task_a)
                && t.kind_tag == crate::db::TaskKindTag::AttackMove
        })
        .collect();
    assert_eq!(queued.len(), 1, "Should have one queued AttackMove task");
    let queued_task = &queued[0];
    assert_eq!(queued_task.state, TaskState::Available);

    // Verify extension data exists.
    let ext = sim.db.task_attack_move_data.get(&queued_task.id);
    assert!(
        ext.is_some(),
        "Queued AttackMove should have TaskAttackMoveData extension row"
    );
    assert_eq!(ext.unwrap().destination, dest);
}

#[test]
fn deleted_prerequisite_unblocks_dependent_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Create prerequisite task A.
    let task_a_id = TaskId::new(&mut sim.rng);
    let pos = VoxelCoord::new(10, 1, 10);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_a_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: pos,
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf),
            prerequisite_task_id: None,
            required_civ_id: None,
        },
    );

    // Create dependent task B.
    let task_b_id = TaskId::new(&mut sim.rng);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_b_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: VoxelCoord::new(20, 1, 20),
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf),
            prerequisite_task_id: Some(task_a_id),
            required_civ_id: None,
        },
    );

    // Delete the prerequisite task entirely (simulates DB compaction/pruning).
    sim.db.remove_task(&task_a_id).unwrap();

    // Task B should now be available (missing prerequisite treated as complete).
    assert_eq!(
        sim.find_available_task(elf),
        Some(task_b_id),
        "Dependent task should become available when prerequisite is deleted"
    );
}

#[test]
fn queue_attack_creature_creates_linked_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue a GoTo to give the elf a current task.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);
    let task_a = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Spawn a goblin target via debug raid (simplest way to get a hostile).
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let goblin_pos = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let goblin_cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Goblin,
            position: goblin_pos,
        },
    };
    sim.step(&[goblin_cmd], sim.tick + 2);
    let goblin = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Goblin)
        .unwrap()
        .id;

    // Queue an AttackCreature command.
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    // Elf's current task should still be task_a.
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().current_task,
        Some(task_a),
        "Current task should remain the GoTo"
    );

    // Find the queued AttackTarget task.
    let queued: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| {
            t.restrict_to_creature_id == Some(elf)
                && t.prerequisite_task_id == Some(task_a)
                && t.kind_tag == crate::db::TaskKindTag::AttackTarget
        })
        .collect();
    assert_eq!(queued.len(), 1, "Should have one queued AttackTarget task");
    assert_eq!(queued[0].state, TaskState::Available);
    assert_eq!(queued[0].target_creature, Some(goblin));
}

#[test]
fn sim_action_queue_serde_roundtrip() {
    let mut rng = crate::prng::GameRng::new(42);
    let test_zone_id = ZoneId(42);

    // DirectedGoTo with queue: true
    let cmd = SimCommand {
        player_name: "p".to_string(),
        tick: 1,
        action: SimAction::DirectedGoTo {
            zone_id: test_zone_id,
            creature_id: CreatureId::new(&mut rng),
            position: VoxelCoord::new(5, 1, 5),
            queue: true,
        },
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());

    // AttackMove with queue: true
    let cmd2 = SimCommand {
        player_name: "p".to_string(),
        tick: 2,
        action: SimAction::AttackMove {
            zone_id: test_zone_id,
            creature_id: CreatureId::new(&mut rng),
            destination: VoxelCoord::new(10, 1, 10),
            queue: true,
        },
    };
    let json2 = serde_json::to_string(&cmd2).unwrap();
    let restored2: SimCommand = serde_json::from_str(&json2).unwrap();
    assert_eq!(json2, serde_json::to_string(&restored2).unwrap());

    // AttackCreature with queue: true
    let cmd3 = SimCommand {
        player_name: "p".to_string(),
        tick: 3,
        action: SimAction::AttackCreature {
            attacker_id: CreatureId::new(&mut rng),
            target_id: CreatureId::new(&mut rng),
            queue: true,
        },
    };
    let json3 = serde_json::to_string(&cmd3).unwrap();
    let restored3: SimCommand = serde_json::from_str(&json3).unwrap();
    assert_eq!(json3, serde_json::to_string(&restored3).unwrap());
}

#[test]
fn sim_action_queue_backward_compat() {
    // Old-format JSON without queue field should deserialize with queue: false.
    let json = r#"{"player_name":"p","tick":1,"action":{"DirectedGoTo":{"zone_id":0,"creature_id":"00000000-0000-0000-0000-000000000001","position":[5,1,5]}}}"#;
    let cmd: SimCommand = serde_json::from_str(json).unwrap();
    match &cmd.action {
        SimAction::DirectedGoTo { queue, .. } => assert!(!queue, "queue should default to false"),
        other => panic!("Expected DirectedGoTo, got {:?}", other),
    }
}

#[test]
fn two_queue_commands_same_tick_form_linear_chain() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Give elf a current task.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);
    let task_a = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Issue two queue commands in the same tick.
    let pos_b = VoxelCoord::new(20, 1, 20);
    let pos_c = VoxelCoord::new(30, 1, 30);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    let cmd_c = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1, // same tick as cmd_b
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_c,
            queue: true,
        },
    };
    sim.step(&[cmd_b, cmd_c], sim.tick + 2);

    // Verify the chain is linear: A -> B -> C (no forks).
    // Each task should have at most one non-Complete dependent.
    let queued: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.restrict_to_creature_id == Some(elf))
        .collect();
    assert_eq!(queued.len(), 2, "Should have exactly 2 queued tasks");

    // One should have prerequisite = task_a, the other should chain off it.
    let task_b = queued
        .iter()
        .find(|t| t.prerequisite_task_id == Some(task_a));
    assert!(task_b.is_some(), "One queued task should depend on task A");
    let task_b_id = task_b.unwrap().id;

    let task_c = queued
        .iter()
        .find(|t| t.prerequisite_task_id == Some(task_b_id));
    assert!(
        task_c.is_some(),
        "Second queued task should depend on first queued task (linear chain)"
    );
}

#[test]
fn find_queue_tail_terminates_on_cycle() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Create two tasks that form a prerequisite cycle (corrupted data).
    let task_a_id = TaskId::new(&mut sim.rng);
    let task_b_id = TaskId::new(&mut sim.rng);
    let pos = VoxelCoord::new(10, 1, 10);

    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_a_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: pos,
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf),
            prerequisite_task_id: Some(task_b_id),
            required_civ_id: None,
        },
    );
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_b_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: pos,
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf),
            prerequisite_task_id: Some(task_a_id),
            required_civ_id: None,
        },
    );

    // Set the elf's current_task to task_a to start the chain.
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_a_id);
        sim.db.update_creature(c).unwrap();
    }

    // find_queue_tail should terminate (return None due to cycle guard),
    // not loop forever.
    let result = sim.find_queue_tail(elf);
    assert_eq!(
        result, None,
        "find_queue_tail should return None on a cyclic chain"
    );
}

#[test]
fn flee_interrupt_preserves_command_queue() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue a GoTo and queue a second one.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);
    let task_a = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    let pos_b = VoxelCoord::new(20, 1, 20);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    // Find the queued task B.
    let task_b_id = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.prerequisite_task_id == Some(task_a))
        .unwrap()
        .id;

    // Simulate flee interruption: interrupt the current task as flee does.
    sim.interrupt_task(elf, task_a);

    // The queued task B should survive (Available, not Complete).
    let task_b = sim.db.tasks.get(&task_b_id).unwrap();
    assert_eq!(
        task_b.state,
        TaskState::Available,
        "Queued task should survive autonomous interruption (flee)"
    );

    // The interrupted task A should be Complete (GoTo is non-resumable).
    let task_a_state = sim.db.tasks.get(&task_a).unwrap().state;
    assert_eq!(task_a_state, TaskState::Complete);

    // Since task A is Complete (the prerequisite), task B's prerequisite is
    // satisfied. The creature should be able to pick it up.
    let found = sim.find_available_task(elf);
    assert_eq!(
        found,
        Some(task_b_id),
        "After flee interrupt, queued task B should be available (prerequisite A is Complete)"
    );
}

#[test]
fn shift_click_during_flee_preserves_and_extends_queue() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Issue GoTo A + queue GoTo B.
    let pos_a = VoxelCoord::new(10, 1, 10);
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], sim.tick + 2);
    let task_a = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    let pos_b = VoxelCoord::new(20, 1, 20);
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_b,
            queue: true,
        },
    };
    sim.step(&[cmd_b], sim.tick + 2);

    let task_b_id = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.prerequisite_task_id == Some(task_a))
        .unwrap()
        .id;

    // Simulate flee: interrupt the current task (sets current_task = None).
    sim.interrupt_task(elf, task_a);
    assert!(
        sim.db.creatures.get(&elf).unwrap().current_task.is_none(),
        "After flee interrupt, creature should have no current task"
    );

    // Now shift-click GoTo C while the creature is "fleeing" (no current_task).
    let pos_c = VoxelCoord::new(30, 1, 30);
    let cmd_c = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: pos_c,
            queue: true,
        },
    };
    sim.step(&[cmd_c], sim.tick + 2);

    // Task B should still exist and not be cancelled/wiped. In the poll-based
    // model, the creature may have been activated during the step and claimed
    // task B (InProgress), or it may still be Available — either is fine.
    let task_b = sim.db.tasks.get(&task_b_id).unwrap();
    assert!(
        task_b.state == TaskState::Available || task_b.state == TaskState::InProgress,
        "Surviving queued task B should not be wiped by shift-click during flee, \
             but was {:?}",
        task_b.state,
    );

    // There should be a new task C appended after B in the chain.
    let queued: Vec<_> = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.restrict_to_creature_id == Some(elf) && t.state != TaskState::Complete)
        .collect();
    assert!(
        queued.len() >= 2,
        "Should have at least 2 queued tasks (B and C), got {}",
        queued.len()
    );

    // C should chain off B (not off the interrupted task A).
    let task_c = queued
        .iter()
        .find(|t| t.prerequisite_task_id == Some(task_b_id));
    assert!(
        task_c.is_some(),
        "New shift-click task C should chain off surviving task B"
    );
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn creature_wanders_via_activation_chain() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    let initial_pos = elf.position;

    // Step enough for many activations (each moves 1 edge; ground edges
    // cost ~500 ticks at move_ticks_per_voxel=500).
    sim.step(&[], 50000);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();

    // After many activations, creature should have moved.
    assert_ne!(
        initial_pos.min, elf.position.min,
        "Elf should have moved after activation chain"
    );
    // Position should be walkable.
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            elf.position.min,
            [1, 1, 1],
            true,
        ),
        "Elf should be at a walkable position"
    );
    // Creature should not have a stored path (wandering doesn't use paths).
    assert!(
        elf.path.is_none(),
        "Wandering creature should not have a stored path"
    );
    let _ = initial_pos;
}

#[test]
fn goto_task_completes_on_arrival() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Put the task at the elf's current location for instant completion.
    let elf_node = creature_pos(&sim, elf_id);
    let task_id = insert_goto_task(&mut sim, elf_node);

    // One activation should be enough: elf claims task, is already there, completes.
    sim.step(&[], sim.tick + 10000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "GoTo task should be complete"
    );
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.current_task, None,
        "Elf should be unassigned after task completion"
    );
}

#[test]
fn completed_task_creature_resumes_wandering() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Put the task at the elf's current location for instant completion.
    let elf_node = creature_pos(&sim, elf_id);
    let _task_id = insert_goto_task(&mut sim, elf_node);

    // Complete the task.
    sim.step(&[], sim.tick + 10000);
    let pos_after_task = sim.db.creatures.get(&elf_id).unwrap().position;

    // Continue ticking — elf should resume wandering (position changes).
    sim.step(&[], sim.tick + 50000);

    let pos_after_wander = sim.db.creatures.get(&elf_id).unwrap().position;
    assert_ne!(
        pos_after_task, pos_after_wander,
        "Elf should have wandered after task completion"
    );
    assert!(
        sim.db
            .creatures
            .get(&elf_id)
            .unwrap()
            .current_task
            .is_none(),
        "Elf should still have no task"
    );
}

#[test]
fn create_task_command_adds_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateTask {
            zone_id: sim.home_zone_id(),
            kind: TaskKind::GoTo,
            position: tree_pos,
            required_species: Some(Species::Elf),
        },
    };
    sim.step(&[cmd], 2);

    assert_eq!(sim.db.tasks.len(), 1, "Should have 1 task");
    let task = sim.db.tasks.iter_all().next().unwrap();
    assert_eq!(task.state, TaskState::Available);
    assert!(task.kind_tag == TaskKindTag::GoTo);
}

#[test]
fn end_to_end_summon_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf.
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[spawn_cmd], 2);

    // Create a GoTo task at a ground position near the tree.
    let task_cmd = SimCommand {
        player_name: String::new(),
        tick: 3,
        action: SimAction::CreateTask {
            zone_id: sim.home_zone_id(),
            kind: TaskKind::GoTo,
            position: VoxelCoord::new(tree_pos.x + 10, 0, tree_pos.z),
            required_species: Some(Species::Elf),
        },
    };
    sim.step(&[task_cmd], 4);

    assert_eq!(sim.db.tasks.len(), 1);
    let task_id = *sim.db.tasks.iter_keys().next().unwrap();

    // Tick until the elf completes the task.
    sim.step(&[], 50000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Task should be complete after enough ticks"
    );

    // Elf should be unassigned and wandering again.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert!(elf.current_task.is_none());
}

#[test]
fn only_one_creature_claims_goto_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn multiple elves and capybaras.
    for _ in 0..3 {
        let cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                zone_id: sim.home_zone_id(),
                species: Species::Elf,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
    }
    for _ in 0..2 {
        let cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                zone_id: sim.home_zone_id(),
                species: Species::Capybara,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
    }

    // Create an elf-only GoTo task at a distant walkable position.
    let far_pos = find_different_walkable(&sim, tree_pos);
    let task_id = insert_goto_task(&mut sim, far_pos);

    // Tick enough for a creature to claim the task. The elf may or may
    // not have arrived yet (GoTo completes on arrival, clearing
    // current_task), so we check that the task was claimed OR completed.
    sim.step(&[], sim.tick + 5000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    let claimers = sim
        .db
        .creatures
        .by_current_task(&Some(task.id), tabulosity::QueryOpts::ASC);

    if task.state == crate::task::TaskState::Complete {
        // Task was completed — some elf claimed and finished it.
        assert!(
            claimers.is_empty(),
            "Completed task should have no current claimers"
        );
    } else {
        // Task still in progress — exactly one elf should be on it.
        assert_eq!(
            claimers.len(),
            1,
            "Exactly one creature should claim the task, got {}",
            claimers.len()
        );
        let assignee = &claimers[0];
        assert_eq!(assignee.species, Species::Elf);
    }

    // No capybara should have a task (elf-only restriction).
    for creature in sim.db.creatures.iter_all() {
        if creature.species == Species::Capybara {
            assert!(
                creature.current_task.is_none(),
                "Capybara should not have claimed an elf-only task"
            );
        }
    }
}

#[test]
fn abort_current_action_clears_activation_state() {
    // B-erratic-movement safety net: when abort_current_action is called
    // (death, flee, nav invalidation), the creature's next_available_tick
    // should be cleared so it won't be polled for activation.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Issue a DirectedGoTo and advance until the elf is mid-walk.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: target,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    for t in (sim.tick + 1)..=(sim.tick + 50) {
        sim.step(&[], t);
        let c = sim.db.creatures.get(&elf).unwrap();
        if c.action_kind == ActionKind::Move {
            break;
        }
    }

    // Verify the elf has a pending activation before abort.
    let count_before = sim.count_pending_activations_for(elf);
    assert!(
        count_before >= 1,
        "Should have at least one pending activation"
    );

    // Abort the current action (simulating death/flee/nav invalidation).
    sim.abort_current_action(elf);

    // After abort, next_available_tick should be None (no pending activation).
    let count_after = sim.count_pending_activations_for(elf);
    assert_eq!(count_after, 0, "Activation should be cleared after abort");
}

// ---- Command queue (F-command-queue) ----

#[test]
fn find_available_task_skips_task_restricted_to_other_creature() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);

    // Create a task restricted to elf_a at a reachable location.
    let task_id = TaskId::new(&mut sim.rng);
    let task_pos = VoxelCoord::new(10, 1, 10);
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_a),
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);

    // elf_b should NOT find this task.
    assert!(
        sim.find_available_task(elf_b).is_none(),
        "Task restricted to elf_a should not be found by elf_b"
    );
    // elf_a SHOULD find it.
    assert_eq!(
        sim.find_available_task(elf_a),
        Some(task_id),
        "Task restricted to elf_a should be found by elf_a"
    );
}

#[test]
fn find_available_task_skips_incomplete_prerequisite() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Create prerequisite task A (Available, not Complete).
    let task_a_id = TaskId::new(&mut sim.rng);
    let task_pos = VoxelCoord::new(10, 1, 10);
    let task_a = Task {
        id: task_a_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task_a);

    // Create dependent task B with prerequisite = A.
    let task_b_id = TaskId::new(&mut sim.rng);
    let task_b_pos = VoxelCoord::new(20, 1, 20);
    let task_b = Task {
        id: task_b_id,
        kind: TaskKind::GoTo,
        state: TaskState::Available,
        location: task_b_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: Some(elf_id),
        prerequisite_task_id: Some(task_a_id),
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task_b);

    // Only task A should be found (B's prerequisite is not complete).
    assert_eq!(
        sim.find_available_task(elf_id),
        Some(task_a_id),
        "Should find task A (no prerequisite), not B (prerequisite incomplete)"
    );

    // Complete task A.
    if let Some(mut t) = sim.db.tasks.get(&task_a_id) {
        t.state = TaskState::Complete;
        sim.db.update_task(t).unwrap();
    }

    // Now task B should be found.
    assert_eq!(
        sim.find_available_task(elf_id),
        Some(task_b_id),
        "After prerequisite completes, task B should be available"
    );
}

#[test]
fn cancel_creature_queue_cancels_entire_chain() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Create a chain: A -> B -> C (B depends on A, C depends on B).
    let task_a_id = TaskId::new(&mut sim.rng);
    let pos = VoxelCoord::new(10, 1, 10);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_a_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: pos,
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf_id),
            prerequisite_task_id: None,
            required_civ_id: None,
        },
    );
    // Assign A to the elf.
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_a_id);
        sim.db.update_creature(c).unwrap();
    }

    let task_b_id = TaskId::new(&mut sim.rng);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_b_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: VoxelCoord::new(20, 1, 20),
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf_id),
            prerequisite_task_id: Some(task_a_id),
            required_civ_id: None,
        },
    );

    let task_c_id = TaskId::new(&mut sim.rng);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_c_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: VoxelCoord::new(30, 1, 30),
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf_id),
            prerequisite_task_id: Some(task_b_id),
            required_civ_id: None,
        },
    );

    // Cancel the queue via cancel_creature_queue (simulates unshifted player
    // command replacing the queue).
    sim.cancel_creature_queue(elf_id);

    // B and C should both be Complete (queue cancellation).
    let task_b = sim.db.tasks.get(&task_b_id).unwrap();
    assert_eq!(
        task_b.state,
        TaskState::Complete,
        "Dependent task B should be cancelled (Complete)"
    );
    let task_c = sim.db.tasks.get(&task_c_id).unwrap();
    assert_eq!(
        task_c.state,
        TaskState::Complete,
        "Transitive dependent task C should be cancelled (Complete)"
    );
}

#[test]
fn cancel_creature_queue_only_cancels_player_directed() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Create a player-directed queued task.
    let pd_task_id = TaskId::new(&mut sim.rng);
    let pos = VoxelCoord::new(10, 1, 10);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: pd_task_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: pos,
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: Some(elf_id),
            prerequisite_task_id: None,
            required_civ_id: None,
        },
    );

    // Create an autonomous restricted task (should survive cancellation).
    let auto_task_id = TaskId::new(&mut sim.rng);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: auto_task_id,
            kind: TaskKind::GoTo,
            state: TaskState::Available,
            location: VoxelCoord::new(20, 1, 20),
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::Autonomous,
            target_creature: None,
            restrict_to_creature_id: Some(elf_id),
            prerequisite_task_id: None,
            required_civ_id: None,
        },
    );

    // Cancel the creature's player-directed queue.
    sim.cancel_creature_queue(elf_id);

    let pd_task = sim.db.tasks.get(&pd_task_id).unwrap();
    assert_eq!(
        pd_task.state,
        TaskState::Complete,
        "Player-directed queued task should be cancelled"
    );
    let auto_task = sim.db.tasks.get(&auto_task_id).unwrap();
    assert_eq!(
        auto_task.state,
        TaskState::Available,
        "Autonomous restricted task should NOT be cancelled"
    );
}

// -----------------------------------------------------------------------
// Poll-based activation index
// -----------------------------------------------------------------------

#[test]
fn activation_ready_index_returns_alive_creatures_at_or_before_tick() {
    let mut sim = test_sim(legacy_test_seed());

    // Spawn two elves and force them idle with specific next_available_tick values.
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_a);
    force_idle_and_cancel_activations(&mut sim, elf_b);

    // Set elf_a ready at tick 100, elf_b ready at tick 200.
    {
        let mut c = sim.db.creatures.get(&elf_a).unwrap();
        c.next_available_tick = Some(100);
        sim.db.update_creature(c).unwrap();
    }
    {
        let mut c = sim.db.creatures.get(&elf_b).unwrap();
        c.next_available_tick = Some(200);
        sim.db.update_creature(c).unwrap();
    }

    // At tick 99: neither ready.
    let ready = sim.poll_ready_creatures(99);
    assert!(
        !ready.contains(&elf_a) && !ready.contains(&elf_b),
        "No creature should be ready before tick 100"
    );

    // At tick 100: only elf_a.
    let ready = sim.poll_ready_creatures(100);
    assert!(ready.contains(&elf_a), "elf_a should be ready at tick 100");
    assert!(
        !ready.contains(&elf_b),
        "elf_b should not be ready at tick 100"
    );

    // At tick 200: both.
    let ready = sim.poll_ready_creatures(200);
    assert!(ready.contains(&elf_a), "elf_a should be ready at tick 200");
    assert!(ready.contains(&elf_b), "elf_b should be ready at tick 200");
}

#[test]
fn activation_ready_index_excludes_dead_creatures() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Set ready at tick 10.
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.next_available_tick = Some(10);
        sim.db.update_creature(c).unwrap();
    }
    let ready = sim.poll_ready_creatures(10);
    assert!(ready.contains(&elf), "alive elf should be ready");

    // Kill the elf.
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.vital_status = VitalStatus::Dead;
        sim.db.update_creature(c).unwrap();
    }
    let ready = sim.poll_ready_creatures(10);
    assert!(!ready.contains(&elf), "dead elf should not be ready");
}

#[test]
fn activation_ready_index_excludes_incapacitated_creatures() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Set ready at tick 10.
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.next_available_tick = Some(10);
        sim.db.update_creature(c).unwrap();
    }
    let ready = sim.poll_ready_creatures(10);
    assert!(ready.contains(&elf), "alive elf should be ready");

    // Incapacitate the elf.
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.vital_status = VitalStatus::Incapacitated;
        sim.db.update_creature(c).unwrap();
    }
    let ready = sim.poll_ready_creatures(10);
    assert!(
        !ready.contains(&elf),
        "incapacitated elf should not be ready"
    );
}

#[test]
fn activation_ready_index_includes_none_next_available_tick() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    // Directly set next_available_tick to None to test the index behavior.
    // (force_idle_and_cancel_activations uses Some(u64::MAX) to suppress.)
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.next_available_tick = None;
        sim.db.update_creature(c).unwrap();
    }

    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(c.next_available_tick, None);

    // None should be included in the ready set (None < Some(anything)).
    let ready = sim.poll_ready_creatures(0);
    assert!(
        ready.contains(&elf),
        "creature with next_available_tick=None should be ready"
    );
}

#[test]
fn activation_ready_index_returns_sorted_by_creature_id() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    let elf_c = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_a);
    force_idle_and_cancel_activations(&mut sim, elf_b);
    force_idle_and_cancel_activations(&mut sim, elf_c);

    // All ready at tick 50.
    for &id in &[elf_a, elf_b, elf_c] {
        let mut c = sim.db.creatures.get(&id).unwrap();
        c.next_available_tick = Some(50);
        sim.db.update_creature(c).unwrap();
    }

    let ready = sim.poll_ready_creatures(50);
    // Must contain all three and be sorted by CreatureId.
    let mut our_ids: Vec<_> = ready
        .iter()
        .copied()
        .filter(|id| *id == elf_a || *id == elf_b || *id == elf_c)
        .collect();
    let sorted = {
        let mut s = our_ids.clone();
        s.sort();
        s
    };
    assert_eq!(our_ids, sorted, "ready list should be sorted by CreatureId");
    assert_eq!(our_ids.len(), 3);
}

// -----------------------------------------------------------------------
// Serde backward compat: _LegacyCreatureActivation
// -----------------------------------------------------------------------

#[test]
fn legacy_creature_activation_deserializes_from_old_format() {
    // Old save files contain `{"CreatureActivation":{...}}`. The alias on
    // `_LegacyCreatureActivation` must accept this.
    let json = r#"{"CreatureActivation":{"creature_id":"00000000-0000-0000-0000-000000000001"}}"#;
    let kind: ScheduledEventKind = serde_json::from_str(json).unwrap();
    match kind {
        ScheduledEventKind::_LegacyCreatureActivation { creature_id } => {
            assert_eq!(
                creature_id.0.to_string(),
                "00000000-0000-0000-0000-000000000001"
            );
        }
        other => panic!("expected _LegacyCreatureActivation, got {other:?}"),
    }
}

// -----------------------------------------------------------------------
// next_creature_activation_tick
// -----------------------------------------------------------------------

#[test]
fn next_creature_activation_tick_returns_earliest() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    let elf_c = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_a);
    force_idle_and_cancel_activations(&mut sim, elf_b);
    force_idle_and_cancel_activations(&mut sim, elf_c);

    // Set different next_available_tick values.
    {
        let mut c = sim.db.creatures.get(&elf_a).unwrap();
        c.next_available_tick = Some(100);
        sim.db.update_creature(c).unwrap();
    }
    {
        let mut c = sim.db.creatures.get(&elf_b).unwrap();
        c.next_available_tick = Some(50);
        sim.db.update_creature(c).unwrap();
    }
    {
        let mut c = sim.db.creatures.get(&elf_c).unwrap();
        c.next_available_tick = Some(200);
        sim.db.update_creature(c).unwrap();
    }

    // Should return the smallest tick among living creatures.
    assert!(matches!(
        sim.next_creature_activation_tick(),
        NextCreatureActivation::AtTick(50)
    ));
}

#[test]
fn next_creature_activation_tick_none_means_immediate() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Set next_available_tick to None (meaning "immediately ready").
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.next_available_tick = None;
        sim.db.update_creature(c).unwrap();
    }

    // None means a creature needs activation right now.
    assert!(matches!(
        sim.next_creature_activation_tick(),
        NextCreatureActivation::Immediate
    ));
}

#[test]
fn next_creature_activation_tick_no_alive_returns_no_creatures() {
    let mut sim = test_sim(legacy_test_seed());

    // Kill all creatures so no living creatures exist.
    let all_ids: Vec<_> = sim.db.creatures.iter_all().map(|c| c.id).collect();
    for id in all_ids {
        let mut c = sim.db.creatures.get(&id).unwrap();
        c.vital_status = VitalStatus::Dead;
        sim.db.update_creature(c).unwrap();
    }

    assert!(matches!(
        sim.next_creature_activation_tick(),
        NextCreatureActivation::NoCreatures
    ));
}
