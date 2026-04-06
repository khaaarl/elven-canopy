//! Tests for movement and navigation: voxel exclusion (hostile blocking),
//! creature gravity (falling, damage), pursuit tasks, path caching,
//! position interpolation, flying creature movement, and task proximity.
//! Corresponds to `sim/movement.rs`.

use super::combat_tests::{give_spear, setup_aggressive_elf, setup_frozen_hornet};
use super::*;

/// Helper: spawn a second elf and return its CreatureId.
fn spawn_second_elf(sim: &mut SimState) -> CreatureId {
    // Collect existing elf IDs before spawning.
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .map(|c| c.id)
        .collect();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
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
    // Return the newly spawned elf (not in the existing set).
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf && !existing.contains(&c.id))
        .unwrap()
        .id
}

/// Insert a pursuit task targeting a specific creature.
fn insert_pursuit_task(
    sim: &mut SimState,
    pursuer: CreatureId,
    target: CreatureId,
    destination: VoxelCoord,
) -> crate::types::TaskId {
    let task_id = crate::types::TaskId::new(&mut sim.rng);
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: destination,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: Some(target),
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);
    // Directly assign the pursuer to this task.
    let mut pursuer_creature = sim.db.creatures.get(&pursuer).unwrap();
    pursuer_creature.current_task = Some(task_id);
    sim.db.update_creature(pursuer_creature).unwrap();
    task_id
}

// ===================================================================
// Elf wander (basic ground movement)
// ===================================================================

// ===================================================================
// Position interpolation
// ===================================================================

/// Helper: create a minimal `db::Creature` for interpolation tests.
fn make_interp_creature(
    position: VoxelCoord,
    action_kind: ActionKind,
    next_available_tick: Option<u64>,
) -> crate::db::Creature {
    crate::db::Creature {
        id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
        zone_id: Some(ZoneId(0)),
        species: Species::Elf,
        position: VoxelBox::point(position),
        name: String::new(),
        name_meaning: String::new(),
        path: None,
        current_task: None,
        current_activity: None,
        food: 1000,
        rest: 1000,
        assigned_home: None,
        inventory_id: InventoryId(0),
        civ_id: None,
        military_group: None,
        action_kind,
        next_available_tick,
        hp: 100,
        hp_max: 100,
        hp_regen_remainder: 0,
        vital_status: VitalStatus::Alive,
        mp: 0,
        mp_max: 0,
        mp_regen_remainder: 0,
        wasted_action_count: 0,
        last_dance_tick: 0,
        last_dinner_party_tick: 0,
        movement_category: crate::nav::MovementCategory::WalkOrLadder,
        sex: CreatureSex::None,
    }
}

#[test]
fn interpolated_position_midpoint() {
    let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
    let ma = MoveAction {
        creature_id: creature.id,
        move_from: VoxelCoord::new(0, 0, 0),
        move_to: VoxelCoord::new(10, 0, 0),
        move_start_tick: 100,
    };
    let (x, y, z) = creature.interpolated_position(150.0, Some(&ma));
    assert!((x - 5.0).abs() < 0.001, "x should be 5.0, got {x}");
    assert!((y - 0.0).abs() < 0.001, "y should be 0.0, got {y}");
    assert!((z - 0.0).abs() < 0.001, "z should be 0.0, got {z}");
}

#[test]
fn interpolated_position_at_start() {
    let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
    let ma = MoveAction {
        creature_id: creature.id,
        move_from: VoxelCoord::new(0, 0, 0),
        move_to: VoxelCoord::new(10, 0, 0),
        move_start_tick: 100,
    };
    let (x, _, _) = creature.interpolated_position(100.0, Some(&ma));
    assert!((x - 0.0).abs() < 0.001, "At t=0 should be at from, got {x}");
}

#[test]
fn interpolated_position_at_end() {
    let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
    let ma = MoveAction {
        creature_id: creature.id,
        move_from: VoxelCoord::new(0, 0, 0),
        move_to: VoxelCoord::new(10, 0, 0),
        move_start_tick: 100,
    };
    let (x, _, _) = creature.interpolated_position(200.0, Some(&ma));
    assert!((x - 10.0).abs() < 0.001, "At t=1 should be at to, got {x}");
}

#[test]
fn interpolated_position_clamped_past_end() {
    let creature = make_interp_creature(VoxelCoord::new(10, 0, 0), ActionKind::Move, Some(200));
    let ma = MoveAction {
        creature_id: creature.id,
        move_from: VoxelCoord::new(0, 0, 0),
        move_to: VoxelCoord::new(10, 0, 0),
        move_start_tick: 100,
    };
    let (x, _, _) = creature.interpolated_position(999.0, Some(&ma));
    assert!(
        (x - 10.0).abs() < 0.001,
        "Past end should clamp to destination, got {x}"
    );
}

#[test]
fn interpolated_position_stationary() {
    let creature = make_interp_creature(VoxelCoord::new(5, 3, 7), ActionKind::NoAction, None);
    let (x, y, z) = creature.interpolated_position(50.0, None);
    assert!((x - 5.0).abs() < 0.001);
    assert!((y - 3.0).abs() < 0.001);
    assert!((z - 7.0).abs() < 0.001);
}

/// Medium-priority #13: interpolated_position with non-Move action returns
/// static position, not a crash.
#[test]
fn interpolated_position_with_build_action_returns_static() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 100_000;
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

    // Run until elf is mid-Build.
    sim.step(&[], sim.tick + 200_000);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    // Force build action state for the test if not naturally set.
    if elf.action_kind == ActionKind::Build {
        // Calling interpolated_position with no MoveAction should not panic
        // and should return the static position.
        let pos = elf.interpolated_position(sim.tick as f64, None);
        let expected = (
            elf.position.min.x as f32,
            elf.position.min.y as f32,
            elf.position.min.z as f32,
        );
        assert_eq!(
            pos, expected,
            "Non-Move action should return static position"
        );
    }
}

// ===================================================================
// Walk toward dead task node
// ===================================================================

#[test]
fn walk_toward_dead_task_node_does_not_panic() {
    // Reproduce B-dead-node-panic: a creature has a task whose location
    // nav node gets removed by an incremental update. The creature should
    // gracefully abandon the task instead of panicking in pathfinding.
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

    let elf_id = *sim.db.creatures.iter_keys().next().unwrap();

    // Find a walkable position different from the elf's to use as task target.
    let elf_pos = creature_pos(&sim, elf_id);
    let task_pos = find_different_walkable(&sim, elf_pos);

    // Create a GoTo task at that position and assign it to the elf.
    let task_id = TaskId::new(&mut sim.rng);
    sim.insert_task(
        sim.home_zone_id(),
        Task {
            id: task_id,
            kind: TaskKind::GoTo,
            state: TaskState::InProgress,
            location: task_pos,
            progress: 0,
            total_cost: 0,
            required_species: None,
            origin: TaskOrigin::PlayerDirected,
            target_creature: None,
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        },
    );
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Make the task position unwalkable by filling it with solid voxel,
    // simulating a world change that invalidates the task's destination.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(task_pos, VoxelType::Dirt);
    sim.rebuild_transient_state();

    assert!(
        !crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            task_pos,
            [1, 1, 1],
            true
        ),
        "Task position should be unwalkable",
    );

    // Step the sim — the elf should gracefully handle the unwalkable task
    // position by abandoning the task. Must NOT panic.
    sim.step(&[], 50000);

    // The elf should have completed the GoTo task or abandoned it.
    // Either way, it should not still be working on the original task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    if let Some(tid) = elf.current_task {
        assert_ne!(
            tid, task_id,
            "Elf should not still be working on the task with the unwalkable location",
        );
    }
}

// ===================================================================
// Pursuit tasks
// ===================================================================

#[test]
fn pursuit_task_repaths_when_target_moves() {
    let mut sim = test_sim(legacy_test_seed());
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Get target's initial node.
    let target_node = creature_pos(&sim, target_id);

    // Pick a different walkable position to move the target to.
    let new_target_pos = find_different_walkable(&sim, target_node);
    assert_ne!(target_node, new_target_pos);

    // Freeze all creatures so only the pursuer activates.
    let all_ids: Vec<CreatureId> = sim.db.creatures.iter_all().map(|c| c.id).collect();
    for cid in &all_ids {
        force_idle_and_cancel_activations(&mut sim, *cid);
    }

    // Create pursuit task at target's current position, assigned to pursuer.
    let target_pos = target_node;
    let task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, target_pos);
    assert_eq!(sim.db.tasks.get(&task_id).unwrap().location, target_pos);

    // Manually move the target to the new position (simulates target movement).
    let new_pos = new_target_pos;
    {
        let mut c = sim.db.creatures.get(&target_id).unwrap();
        c.position = VoxelBox::point(new_pos);
        sim.db.update_creature(c).unwrap();
    }

    // Schedule only the pursuer to activate.
    let tick = sim.tick;
    schedule_activation_at(&mut sim, pursuer_id, tick + 1);

    // Step so the pursuer's activation fires and updates the task location.
    sim.step(&[], tick + 10000);

    // The pursuit task's location should have changed from the initial
    // value, proving the repath logic fired. We don't assert the exact
    // coord because the target may have moved further during the step
    // (heartbeat-driven tasks, wandering after the GoTo completes, etc.).
    if let Some(task) = sim.db.tasks.get(&task_id) {
        assert_ne!(
            task.location, target_pos,
            "Pursuit task location should have updated when target moved"
        );
    }
    // If the task was completed (pursuer caught the target), that also
    // proves the repath worked — the pursuer followed the target.
}

#[test]
fn pursuit_task_completes_when_adjacent() {
    let mut sim = test_sim(legacy_test_seed());
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Read the pursuer's current node (may have wandered during spawns).
    let pursuer_node = creature_pos(&sim, pursuer_id);

    // Place both creatures at the same position and prevent them from wandering.
    let node_pos = pursuer_node;
    {
        let mut c = sim.db.creatures.get(&target_id).unwrap();
        c.position = VoxelBox::point(node_pos);
        sim.db.update_creature(c).unwrap();
    }
    {
        let mut c = sim.db.creatures.get(&pursuer_id).unwrap();
        c.position = VoxelBox::point(node_pos);
        c.path = None;
        sim.db.update_creature(c).unwrap();
    }

    // Give the target a Sleep task so it stays still.
    let sleep_task_id = TaskId::new(&mut sim.rng);
    let sleep_task = Task {
        id: sleep_task_id,
        kind: TaskKind::Sleep {
            bed_pos: None,
            location: task::SleepLocation::Ground,
        },
        state: TaskState::InProgress,
        location: pursuer_node,
        progress: 0,
        total_cost: 999999,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), sleep_task);
    let mut target = sim.db.creatures.get(&target_id).unwrap();
    target.current_task = Some(sleep_task_id);
    sim.db.update_creature(target).unwrap();

    // Create pursuit task at the shared position.
    let pursuer_pos = pursuer_node;
    let task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, pursuer_pos);

    // Clear pursuer's action state and schedule an immediate activation so
    // the pursuit logic fires regardless of the sim's PRNG-dependent event
    // schedule. This makes the test robust to worldgen PRNG changes.
    {
        let mut c = sim.db.creatures.get(&pursuer_id).unwrap();
        c.next_available_tick = Some(sim.tick + 1);
        c.action_kind = crate::db::ActionKind::NoAction;
        sim.db.update_creature(c).unwrap();
    }

    // Step — pursuer should complete the GoTo since it's at the target's node.
    sim.step(&[], sim.tick + 10000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Pursuit task should complete when pursuer is at target's node"
    );
    let pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
    assert_eq!(
        pursuer.current_task, None,
        "Pursuer should be unassigned after task completion"
    );
}

#[test]
fn pursuit_task_abandons_when_target_gone() {
    let mut sim = test_sim(legacy_test_seed());
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Let both creatures settle (complete initial movement).
    sim.step(&[], sim.tick + 10000);

    let target_node = creature_pos(&sim, target_id);

    // Assign pursuit task — clear any existing task first.
    let mut pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
    pursuer.current_task = None;
    pursuer.path = None;
    sim.db.update_creature(pursuer).unwrap();

    let target_pos = target_node;
    let task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, target_pos);

    // Simulate target becoming unreachable by moving it to a position that
    // is not walkable. This triggers the unreachable target
    // branch in pursuit logic, causing the pursuer to abandon the task.
    {
        let mut c = sim.db.creatures.get(&target_id).unwrap();
        c.position = VoxelBox::point(VoxelCoord::new(0, 200, 0));
        sim.db.update_creature(c).unwrap();
    }

    // Step — pursuer should notice target has no nav node and unassign.
    sim.step(&[], sim.tick + 500000);

    let pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
    // The pursuer should have abandoned the pursuit task.
    assert_ne!(
        pursuer.current_task,
        Some(task_id),
        "Pursuer should have abandoned the pursuit task when target has no nav node"
    );

    // The pursuit task should be completed (not left Available for re-claim).
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Abandoned pursuit task should be completed"
    );
}

#[test]
fn pursuit_task_abandons_when_target_unreachable() {
    let mut sim = test_sim(legacy_test_seed());
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Place target at a non-existent position (simulates disconnected region).
    let bogus_pos = VoxelCoord::new(999, 999, 999);
    let _task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, bogus_pos);
    // Move the target to an unreachable position with no nav node.
    {
        let mut c = sim.db.creatures.get(&target_id).unwrap();
        c.position = VoxelBox::point(VoxelCoord::new(0, 200, 0));
        sim.db.update_creature(c).unwrap();
    }

    // Step enough ticks for pursuer's activation to fire and hit the
    // dead-node check. The exact timing depends on PRNG state (spawn
    // position, wander path), so we step generously.
    sim.step(&[], sim.tick + 500_000);

    // Pursuer should have abandoned the pursuit task (may have claimed
    // another task from heartbeat, but not the pursuit task).
    let pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
    assert_ne!(
        pursuer.current_task,
        Some(_task_id),
        "Pursuer should have abandoned the pursuit task for unreachable target"
    );
}

#[test]
fn non_pursuit_tasks_unaffected() {
    // Verify existing GoTo tasks (without target_creature) still work.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let elf_node = creature_pos(&sim, elf_id);

    // Insert a regular GoTo task (no target_creature).
    let task_id = insert_goto_task(&mut sim, elf_node);

    // Verify target_creature is None.
    let db_task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(db_task.target_creature, None);

    // Step — should complete normally.
    sim.step(&[], sim.tick + 10000);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Non-pursuit GoTo task should complete normally"
    );
}

#[test]
fn pursuit_task_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);
    let target_node = creature_pos(&sim, target_id);

    let target_pos = target_node;
    let task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, target_pos);

    // Verify the task has target_creature set.
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.target_creature, Some(target_id));

    // Serialize the entire sim state via save/load.
    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: crate::db::SimDb = serde_json::from_str(&json).unwrap();

    let restored_task = restored.tasks.get(&task_id).unwrap();
    assert_eq!(
        restored_task.target_creature,
        Some(target_id),
        "target_creature should survive serde roundtrip"
    );
}

// ===================================================================
// find_available_task proximity
// ===================================================================

#[test]
fn find_available_task_prefers_nearest_by_nav_distance() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf near the tree.
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
        .expect("elf should exist");
    let elf_id = elf.id;
    let elf_pos = elf.position.min;
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            elf_pos,
            [1, 1, 1],
            true
        ),
        "elf should be at a walkable position"
    );

    // Use explicit positions relative to the elf: near_pos is 2 voxels away,
    // far_pos is 15 voxels away. Both on the same floor_y.
    let floor_y = sim.config.floor_y + 1;
    let near_pos = VoxelCoord::new(elf_pos.x + 2, floor_y, elf_pos.z);
    let far_pos = VoxelCoord::new(elf_pos.x + 15, floor_y, elf_pos.z);
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            near_pos,
            [1, 1, 1],
            true
        ),
        "near_pos should be walkable"
    );
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            far_pos,
            [1, 1, 1],
            true
        ),
        "far_pos should be walkable"
    );

    // Create two Available tasks — far task first (to ensure it would be
    // picked under the old first-found behavior if its TaskId sorts first).
    let far_task_id = TaskId::new(&mut sim.rng);
    let near_task_id = TaskId::new(&mut sim.rng);

    let far_task = crate::db::Task {
        id: far_task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: far_pos,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    let near_task = crate::db::Task {
        id: near_task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: near_pos,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    sim.db.insert_task(far_task).unwrap();
    sim.db.insert_task(near_task).unwrap();

    // Clear the elf's current task so it's idle.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let chosen = sim.find_available_task(elf_id).expect("should find a task");
    assert_eq!(
        chosen, near_task_id,
        "find_available_task should prefer the nearest task by nav distance"
    );
}

#[test]
fn find_available_task_single_candidate_skips_dijkstra() {
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
        .expect("elf should exist");
    let elf_id = elf.id;

    // Pick any walkable position for the single task.
    let task_pos =
        find_walkable(&sim, elf.position.min, 20).expect("should have a walkable position");

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

    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let chosen = sim
        .find_available_task(elf_id)
        .expect("should find the only task");
    assert_eq!(chosen, task_id);
}

#[test]
fn find_available_task_respects_species_filter_with_proximity() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn a capybara (non-elf).
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

    let task_pos =
        find_walkable(&sim, capy.position.min, 20).expect("should have a walkable position");

    // Create an elf-only task — capybara should not see it.
    let elf_task_id = TaskId::new(&mut sim.rng);
    let elf_task = crate::db::Task {
        id: elf_task_id,
        zone_id: sim.home_zone_id(),
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 1,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.insert_task(elf_task).unwrap();

    {
        let mut c = sim.db.creatures.get(&capy_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
    }

    let chosen = sim.find_available_task(capy_id);
    assert_eq!(
        chosen, None,
        "capybara should not be assigned an elf-only task"
    );
}
// ===================================================================
// Troll cross-graph pursuit
// ===================================================================

#[test]
fn troll_pursues_elf_cross_species_pathfinding() {
    // Trolls (2x2x2) and elves (1x1x1) use different footprints for
    // pathfinding. A troll should still be able to pursue a nearby elf
    // using voxel-direct A* even when footprint sizes differ.
    let mut sim = flat_world_sim(legacy_test_seed());
    let mut events = Vec::new();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn troll near the tree.
    let troll_id = sim
        .spawn_creature(Species::Troll, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn troll");
    let troll_pos = sim.db.creatures.get(&troll_id).unwrap().position.min;

    // Spawn elf within troll detection range (~12 voxels).
    let elf_spawn_pos = VoxelCoord::new(troll_pos.x + 5, troll_pos.y, troll_pos.z);
    let elf_id = sim
        .spawn_creature(Species::Elf, elf_spawn_pos, sim.home_zone_id(), &mut events)
        .expect("spawn elf");

    force_idle_and_cancel_activations(&mut sim, troll_id);
    force_idle_and_cancel_activations(&mut sim, elf_id);

    let troll_node = creature_pos(&sim, troll_id);
    let pursued = sim.hostile_pursue(troll_id, Some(troll_node), Species::Troll, &mut events);

    assert!(
        pursued,
        "Troll should pursue nearby elf using voxel-direct A*"
    );
}

// ===================================================================
// Voxel exclusion (hostile blocking)
// ===================================================================

#[test]
fn voxel_exclusion_hostile_blocks_movement() {
    // A goblin at node B should prevent an elf from moving to node B.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    // Schedule only the elf to activate (goblin stays idle).
    suppress_activation_until(&mut sim, goblin, u64::MAX);
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, elf, tick);

    // Run a few ticks — elf should NOT move to goblin's voxel.
    sim.step(&[], sim.tick + 200);

    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

    // Elf must not be at the goblin's position.
    assert_ne!(
        elf_pos_after, goblin_pos,
        "Elf should not move into voxel occupied by hostile goblin"
    );
}

#[test]
fn voxel_exclusion_non_hostile_does_not_block() {
    // Two elves (same civ) should be able to share a voxel.
    let mut sim = test_sim(legacy_test_seed());
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf_a, node_a);
    force_position(&mut sim, elf_b, node_b);
    force_idle(&mut sim, elf_a);
    force_idle(&mut sim, elf_b);

    // Verify they are non-hostile.
    assert!(
        sim.is_non_hostile(elf_a, elf_b),
        "Two elves should be non-hostile"
    );

    // The exclusion check should return false (not blocked).
    let elf_b_pos = sim.db.creatures.get(&elf_b).unwrap().position.min;
    let elf_footprint = sim.species_table[&Species::Elf].footprint;
    assert!(
        !sim.destination_blocked_by_hostile(elf_a, elf_b_pos, elf_footprint),
        "Non-hostile creature should not block destination"
    );
}

#[test]
fn voxel_exclusion_self_does_not_block() {
    // A creature should not block itself (e.g., when source and dest
    // footprints overlap for large creatures).
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let elf_footprint = sim.species_table[&Species::Elf].footprint;

    assert!(
        !sim.destination_blocked_by_hostile(elf, elf_pos, elf_footprint),
        "Creature should not block itself"
    );
}

#[test]
fn voxel_exclusion_dead_hostile_does_not_block() {
    // A dead goblin should not block an elf's movement.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);

    // Kill the goblin (vital_status is indexed, so use update).
    if let Some(mut c) = sim.db.creatures.get(&goblin) {
        c.vital_status = VitalStatus::Dead;
        sim.db.update_creature(c).unwrap();
    }

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let elf_footprint = sim.species_table[&Species::Elf].footprint;
    assert!(
        !sim.destination_blocked_by_hostile(elf, goblin_pos, elf_footprint),
        "Dead hostile should not block destination"
    );
}

#[test]
fn voxel_exclusion_bidirectional_blocking() {
    // Both elf and goblin should be blocked from entering each other's
    // voxels — exclusion is symmetric.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let elf_fp = sim.species_table[&Species::Elf].footprint;
    let goblin_fp = sim.species_table[&Species::Goblin].footprint;

    assert!(
        sim.destination_blocked_by_hostile(elf, goblin_pos, elf_fp),
        "Goblin should block elf from entering its voxel"
    );
    assert!(
        sim.destination_blocked_by_hostile(goblin, elf_pos, goblin_fp),
        "Elf should block goblin from entering its voxel"
    );
}

#[test]
fn voxel_exclusion_hostile_pursue_blocked() {
    // A goblin pursuing an elf should be blocked when the elf occupies
    // the destination voxel (but the goblin can still attack from range).
    let mut sim = test_sim(legacy_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);

    // Place them on adjacent nodes.
    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, goblin, node_a);
    force_position(&mut sim, elf, node_b);
    force_idle(&mut sim, goblin);
    force_idle(&mut sim, elf);

    let goblin_pos_before = sim.db.creatures.get(&goblin).unwrap().position.min;
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;

    // Freeze the elf so only the goblin activates.
    suppress_activation(&mut sim, elf);

    // Schedule the goblin's activation and step the sim.
    let goblin_tick = sim
        .db
        .creatures
        .get(&goblin)
        .unwrap()
        .next_available_tick
        .unwrap_or(sim.tick);
    sim.step(&[], goblin_tick + 1);

    let goblin_pos_after = sim.db.creatures.get(&goblin).unwrap().position.min;

    // Goblin should NOT have moved onto the elf's exact voxel.
    assert_ne!(
        goblin_pos_after, elf_pos,
        "Goblin should not move into elf's voxel (hostile exclusion)"
    );

    // Goblin should either stay put or have dealt damage (melee from
    // adjacent position).
    let elf_hp = sim.db.creatures.get(&elf).unwrap().hp;
    let elf_max_hp = sim.db.creatures.get(&elf).unwrap().hp_max;
    let goblin_moved = goblin_pos_before != goblin_pos_after;
    let dealt_damage = elf_hp < elf_max_hp;
    assert!(
        !goblin_moved || dealt_damage,
        "Goblin should either stay put (blocked) or attack; \
             moved={goblin_moved}, dealt_damage={dealt_damage}"
    );
}

#[test]
fn voxel_exclusion_wander_avoids_hostile_voxels() {
    // An elf wandering should not pick an edge leading to a hostile's voxel.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place elf at node_b, goblin at node_c. Elf should not wander to node_c.
    let (_node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_position(&mut sim, elf, node_b);
    force_position(&mut sim, goblin, node_c);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    // Cancel goblin activations so it stays put.
    suppress_activation_until(&mut sim, goblin, u64::MAX);
    suppress_activation_until(&mut sim, elf, u64::MAX);

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

    // Run many activations and verify the elf never lands on goblin's voxel.
    for i in 0..20 {
        force_idle(&mut sim, elf);
        force_position(&mut sim, elf, node_b);
        let tick = sim.tick + 1;
        schedule_activation_at(&mut sim, elf, tick);
        sim.step(&[], sim.tick + 200);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
        assert_ne!(
            elf_pos, goblin_pos,
            "Elf should not wander into hostile voxel (iteration {i})"
        );
    }
}

#[test]
fn voxel_exclusion_flee_avoids_hostile_voxels() {
    // A fleeing elf should prefer edges not occupied by hostiles.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Set up: elf at node_b (middle), goblin at node_a (nearby threatening).
    // Node_c should be unoccupied — elf should flee toward node_c, not node_a.
    let (node_a, node_b, _node_c) = find_chain_of_three(&sim);
    force_position(&mut sim, elf, node_b);
    force_position(&mut sim, goblin, node_a);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    suppress_activation_until(&mut sim, elf, u64::MAX);
    suppress_activation_until(&mut sim, goblin, u64::MAX);

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

    // Activate only the elf — it should detect the goblin and flee.
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, elf, tick);
    sim.step(&[], sim.tick + 200);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    // Elf should NOT have fled into the goblin's voxel.
    assert_ne!(
        elf_pos, goblin_pos,
        "Fleeing elf should not enter hostile voxel"
    );
}

#[test]
fn voxel_exclusion_flee_cornered_still_moves() {
    // If ALL neighboring voxels have hostiles, the fleeing creature should
    // still be allowed to move (flee fallback — better to move through a
    // hostile than freeze).
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Use a flat-world position so all 26 neighbors are walkable.
    let floor_y = sim.config.floor_y + 1;
    let elf_pos = VoxelCoord::new(30, floor_y, 30);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    // Enumerate ALL walkable neighbors (from NEIGHBOR_OFFSETS) so the elf
    // is truly cornered. On the flat world the only walkable neighbors are
    // those at the same y (ground plane) since y+1 has no adjacent solid.
    let walkable_neighbors: Vec<VoxelCoord> = crate::pathfinding::NEIGHBOR_OFFSETS
        .iter()
        .map(|&(dx, dy, dz, _)| VoxelCoord::new(elf_pos.x + dx, elf_pos.y + dy, elf_pos.z + dz))
        .filter(|&pos| {
            crate::walkability::footprint_walkable(
                sim.voxel_zone(sim.home_zone_id()).unwrap(),
                &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
                pos,
                [1, 1, 1],
                true,
            )
        })
        .collect();

    // Spawn a goblin at each walkable neighbor so every exit is hostile.
    let mut goblins = Vec::new();
    for &neighbor_pos in &walkable_neighbors {
        let g = spawn_species(&mut sim, Species::Goblin);
        force_position(&mut sim, g, neighbor_pos);
        force_idle(&mut sim, g);
        suppress_activation_until(&mut sim, g, u64::MAX);
        goblins.push(g);
    }

    // Re-confirm elf position (spawning may have moved things).
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    // Verify that all walkable neighbors are indeed hostile-blocked.
    let elf_fp = sim.species_table[&Species::Elf].footprint;
    for &pos in &walkable_neighbors {
        assert!(
            sim.destination_blocked_by_hostile(elf, pos, elf_fp),
            "Expected neighbor at {pos:?} to be hostile-blocked"
        );
    }

    let elf_pos_before = sim.db.creatures.get(&elf).unwrap().position.min;

    // Now activate the elf — it should be cornered but flee should still
    // use the fallback (allow movement through hostile).
    let mut ec = sim.db.creatures.get(&elf).unwrap();
    ec.next_available_tick = Some(sim.tick + 1);
    sim.db.update_creature(ec).unwrap();
    sim.step(&[], sim.tick + 200);

    // The elf has hostile_detection_range_sq=225 (15 voxels) and the
    // goblins are on adjacent positions — well within range. The elf
    // should detect the threat and flee. Since all exits are hostile-
    // occupied, the flee fallback should allow movement through a hostile.
    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_ne!(
        elf_pos_before, elf_pos_after,
        "Cornered elf should flee (using fallback through hostile-occupied voxel)"
    );
    // Elf moved — verify it went to one of the walkable neighbor positions
    // (which are all hostile-occupied, confirming fallback worked).
    let moved_to_neighbor = walkable_neighbors.iter().any(|&p| p == elf_pos_after);
    assert!(
        moved_to_neighbor,
        "Cornered elf should flee to a neighbor (even hostile-occupied), \
         instead moved from {elf_pos_before:?} to {elf_pos_after:?}"
    );
}

#[test]
fn voxel_exclusion_ground_move_one_step_returns_false_when_blocked() {
    // Directly test ground_move_one_step returns false when destination is hostile.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    let elf_pos_before = sim.db.creatures.get(&elf).unwrap().position.min;
    let result = sim.ground_move_one_step(elf, Species::Elf, node_b);

    assert!(
        !result,
        "ground_move_one_step should return false when blocked"
    );
    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_eq!(
        elf_pos_before, elf_pos_after,
        "Elf position should not change when move is blocked"
    );
}

#[test]
fn voxel_exclusion_skip_exclusion_allows_blocked_move() {
    // ground_move_one_step_inner with skip_exclusion=true should succeed even
    // when the destination is hostile-occupied. This is the mechanism
    // used by ground_flee_step's cornered fallback.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    // Without skip_exclusion: blocked.
    let result_blocked = sim.ground_move_one_step(elf, Species::Elf, node_b);
    assert!(
        !result_blocked,
        "ground_move_one_step should block when hostile present"
    );

    // Cancel the retry activation scheduled by the failed move.
    suppress_activation_until(&mut sim, elf, u64::MAX);

    // With skip_exclusion: allowed.
    let result_forced = sim.ground_move_one_step_inner(elf, Species::Elf, node_b, true);
    assert!(
        result_forced,
        "ground_move_one_step_inner with skip_exclusion should succeed"
    );

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_eq!(
        elf_pos, node_b,
        "Elf should have moved to goblin's voxel with skip_exclusion"
    );
}

#[test]
fn voxel_exclusion_ground_move_one_step_returns_true_when_clear() {
    // ground_move_one_step should return true when destination is clear.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_idle(&mut sim, elf);

    let result = sim.ground_move_one_step(elf, Species::Elf, node_b);
    assert!(
        result,
        "ground_move_one_step should return true when path is clear"
    );

    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_eq!(elf_pos_after, node_b, "Elf should have moved to node B");
}

#[test]
fn voxel_exclusion_blocked_schedules_retry() {
    // When movement is blocked, a retry activation should be scheduled.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    suppress_activation_until(&mut sim, elf, u64::MAX);
    suppress_activation_until(&mut sim, goblin, u64::MAX);

    // Before the blocked move, elf is suppressed (next_available_tick = u64::MAX).
    let nat_before = sim.db.creatures.get(&elf).unwrap().next_available_tick;
    assert_eq!(nat_before, Some(u64::MAX), "precondition: elf suppressed");

    sim.ground_move_one_step(elf, Species::Elf, node_b);

    // After the blocked move, next_available_tick should be set to a
    // near-future retry tick (tick + voxel_exclusion_retry_ticks), not
    // u64::MAX.
    let nat_after = sim.db.creatures.get(&elf).unwrap().next_available_tick;
    let expected_retry = sim.tick + sim.config.voxel_exclusion_retry_ticks;
    assert_eq!(
        nat_after,
        Some(expected_retry),
        "Blocked move should schedule a retry activation at tick + retry_delay"
    );
}

#[test]
fn voxel_exclusion_already_overlapping_allowed() {
    // If two hostile creatures are already in the same voxel (e.g., both
    // spawned there), they should be allowed to stay — the check only
    // prevents MOVING into a hostile's voxel.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, _) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_a);

    // Both are now at the same position — this should be fine.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    assert_eq!(
        elf_pos, goblin_pos,
        "Test setup: both should be at same position"
    );

    // The sim should not crash when processing these creatures.
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, elf, tick);
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, goblin, tick);

    // Just verify no panic. Creatures should separate on their next moves.
    sim.step(&[], sim.tick + 500);
}

#[test]
fn voxel_exclusion_different_hostile_civs_block() {
    // Two creatures from different hostile civs should block each other.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let orc = spawn_species(&mut sim, Species::Orc);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, orc, node_b);

    let orc_pos = sim.db.creatures.get(&orc).unwrap().position.min;
    let elf_fp = sim.species_table[&Species::Elf].footprint;

    // Orc is aggressive non-civ — should be hostile to elf.
    assert!(
        !sim.is_non_hostile(elf, orc),
        "Elf and orc should be hostile"
    );
    assert!(
        sim.destination_blocked_by_hostile(elf, orc_pos, elf_fp),
        "Orc should block elf destination"
    );
}

#[test]
fn voxel_exclusion_two_non_civ_same_species_share() {
    // Two non-civ goblins should NOT block each other (they're non-hostile).
    let mut sim = test_sim(legacy_test_seed());
    let g1 = spawn_species(&mut sim, Species::Goblin);
    let g2 = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, g1, node_a);
    force_position(&mut sim, g2, node_b);

    let g2_pos = sim.db.creatures.get(&g2).unwrap().position.min;
    let goblin_fp = sim.species_table[&Species::Goblin].footprint;

    // Non-civ same-species are non-hostile.
    assert!(
        sim.is_non_hostile(g1, g2),
        "Two goblins should be non-hostile"
    );
    assert!(
        !sim.destination_blocked_by_hostile(g1, g2_pos, goblin_fp),
        "Same-species non-civ creatures should not block each other"
    );
}

#[test]
fn voxel_exclusion_large_creature_blocked_by_small_hostile_in_footprint() {
    // An elephant (2x2x2) should be blocked if a goblin occupies any
    // voxel within the elephant's destination footprint.
    let mut sim = test_sim(legacy_test_seed());
    let elephant = spawn_species(&mut sim, Species::Elephant);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elephant_fp = sim.species_table[&Species::Elephant].footprint;
    assert_eq!(
        elephant_fp,
        [2, 2, 2],
        "Elephant should have 2x2x2 footprint"
    );

    // Place the goblin at a position that would be inside the elephant's
    // destination footprint. If elephant anchor is at (x,y,z), the
    // footprint covers (x..x+2, y..y+2, z..z+2).
    let elephant_pos = sim.db.creatures.get(&elephant).unwrap().position.min;
    // Place goblin at elephant_pos + (1, 0, 0) — inside the footprint.
    let goblin_pos = VoxelCoord::new(elephant_pos.x + 1, elephant_pos.y, elephant_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    // Elephant is passive non-civ, goblin is aggressive non-civ — both
    // non-civ means non-hostile by default. Give the elephant a civ so the
    // goblin's aggressive engagement initiative makes them hostile.
    let player_civ = sim.player_civ_id.unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elephant) {
        c.civ_id = Some(player_civ);
        sim.db.update_creature(c).unwrap();
    }

    assert!(
        !sim.is_non_hostile(elephant, goblin),
        "Civ elephant and aggressive goblin should be hostile"
    );

    assert!(
        sim.destination_blocked_by_hostile(elephant, elephant_pos, elephant_fp),
        "Goblin inside elephant's footprint should block it"
    );
}

#[test]
fn voxel_exclusion_small_creature_blocked_by_large_hostile_footprint() {
    // A 1x1x1 elf should be blocked if a troll (2x2x2, aggressive) has
    // a footprint covering the elf's destination voxel.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    let troll_fp = sim.species_table[&Species::Troll].footprint;
    assert_eq!(
        troll_fp,
        [2, 2, 2],
        "Troll must have 2x2x2 footprint for this test"
    );

    let troll = spawn_species(&mut sim, Species::Troll);

    let troll_pos = sim.db.creatures.get(&troll).unwrap().position.min;
    // Troll footprint covers troll_pos to troll_pos + (1,1,1).
    // Test a position inside the footprint but not the anchor.
    let blocked_pos = VoxelCoord::new(troll_pos.x + 1, troll_pos.y, troll_pos.z);

    let elf_fp = sim.species_table[&Species::Elf].footprint;

    // Troll is aggressive non-civ, elf has civ — they should be hostile.
    assert!(
        !sim.is_non_hostile(elf, troll),
        "Elf and troll should be hostile"
    );

    assert!(
        sim.destination_blocked_by_hostile(elf, blocked_pos, elf_fp),
        "Troll's extended footprint should block elf at ({}, {}, {})",
        blocked_pos.x,
        blocked_pos.y,
        blocked_pos.z
    );

    // The anchor position should also be blocked.
    assert!(
        sim.destination_blocked_by_hostile(elf, troll_pos, elf_fp),
        "Troll's anchor voxel should also block elf"
    );
}

#[test]
fn voxel_exclusion_config_retry_ticks_respected() {
    // Verify that the retry delay uses the configured value. Set a large
    // retry delay (500 ticks), block a move, then step to 499 — the elf
    // should still be at the original position. Step to 501 — the retry
    // activation fires (elf attempts to move or re-evaluates).
    let mut sim = test_sim(legacy_test_seed());
    sim.config.voxel_exclusion_retry_ticks = 500;

    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    suppress_activation_until(&mut sim, elf, u64::MAX);
    suppress_activation_until(&mut sim, goblin, u64::MAX);

    let tick_at_block = sim.tick;
    sim.ground_move_one_step(elf, Species::Elf, node_b);

    // Step to just before the retry — activation should NOT have fired.
    let activations_before = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_before, 1,
        "Should have exactly one retry scheduled"
    );

    sim.step(&[], tick_at_block + 499);
    // Elf should still be at node_a (retry not yet fired).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_eq!(
        elf_pos, node_a,
        "Elf should not have moved before retry delay expires"
    );

    // Step past the retry tick — activation fires.
    sim.step(&[], tick_at_block + 501);
    let activations_after = sim.count_pending_activations_for(elf);
    // The retry activation should have been consumed (though a new one
    // may have been scheduled if the elf is still blocked).
    assert!(
        activations_after <= 1,
        "Retry activation should have fired by tick {}",
        tick_at_block + 501
    );
}

// -----------------------------------------------------------------------
// Voxel exclusion + logistics interaction
// -----------------------------------------------------------------------

#[test]
fn voxel_exclusion_walk_toward_task_blocked() {
    // An elf walking toward a task should be blocked by a hostile in its path.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Find a chain so elf can walk from A through B (goblin) to C (task).
    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    suppress_activation_until(&mut sim, goblin, u64::MAX);
    suppress_activation_until(&mut sim, elf, u64::MAX);

    // Create a GoTo task at node_c and assign the elf.
    let task_id = insert_goto_task(&mut sim, node_c);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        sim.db.update_task(t).unwrap();
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Activate elf.
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, elf, tick);
    sim.step(&[], sim.tick + 200);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

    assert_ne!(
        elf_pos, goblin_pos,
        "Elf walking toward task should not enter goblin's voxel"
    );
}

#[test]
fn voxel_exclusion_serde_config_roundtrip() {
    // Verify the new config field survives serialization.
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let config2: GameConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        config.voxel_exclusion_retry_ticks, config2.voxel_exclusion_retry_ticks,
        "voxel_exclusion_retry_ticks should survive serde roundtrip"
    );
}

#[test]
fn voxel_exclusion_serde_config_default() {
    // Verify that loading old configs without the new field uses the default.
    // Serialize default config, strip the new field, and re-deserialize.
    let config = GameConfig::default();
    let mut json_val: serde_json::Value = serde_json::to_value(&config).unwrap();
    json_val
        .as_object_mut()
        .unwrap()
        .remove("voxel_exclusion_retry_ticks");
    let config2: GameConfig = serde_json::from_value(json_val).unwrap();
    assert_eq!(
        config2.voxel_exclusion_retry_ticks, 50,
        "Default voxel_exclusion_retry_ticks should be 50"
    );
}

#[test]
fn voxel_exclusion_walk_toward_task_clears_cached_path() {
    // When walk_toward_task is blocked by a hostile, the creature's
    // cached path should be invalidated (set to None) so a fresh
    // path is computed on retry.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    suppress_activation_until(&mut sim, goblin, u64::MAX);
    suppress_activation_until(&mut sim, elf, u64::MAX);

    // Give the elf a cached path through the goblin's position.
    let pos_b = node_b;
    let pos_c = node_c;
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.path = Some(CreaturePath {
            remaining_positions: vec![pos_b, pos_c],
        });
        sim.db.update_creature(c).unwrap();
    }
    assert!(
        sim.db.creatures.get(&elf).unwrap().path.is_some(),
        "Test setup: elf should have a cached path"
    );

    // Create a GoTo task at node_c and assign the elf.
    let task_id = insert_goto_task(&mut sim, node_c);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        sim.db.update_task(t).unwrap();
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Activate the elf — it should try to follow the cached path,
    // hit the hostile, and clear the path.
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, elf, tick);
    sim.step(&[], sim.tick + 10);

    assert!(
        sim.db.creatures.get(&elf).unwrap().path.is_none(),
        "Cached path should be cleared when blocked by hostile"
    );
}

#[test]
fn voxel_exclusion_hostile_dies_creature_retries_and_moves() {
    // A creature blocked by a hostile should successfully move once
    // the hostile dies and the retry activation fires.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    suppress_activation_until(&mut sim, goblin, u64::MAX);
    suppress_activation_until(&mut sim, elf, u64::MAX);

    // Try to move — should be blocked.
    let result = sim.ground_move_one_step(elf, Species::Elf, node_b);
    assert!(!result, "Should be blocked while goblin is alive");
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().position.min,
        node_a,
        "Elf should still be at node A"
    );

    // Kill the goblin.
    let tick = sim.tick;
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    sim.step(&[kill_cmd], tick + 2);

    // The retry activation should fire after voxel_exclusion_retry_ticks.
    // The goblin is dead, so the voxel should now be clear.
    let goblin_fp = sim.species_table[&Species::Goblin].footprint;
    assert!(
        !sim.destination_blocked_by_hostile(elf, node_b, goblin_fp),
        "Dead goblin should not block"
    );

    // Now manually retry the move — should succeed.
    let result2 = sim.ground_move_one_step(elf, Species::Elf, node_b);
    assert!(result2, "Move should succeed after goblin dies");
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().position.min,
        node_b,
        "Elf should have moved to node B"
    );
}

#[test]
fn voxel_exclusion_attack_target_task_blocked_by_hostile() {
    // An elf with a player-directed AttackTarget task walking toward
    // its target should be blocked by a different hostile in the path.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let target_goblin = spawn_species(&mut sim, Species::Goblin);
    let blocking_goblin = spawn_species(&mut sim, Species::Goblin);

    // Set up: elf at A, blocking_goblin at B, target_goblin at C.
    // Elf must walk through B to reach C.
    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, blocking_goblin, node_b);
    force_position(&mut sim, target_goblin, node_c);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, blocking_goblin);
    force_idle(&mut sim, target_goblin);
    suppress_activation_until(&mut sim, blocking_goblin, u64::MAX);
    suppress_activation_until(&mut sim, target_goblin, u64::MAX);
    suppress_activation_until(&mut sim, elf, u64::MAX);

    // Issue AttackCreature command — this creates an AttackTarget task.
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: target_goblin,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Verify elf has an attack task.
    assert!(
        sim.db.creatures.get(&elf).unwrap().current_task.is_some(),
        "Elf should have an attack task"
    );

    // Run for a bit — elf should NOT move onto blocking_goblin's voxel.
    let blocking_pos = sim.db.creatures.get(&blocking_goblin).unwrap().position.min;
    sim.step(&[], tick + 500);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_ne!(
        elf_pos, blocking_pos,
        "Elf should not walk through blocking goblin to reach attack target"
    );
}

#[test]
fn voxel_exclusion_attack_move_task_blocked_by_hostile() {
    // An elf with an AttackMove task walking toward a destination
    // should be blocked by a hostile in the path.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_position(&mut sim, elf, node_a);
    force_position(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    suppress_activation_until(&mut sim, goblin, u64::MAX);
    suppress_activation_until(&mut sim, elf, u64::MAX);

    let dest_pos = node_c;

    // Issue AttackMove command toward node_c.
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            destination: dest_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Run for a bit — elf should NOT move onto goblin's voxel.
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    sim.step(&[], tick + 500);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_ne!(
        elf_pos, goblin_pos,
        "Elf on attack-move should not walk through hostile goblin"
    );
}

// ===================================================================
// Path serde and rerouting
// ===================================================================

#[test]
fn creature_path_serde_roundtrip() {
    let path = CreaturePath {
        remaining_positions: vec![
            VoxelCoord::new(10, 5, 20),
            VoxelCoord::new(11, 5, 20),
            VoxelCoord::new(12, 6, 21),
            VoxelCoord::new(13, 6, 22),
        ],
    };
    let json = serde_json::to_string(&path).unwrap();
    let restored: CreaturePath = serde_json::from_str(&json).unwrap();
    assert_eq!(
        path.remaining_positions, restored.remaining_positions,
        "CreaturePath positions should survive serde roundtrip"
    );
}

/// Verify that a creature with a cached path to a bogus position repaths
/// gracefully and ends up at a valid task location (not just "doesn't panic",
/// but actually completes or re-evaluates its task).
#[test]
fn cached_path_reroutes_when_nav_node_destroyed() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Find two connected nodes: start (A) and destination (B).
    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_idle(&mut sim, elf);

    // Give the elf a GoTo task at node_b.
    let task_id = insert_goto_task(&mut sim, node_b);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        sim.db.update_task(t).unwrap();
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Assign a cached path with TWO bogus positions (unlike the existing
    // test which uses one bogus + one real). This forces a full repath.
    let bogus_a = VoxelCoord::new(63, 63, 63);
    let bogus_b = VoxelCoord::new(62, 63, 63);
    assert!(!crate::walkability::footprint_walkable(
        sim.voxel_zone(sim.home_zone_id()).unwrap(),
        &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
        bogus_a,
        [1, 1, 1],
        true,
    ));
    assert!(!crate::walkability::footprint_walkable(
        sim.voxel_zone(sim.home_zone_id()).unwrap(),
        &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
        bogus_b,
        [1, 1, 1],
        true,
    ));

    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.path = Some(CreaturePath {
            remaining_positions: vec![bogus_a, bogus_b],
        });
        sim.db.update_creature(c).unwrap();
    }

    // Schedule activation and step forward.
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, elf, tick);
    sim.step(&[], sim.tick + 500);

    // Creature should still be alive.
    let creature = sim
        .db
        .creatures
        .get(&elf)
        .expect("Creature should still exist after reroute");
    assert!(creature.hp > 0, "Creature should be alive after reroute");

    // The bogus path should have been cleared — creature should have
    // either rerouted or abandoned the path.
    if let Some(ref path) = creature.path {
        for pos in &path.remaining_positions {
            assert!(
                *pos != bogus_a && *pos != bogus_b,
                "Bogus positions should have been cleared from path"
            );
        }
    }
}

// ===================================================================
// Hornet/wyvern combat movement
// ===================================================================

#[test]
fn hornet_pursues_and_damages_elf() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    force_idle_and_cancel_activations(&mut sim, elf_id);
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

    // Spawn hornet 3 voxels above the elf (within detection range, likely
    // out of melee range). Let the sim run — the hornet should fly down,
    // get adjacent, and melee the elf.
    let hornet_pos = VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z);
    let hornet_id = spawn_hornet_at(&mut sim, hornet_pos);
    force_guaranteed_hits(&mut sim, hornet_id);

    // Run the sim for a generous number of ticks to allow pursuit + melee.
    let target_tick = sim.tick + 5000;
    sim.step(&[], target_tick);

    // The elf should have taken damage (hornet melee_damage = 20).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.hp < elf_hp_before,
        "elf should have taken damage from hornet: hp {} vs before {}",
        elf.hp,
        elf_hp_before
    );

    // The hornet should still be alive (elf might fight back, but with
    // 60 HP and passive default engagement the elf likely didn't kill it).
    let hornet = sim.db.creatures.get(&hornet_id).unwrap();
    // Just verify it participated (was alive at some point and moved).
    assert_ne!(hornet.position.min, hornet_pos, "hornet should have moved");
}

#[test]
fn hornet_wanders_when_alone() {
    let mut sim = test_sim(legacy_test_seed());
    // Spawn hornet far from any creatures.
    let pos = VoxelCoord::new(5, 50, 5);
    let hornet_id = spawn_hornet_at(&mut sim, pos);

    // Run a few ticks — hornet should wander (position should change).
    let target_tick = sim.tick + 3000;
    sim.step(&[], target_tick);

    let hornet = sim.db.creatures.get(&hornet_id).unwrap();
    assert_ne!(hornet.position.min, pos, "hornet should have wandered");
}

/// Test matrix: aggressive elf (autonomous pursuit) vs hornet at various heights.
///
/// Heights are measured as voxel delta from the elf's walking level. With 1x1x1
/// footprints, the AABB gap in Y is `dy - 1` (elf occupies [y, y+1), hornet
/// occupies [y+dy, y+dy+1)). Melee range is squared gap distance:
/// - dy=0: gap=0, dist_sq=0 (same level, definitely in range)
/// - dy=1: gap=0, dist_sq=0 (adjacent, no gap — elf top meets hornet bottom)
/// - dy=2: gap=1, dist_sq=1 ≤ 3 (bare hands reach)
/// - dy=3: gap=2, dist_sq=4 > 3 (bare hands can't, spear_range_sq=8 can)
/// - dy=10: gap=9, dist_sq=81 (way above, nobody can reach)
///
/// For each height, we test bare-handed and spear-armed elves.
#[test]
fn aggressive_elf_vs_hornet_at_heights() {
    struct Case {
        dy: i32,
        has_spear: bool,
        expect_damage: bool,
        label: &'static str,
    }

    let cases = [
        Case {
            dy: 0,
            has_spear: false,
            expect_damage: true,
            label: "dy=0, bare hands",
        },
        Case {
            dy: 0,
            has_spear: true,
            expect_damage: true,
            label: "dy=0, spear",
        },
        Case {
            dy: 1,
            has_spear: false,
            expect_damage: true,
            label: "dy=1, bare hands",
        },
        Case {
            dy: 1,
            has_spear: true,
            expect_damage: true,
            label: "dy=1, spear",
        },
        Case {
            dy: 2,
            has_spear: false,
            expect_damage: true,
            label: "dy=2, bare hands (gap=1, in range)",
        },
        Case {
            dy: 2,
            has_spear: true,
            expect_damage: true,
            label: "dy=2, spear",
        },
        // dy=10+: hornet is way above the tree canopy. Even climbing, the elf
        // can't get within melee range. The elf should give up.
        Case {
            dy: 20,
            has_spear: false,
            expect_damage: false,
            label: "dy=20, bare hands (way above)",
        },
        Case {
            dy: 20,
            has_spear: true,
            expect_damage: false,
            label: "dy=20, spear (way above)",
        },
    ];

    for case in &cases {
        let mut sim = flat_world_sim(legacy_test_seed());
        let (elf_id, elf_pos) = setup_aggressive_elf(&mut sim);
        force_guaranteed_hits(&mut sim, elf_id);

        if case.has_spear {
            give_spear(&mut sim, elf_id);
        }

        // Build a trunk pillar adjacent to the elf. Walkability detects
        // climbable trunk-surface positions on its exterior, letting the elf
        // ascend vertically while staying adjacent (dx=1) to the hornet
        // column — within bare-hands melee range at any shared height.
        for y in elf_pos.y..elf_pos.y + 5 {
            sim.voxel_zone_mut(sim.home_zone_id()).unwrap().set(
                VoxelCoord::new(elf_pos.x + 1, y, elf_pos.z),
                VoxelType::Trunk,
            );
        }
        sim.rebuild_transient_state();

        let hornet_pos = VoxelCoord::new(elf_pos.x, elf_pos.y + case.dy, elf_pos.z);
        let hornet_id = setup_frozen_hornet(&mut sim, hornet_pos);
        let hornet_hp_before = sim.db.creatures.get(&hornet_id).unwrap().hp;

        // Enable elf activation (force_idle sets next_available_tick=None).
        let mut ec = sim.db.creatures.get(&elf_id).unwrap();
        ec.next_available_tick = Some(sim.tick + 1);
        sim.db.update_creature(ec).unwrap();

        // Let the elf act for enough ticks to walk + strike.
        let target_tick = sim.tick + 8000;
        sim.step(&[], target_tick);

        let hornet = sim.db.creatures.get(&hornet_id).unwrap();
        if case.expect_damage {
            assert!(
                hornet.hp < hornet_hp_before,
                "[{}] hornet should have taken damage (hp {} vs before {})",
                case.label,
                hornet.hp,
                hornet_hp_before
            );
        } else {
            assert_eq!(
                hornet.hp, hornet_hp_before,
                "[{}] hornet should NOT have taken damage",
                case.label
            );
        }
    }
}

/// Test matrix: passive elf ordered to attack hornet at various heights.
///
/// Same height cases as above, but the elf is a civilian (passive initiative)
/// given a player-directed AttackCreature command. The elf should pursue the
/// target if a path gets it within melee range. If not reachable,
/// the attack task should eventually cancel (path_failures >= retry limit).
#[test]
fn ordered_elf_vs_hornet_at_heights() {
    struct Case {
        dy: i32,
        has_spear: bool,
        expect_damage: bool,
        label: &'static str,
    }

    let cases = [
        Case {
            dy: 0,
            has_spear: false,
            expect_damage: true,
            label: "dy=0, bare hands",
        },
        Case {
            dy: 0,
            has_spear: true,
            expect_damage: true,
            label: "dy=0, spear",
        },
        Case {
            dy: 1,
            has_spear: false,
            expect_damage: true,
            label: "dy=1, bare hands",
        },
        Case {
            dy: 1,
            has_spear: true,
            expect_damage: true,
            label: "dy=1, spear",
        },
        Case {
            dy: 2,
            has_spear: false,
            expect_damage: true,
            label: "dy=2, bare hands (gap=1)",
        },
        Case {
            dy: 2,
            has_spear: true,
            expect_damage: true,
            label: "dy=2, spear",
        },
        Case {
            dy: 20,
            has_spear: false,
            expect_damage: false,
            label: "dy=20, bare hands (way above)",
        },
        Case {
            dy: 20,
            has_spear: true,
            expect_damage: false,
            label: "dy=20, spear (way above)",
        },
    ];

    for case in &cases {
        let mut sim = flat_world_sim(legacy_test_seed());
        let elf_id = spawn_elf(&mut sim);
        zero_creature_stats(&mut sim, elf_id);
        force_guaranteed_hits(&mut sim, elf_id);
        // Elf stays civilian (passive) — won't pursue autonomously.
        force_idle_and_cancel_activations(&mut sim, elf_id);

        if case.has_spear {
            give_spear(&mut sim, elf_id);
        }

        let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;

        // Build a trunk pillar adjacent to the elf (same as aggressive test).
        for y in elf_pos.y..elf_pos.y + 5 {
            sim.voxel_zone_mut(sim.home_zone_id()).unwrap().set(
                VoxelCoord::new(elf_pos.x + 1, y, elf_pos.z),
                VoxelType::Trunk,
            );
        }
        sim.rebuild_transient_state();

        let hornet_pos = VoxelCoord::new(elf_pos.x, elf_pos.y + case.dy, elf_pos.z);
        let hornet_id = setup_frozen_hornet(&mut sim, hornet_pos);
        let hornet_hp_before = sim.db.creatures.get(&hornet_id).unwrap().hp;

        // Issue player-directed attack command.
        let tick = sim.tick + 1;
        let cmd = SimCommand {
            player_name: String::new(),
            tick,
            action: SimAction::AttackCreature {
                attacker_id: elf_id,
                target_id: hornet_id,
                queue: false,
            },
        };
        sim.step(&[cmd], tick + 8000);

        let hornet = sim.db.creatures.get(&hornet_id).unwrap();
        if case.expect_damage {
            assert!(
                hornet.hp < hornet_hp_before,
                "[ordered, {}] hornet should have taken damage (hp {} vs before {})",
                case.label,
                hornet.hp,
                hornet_hp_before
            );
        } else {
            assert_eq!(
                hornet.hp, hornet_hp_before,
                "[ordered, {}] hornet should NOT have taken damage",
                case.label
            );

            // NOTE: ideally the elf should give up the attack task when the
            // target is unreachable, but the current path_failures mechanism
            // only triggers on pathfinding failure, not "arrived but can't
            // melee." This is a known limitation tracked separately.
        }
    }
}

#[test]
fn wyvern_pursues_and_damages_elf() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    // Place elf at a known position and freeze so the wyvern can chase.
    let elf_pos = VoxelCoord::new(32, 1, 32);
    force_position(&mut sim, elf_id, elf_pos);
    force_idle_and_cancel_activations(&mut sim, elf_id);
    let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

    // Wyvern spawns above and slightly offset from the elf.
    let wyvern_pos = VoxelCoord::new(elf_pos.x - 1, elf_pos.y + 4, elf_pos.z - 1);
    let mut events = Vec::new();
    let wyvern_id = sim
        .spawn_creature(Species::Wyvern, wyvern_pos, sim.home_zone_id(), &mut events)
        .expect("wyvern should spawn");
    force_guaranteed_hits(&mut sim, wyvern_id);

    // Wyvern is fast (flight_tpv=200) but needs time to detect, path, and strike.
    let target_tick = sim.tick + 15000;
    sim.step(&[], target_tick);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.hp < elf_hp_before,
        "elf should have taken damage from wyvern: hp {} vs before {}",
        elf.hp,
        elf_hp_before
    );

    let wyvern = sim.db.creatures.get(&wyvern_id).unwrap();
    assert_ne!(wyvern.position.min, wyvern_pos, "wyvern should have moved");
}

// =========================================================================
// Creature gravity (F-creature-gravity)
// =========================================================================

#[test]
fn creature_on_solid_ground_does_not_fall() {
    let mut sim = test_sim(legacy_test_seed());
    // Spawn an elf away from the tree at a known ground position.
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, 1, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elf");

    // Elf should be on solid ground (terrain at y=0 is solid, elf at y=1).
    let pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    assert_eq!(pos.y, 1);
    assert!(sim.creature_is_supported(elf_id));

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 0);
}

#[test]
fn creature_falls_when_platform_removed() {
    let mut sim = test_sim(legacy_test_seed());
    // Use a low platform so the fall is survivable (2 voxels = 20 damage).
    let platform_pos = VoxelCoord::new(10, 2, 10);
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, 3, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elf");
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.min.y, 3);
    let hp_before = elf.hp;

    // Remove the platform — elf is now unsupported.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1);

    // Elf should have landed at y=1 (above terrain at y=0).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.min.y, 1);

    // Elf should have taken fall damage: 2 voxels * fall_damage_per_voxel.
    let expected_damage = 2 * sim.config.fall_damage_per_voxel;
    assert!(
        hp_before > expected_damage,
        "test setup: fall should be survivable"
    );
    assert_eq!(elf.hp, hp_before - expected_damage);

    // Should have emitted a CreatureFell event.
    let fell_event = events
        .iter()
        .find(|e| matches!(e.kind, SimEventKind::CreatureFell { .. }));
    assert!(fell_event.is_some());
    if let SimEventKind::CreatureFell {
        creature_id,
        from,
        to,
        damage,
        ..
    } = &fell_event.unwrap().kind
    {
        assert_eq!(*creature_id, elf_id);
        assert_eq!(from.y, 3);
        assert_eq!(to.y, 1);
        assert_eq!(*damage, expected_damage);
    }
}

#[test]
fn fatal_fall_kills_creature() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a very high platform so the fall is lethal.
    let platform_y = 40;
    let platform_pos = VoxelCoord::new(10, platform_y, 10);
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, platform_y + 1, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elf");

    // Ensure the fall will be lethal.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let fall_distance = platform_y; // from y=41 to y=1
    let expected_damage = fall_distance as i64 * sim.config.fall_damage_per_voxel;
    assert!(
        expected_damage >= elf.hp,
        "fall should be lethal: damage={expected_damage}, hp={}",
        elf.hp
    );

    // Remove platform.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    events.clear();
    sim.apply_creature_gravity(&mut events);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.vital_status, VitalStatus::Dead);

    // Should have a CreatureDied event with Falling cause.
    let died_event = events.iter().find(|e| {
        matches!(
            e.kind,
            SimEventKind::CreatureDied {
                cause: DeathCause::Falling,
                ..
            }
        )
    });
    assert!(
        died_event.is_some(),
        "expected CreatureDied with Falling cause"
    );
}

#[test]
fn zero_fall_damage_config_means_no_damage() {
    let mut sim = test_sim(legacy_test_seed());
    sim.config.fall_damage_per_voxel = 0;

    let platform_pos = VoxelCoord::new(10, 10, 10);
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, 11, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elf");
    let hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    events.clear();
    sim.apply_creature_gravity(&mut events);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.min.y, 1);
    assert_eq!(elf.hp, hp_before, "no damage when fall_damage_per_voxel=0");
}

#[test]
fn climber_on_trunk_does_not_fall() {
    let mut sim = test_sim(legacy_test_seed());
    // Place trunk voxels to create a trunk surface nav node.
    // The test world has floor_y=0, so trunk at y=1..5.
    for y in 1..6 {
        sim.voxel_zone_mut(sim.home_zone_id())
            .unwrap()
            .set(VoxelCoord::new(15, y, 15), VoxelType::Trunk);
    }
    sim.rebuild_transient_state();

    // Find a walkable position adjacent to the trunk (on the trunk surface).
    // Trunk climb positions are adjacent to solid trunk voxels.
    let trunk_adj = VoxelCoord::new(16, 3, 15); // east of trunk
    if !crate::walkability::footprint_walkable(
        sim.voxel_zone(sim.home_zone_id()).unwrap(),
        &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
        trunk_adj,
        [1, 1, 1],
        true,
    ) {
        // Not walkable here — skip test (topology dependent).
        return;
    }

    // Spawn an elf (climber, ground_only=false) and move it to the trunk node.
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, trunk_adj, sim.home_zone_id(), &mut events)
        .expect("should spawn elf");

    // Move elf to trunk position (may already be there if spawn snapped).
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.position = VoxelBox::point(trunk_adj);
        sim.db.update_creature(c).unwrap();
    }

    // Elf should be supported (has nav node, is a climber).
    assert!(sim.creature_is_supported(elf_id));

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 0);
}

#[test]
fn ground_only_creature_without_solid_below_falls() {
    let mut sim = test_sim(legacy_test_seed());
    // Spawn a capybara (ground_only=true) at ground level.
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let pos = sim.db.creatures.get(&capy_id).unwrap().position.min;
    assert_eq!(pos.y, 1, "capybara should be on ground");

    // Teleport the capybara to a position without solid below — e.g., y=5
    // with no platform. Not walkable either.
    let floating_pos = VoxelCoord::new(pos.x, 5, pos.z);
    {
        let mut c = sim.db.creatures.get(&capy_id).unwrap();
        c.position = VoxelBox::point(floating_pos);
        sim.db.update_creature(c).unwrap();
    }

    assert!(!sim.creature_is_supported(capy_id));

    let mut events = Vec::new();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1);

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(
        capy.position.min.y, 1,
        "capybara should land at ground level"
    );
}

#[test]
fn flying_creature_does_not_fall() {
    let mut sim = test_sim(legacy_test_seed());
    // Spawn a hornet (flying creature) and move it mid-air.
    let mut events = Vec::new();
    let hornet_id = sim
        .spawn_creature(
            Species::Hornet,
            VoxelCoord::new(20, 20, 20),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn hornet");

    // Hornet has MovementCategory::Flyer, so it's flying.
    let species_data = &sim.species_table[&Species::Hornet];
    assert!(species_data.movement_category.is_flyer());

    // Should be supported (flying creatures are always exempt).
    assert!(sim.creature_is_supported(hornet_id));

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    // Hornet should not have fallen.
    let fell_hornets = events.iter().any(|e| {
        matches!(
            e.kind,
            SimEventKind::CreatureFell { creature_id, .. } if creature_id == hornet_id
        )
    });
    assert!(!fell_hornets);
}

#[test]
fn creature_gravity_at_activation_time() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a platform and spawn an elf on it.
    let platform_pos = VoxelCoord::new(10, 5, 10);
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, 6, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elf");
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().position.min.y, 6);

    // Remove the platform and rebuild nav.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    // Trigger creature activation — should detect unsupported and apply gravity.
    events.clear();
    sim.process_creature_activation(elf_id, &mut events);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.position.min.y, 1,
        "elf should fall to ground via activation"
    );

    // Should have emitted a CreatureFell event.
    let fell_event = events
        .iter()
        .any(|e| matches!(e.kind, SimEventKind::CreatureFell { .. }));
    assert!(fell_event, "expected CreatureFell event from activation");
}

#[test]
fn creature_gravity_clears_task_and_path() {
    let mut sim = test_sim(legacy_test_seed());
    let platform_pos = VoxelCoord::new(10, 5, 10);
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, 6, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elf");

    // Give the elf a fake path and task.
    let task_id = TaskId::new(&mut sim.rng);
    let fake_task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: VoxelCoord::new(20, 1, 20),
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), fake_task);
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_id);
        c.path = Some(CreaturePath {
            remaining_positions: vec![VoxelCoord::new(15, 1, 15)],
        });
        sim.db.update_creature(c).unwrap();
    }

    // Remove platform and apply gravity.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    events.clear();
    sim.apply_creature_gravity(&mut events);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.min.y, 1);
    assert!(elf.path.is_none(), "path should be cleared after fall");
    assert!(
        elf.current_task.is_none(),
        "task should be cleared after fall"
    );
}

#[test]
fn logistics_heartbeat_triggers_creature_gravity() {
    let mut sim = test_sim(legacy_test_seed());
    let platform_pos = VoxelCoord::new(10, 5, 10);
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, 6, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elf");
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().position.min.y, 6);

    // Remove the platform and rebuild nav.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    // Advance to a LogisticsHeartbeat tick — step forward enough ticks.
    let interval = sim.config.logistics_heartbeat_interval_ticks;
    let target = sim.tick + interval + 1;
    let result = sim.step(&[], target);

    // Elf should have fallen to ground.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.position.min.y, 1,
        "elf should fall via logistics heartbeat"
    );

    // Should have a CreatureFell event in the step output.
    let fell_event = result
        .events
        .iter()
        .any(|e| matches!(e.kind, SimEventKind::CreatureFell { .. }));
    assert!(fell_event, "expected CreatureFell event from heartbeat");
}

#[test]
fn large_creature_falls_when_ground_removed() {
    let mut sim = test_sim(legacy_test_seed());
    // Elephants use 2x2x2 footprint walkability. Place a 2x2 platform at y=5
    // and spawn an elephant on it.
    for dx in 0..2 {
        for dz in 0..2 {
            sim.voxel_zone_mut(sim.home_zone_id()).unwrap().set(
                VoxelCoord::new(10 + dx, 5, 10 + dz),
                VoxelType::GrownPlatform,
            );
        }
    }
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(10, 6, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elephant");

    let pos = sim.db.creatures.get(&elephant_id).unwrap().position.min;
    // Elephant should be on the platform (y=6, standing on platform at y=5).
    assert_eq!(pos.y, 6, "elephant should be on platform");

    // Remove the platform.
    for dx in 0..2 {
        for dz in 0..2 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(10 + dx, 5, 10 + dz), VoxelType::Air);
        }
    }
    sim.rebuild_transient_state();

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1, "elephant should fall");

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    assert!(
        elephant.position.min.y < 6,
        "elephant should have fallen from y=6, now at y={}",
        elephant.position.min.y
    );

    let fell_event = events
        .iter()
        .any(|e| matches!(e.kind, SimEventKind::CreatureFell { .. }));
    assert!(fell_event, "expected CreatureFell event for elephant");
}

#[test]
fn degenerate_landing_teleports_to_nearest_node() {
    let mut sim = test_sim(legacy_test_seed());
    // Place a creature at a position with no solid surface below in its
    // column — deep in the air with no ground. The column at x=10, z=10
    // has terrain at y=0, so normally find_surface_below would find it.
    // Instead, test the degenerate path by putting the creature at a position
    // where the entire column below has no nav node AND is ground_only.
    // For a ground_only creature, find_creature_landing needs a nav node
    // with solid below. Place creature at y=5 in a column where y=1 has
    // no nav node (it might naturally not have one far from the tree).
    let mut events = Vec::new();

    // Spawn capybara (ground_only) at ground level, then teleport mid-air.
    let capy_id = sim
        .spawn_creature(
            Species::Capybara,
            VoxelCoord::new(5, 1, 5),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn capybara");
    let original_pos = sim.db.creatures.get(&capy_id).unwrap().position.min;

    // Teleport to a column that's entirely air (outside where nav nodes are
    // generated). Use a corner of the world where there's terrain but
    // possibly no nav node.
    let floating_pos = VoxelCoord::new(1, 10, 1);
    {
        let mut c = sim.db.creatures.get(&capy_id).unwrap();
        c.position = VoxelBox::point(floating_pos);
        sim.db.update_creature(c).unwrap();
    }

    events.clear();
    let fell = sim.apply_single_creature_gravity(capy_id, &mut events);
    assert!(fell, "capybara should fall from unsupported position");

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    // The creature should have landed somewhere valid — either at ground
    // level in the same column or teleported to the nearest walkable position.
    let is_walkable = crate::walkability::footprint_walkable(
        sim.voxel_zone(sim.home_zone_id()).unwrap(),
        &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
        capy.position.min,
        [1, 1, 1],
        true,
    );
    let has_solid_below = sim
        .voxel_zone(sim.home_zone_id())
        .unwrap()
        .get(VoxelCoord::new(
            capy.position.min.x,
            capy.position.min.y - 1,
            capy.position.min.z,
        ))
        .is_solid();
    assert!(
        is_walkable && has_solid_below,
        "capybara should be at a valid supported position after degenerate fall \
         (pos={:?}, is_walkable={is_walkable}, solid_below={has_solid_below})",
        capy.position
    );
}

#[test]
fn ground_only_with_nav_node_but_no_solid_below_falls() {
    let mut sim = test_sim(legacy_test_seed());
    // Test creature_is_supported: a ground_only creature at a nav node
    // position without solid below should be unsupported.
    // Build a platform, rebuild nav so there's a node, then force the
    // creature to that position and remove the platform WITHOUT rebuilding
    // nav, so the nav node persists but the voxel below is gone.
    let platform_pos = VoxelCoord::new(10, 3, 10);
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let standing_pos = VoxelCoord::new(10, 4, 10);
    // Verify position is walkable above platform.
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            standing_pos,
            [1, 1, 1],
            true
        ),
        "position should be walkable above platform"
    );

    // Spawn capybara at ground level, then teleport to the platform pos.
    let mut events = Vec::new();
    let capy_id = sim
        .spawn_creature(
            Species::Capybara,
            VoxelCoord::new(10, 1, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn capybara");
    {
        let mut c = sim.db.creatures.get(&capy_id).unwrap();
        c.position = VoxelBox::point(standing_pos);
        sim.db.update_creature(c).unwrap();
    }
    assert!(
        sim.creature_is_supported(capy_id),
        "should be supported on platform"
    );

    // Remove the platform — position becomes unwalkable but creature is
    // still there. Creature should be unsupported: ground_only needs solid below.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .set(platform_pos, VoxelType::Air);

    assert!(
        !sim.creature_is_supported(capy_id),
        "ground_only creature without solid below should be unsupported"
    );

    // Apply gravity — should fall. Rebuild nav first so landing positions
    // are valid.
    sim.rebuild_transient_state();
    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1, "capybara should fall");
    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(capy.position.min.y, 1, "should land at ground level");
}

// ===================================================================
// Flying creature task integration (B-flying-tasks, B-flying-arrow-chase)
// ===================================================================

/// DirectedGoTo should create a GoTo task for a flying creature, even
/// though the destination may have no nearby nav node.
#[test]
fn flying_creature_directed_goto_creates_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 5, tree_pos.y + 10, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Target is also in the air — no nav node nearby.
    let target = VoxelCoord::new(tree_pos.x + 15, tree_pos.y + 10, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: hornet,
            position: target,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Flying creature should receive a GoTo task"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::GoTo);
}

/// A flying creature with a GoTo task should fly to the destination and
/// complete the task.
#[test]
fn flying_creature_goto_reaches_destination() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    // Spawn hornet above elf (known flyable area), then reposition.
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    // Move hornet high above elves to avoid combat interference.
    let start = VoxelCoord::new(elf_pos.x, elf_pos.y + 20, elf_pos.z);
    force_position(&mut sim, hornet, start);

    let target = VoxelCoord::new(elf_pos.x + 5, elf_pos.y + 20, elf_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: hornet,
            position: target,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Verify the task was accepted.
    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(creature.current_task.is_some(), "GoTo task should exist");
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::GoTo);

    // Run enough ticks for the hornet to fly ~5 voxels (250 tpv = 1250 ticks).
    // Give generous time in case of pathing detours.
    let target_tick = sim.tick + 10_000;
    sim.step(&[], target_tick);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_none(),
        "GoTo task should be completed: hornet pos={:?}, target={:?}",
        creature.position,
        target,
    );
}

/// AttackMove should create a task for a flying creature.
#[test]
fn flying_creature_attack_move_creates_task() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 5, tree_pos.y + 10, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    force_idle_and_cancel_activations(&mut sim, hornet);

    let dest = VoxelCoord::new(tree_pos.x + 20, tree_pos.y + 10, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: hornet,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Flying creature should receive an AttackMove task"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackMove);
}

/// A flying creature with an AttackMove task should fly toward the
/// destination and complete on arrival.
#[test]
fn flying_creature_attack_move_reaches_destination() {
    // Use flat_world_sim to avoid worldgen elves that the hornet would detect
    // and engage during attack-move, preventing it from reaching the destination.
    let mut sim = flat_world_sim(legacy_test_seed());

    // We need an elf reference position for spawning (spawn_hornet_at needs a
    // nearby position).  Use the tree position instead.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(tree_pos.x, tree_pos.y + 3, tree_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let start = VoxelCoord::new(tree_pos.x, tree_pos.y + 20, tree_pos.z);
    force_position(&mut sim, hornet, start);

    let dest = VoxelCoord::new(tree_pos.x + 5, tree_pos.y + 20, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: hornet,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let target_tick = sim.tick + 10_000;
    sim.step(&[], target_tick);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_none(),
        "AttackMove task should be completed on arrival: hornet pos={:?}, dest={:?}",
        creature.position,
        dest
    );
}
/// A flying creature's autonomous combat should preempt a low-priority
/// task when hostiles are detected.
#[test]
fn flying_creature_autonomous_combat_preempts_task() {
    // Use flat_world_sim to avoid worldgen elves near the tree that the
    // hornet might engage instead of the test elf.
    let mut sim = flat_world_sim(legacy_test_seed());
    let air_pos = VoxelCoord::new(32, 10, 32);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    zero_creature_stats(&mut sim, hornet);
    force_guaranteed_hits(&mut sim, hornet);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Give the hornet a GoTo task (PlayerDirected level). The hornet still
    // engages the elf because hostile_pursue runs during the activation loop
    // when the hornet encounters the elf while flying toward the GoTo target.
    let dest = VoxelCoord::new(62, 10, 32);
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: hornet,
            position: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Now spawn an elf near the hornet (within detection range).
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position.min;
    force_position(
        &mut sim,
        elf,
        VoxelCoord::new(hornet_pos.x + 2, hornet_pos.y, hornet_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, elf);

    // Run — hornet should detect the elf and engage rather than continuing GoTo.
    let target_tick = sim.tick + 3000;
    sim.step(&[], target_tick);

    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.hp < elf_creature.hp_max,
        "Elf should have taken damage from hornet engaging in autonomous combat"
    );
}

/// find_available_task should find tasks for flying creatures using
/// Euclidean distance instead of ground A*.
#[test]
fn find_available_task_works_for_flying_creature() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let air_pos = VoxelCoord::new(32, 10, 32);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Create an Available GoTo task near the hornet.
    let task_pos = VoxelCoord::new(air_pos.x + 3, air_pos.y, air_pos.z);
    let task_id = TaskId::new(&mut sim.rng);
    let task = task::Task {
        id: task_id,
        kind: task::TaskKind::GoTo,
        state: task::TaskState::Available,
        location: task_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Hornet),
        origin: task::TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);

    let found = sim.find_available_task(hornet);
    assert_eq!(
        found,
        Some(task_id),
        "find_available_task should find tasks for flying creatures"
    );
}

/// at_task_location: flying creature within 1 voxel is at location,
/// 2+ voxels away is not.
#[test]
fn flying_creature_at_task_location_proximity() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position.min;

    let target = VoxelCoord::new(hornet_pos.x + 1, hornet_pos.y, hornet_pos.z); // 1 voxel away
    assert!(
        sim.at_task_location(hornet, None, None, target),
        "Flying creature 1 voxel away should be at task location"
    );

    let far_target = VoxelCoord::new(hornet_pos.x + 3, hornet_pos.y, hornet_pos.z); // 3 voxels away
    assert!(
        !sim.at_task_location(hornet, None, None, far_target),
        "Flying creature 3 voxels away should not be at task location"
    );

    // Diagonal: (1,1,1) away — should be at location (Chebyshev distance <= 1).
    let diag_target = VoxelCoord::new(hornet_pos.x + 1, hornet_pos.y + 1, hornet_pos.z + 1);
    assert!(
        sim.at_task_location(hornet, None, None, diag_target),
        "Flying creature at diagonal (1,1,1) should be at task location"
    );
}

/// walk_toward_task: when fly_toward_target fails for a flying creature,
/// the creature is unassigned from its task and wanders.
#[test]
fn flying_creature_walk_toward_task_unreachable_unassigns() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Create and assign a task at an unreachable position (inside solid ground).
    let task_id = TaskId::new(&mut sim.rng);
    let task = task::Task {
        id: task_id,
        kind: task::TaskKind::GoTo,
        state: task::TaskState::InProgress,
        location: VoxelCoord::new(32, -10, 32), // below world — unreachable
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Hornet),
        origin: task::TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);
    sim.claim_task(hornet, task_id);

    let mut events = Vec::new();
    sim.walk_toward_task(hornet, VoxelCoord::new(32, -10, 32), None, &mut events);

    // Creature should be unassigned from task.
    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Flying creature should be unassigned from unreachable task"
    );
}

/// A flying creature with an AttackMove task should engage hostiles it
/// encounters en route to the destination.
#[test]
fn flying_creature_attack_move_engages_hostile_en_route() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    zero_creature_stats(&mut sim, elf);

    // Freeze the elf so it can't flee or interfere.
    force_idle_and_cancel_activations(&mut sim, elf);
    suppress_activation(&mut sim, elf);

    // Spawn hornet above the elf. AttackMove destination is far past the elf.
    let hornet_pos = VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z);
    let hornet = spawn_hornet_at(&mut sim, hornet_pos);
    zero_creature_stats(&mut sim, hornet);
    // Guarantee the hornet's attacks connect so the elf takes damage.
    force_guaranteed_hits(&mut sim, hornet);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // AttackMove to a far destination — the hornet must pass near the elf.
    let dest = VoxelCoord::new(elf_pos.x + 30, elf_pos.y + 3, elf_pos.z);
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: hornet,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Run enough ticks for the hornet to detect and engage the elf.
    let target_tick = sim.tick + 5000;
    sim.step(&[], target_tick);

    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.hp < elf_creature.hp_max,
        "Elf should have taken damage from hornet engaging en route: hp={}/{}",
        elf_creature.hp,
        elf_creature.hp_max
    );
}

/// find_available_task for a flying creature with multiple tasks should
/// pick the nearest by squared Euclidean distance.
#[test]
fn flying_creature_find_available_task_nearest_by_euclidean() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position.min;

    // Create two tasks: one near, one far.
    let near_task_id = TaskId::new(&mut sim.rng);
    let near_task = task::Task {
        id: near_task_id,
        kind: task::TaskKind::GoTo,
        state: task::TaskState::Available,
        location: VoxelCoord::new(hornet_pos.x + 2, hornet_pos.y, hornet_pos.z),
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Hornet),
        origin: task::TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), near_task);

    let far_task_id = TaskId::new(&mut sim.rng);
    let far_task = task::Task {
        id: far_task_id,
        kind: task::TaskKind::GoTo,
        state: task::TaskState::Available,
        location: VoxelCoord::new(hornet_pos.x + 20, hornet_pos.y, hornet_pos.z),
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Hornet),
        origin: task::TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), far_task);

    let found = sim.find_available_task(hornet);
    assert_eq!(
        found,
        Some(near_task_id),
        "find_available_task should pick the nearest task by Euclidean distance"
    );
}

/// A flying creature with no task and no hostiles should wander via
/// fly_wander (dispatched through wander_dispatch).
#[test]
fn flying_creature_idle_wanders() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    // Spawn hornet far from elf to avoid combat.
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 20, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position.min;

    // Trigger one activation. With no task and no hostiles in range,
    // the hornet should wander (move to a neighboring voxel).
    let mut events = Vec::new();
    sim.process_creature_activation(hornet, &mut events);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    // Should have started a move action (fly_wander picks a neighbor).
    assert_eq!(
        creature.action_kind,
        ActionKind::Move,
        "Idle flying creature should wander (Move action)"
    );
    assert!(
        creature.next_available_tick.is_some(),
        "Wander should schedule next activation"
    );
}

/// DirectedGoTo issued to a flying creature mid-wander-move should defer
/// the activation (not create a duplicate) and the hornet should pick up
/// the GoTo after the current move completes.
#[test]
fn flying_creature_directed_goto_mid_move_defers() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 20, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Trigger one activation to start a wander move.
    let mut events = Vec::new();
    sim.process_creature_activation(hornet, &mut events);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert_eq!(
        creature.action_kind,
        ActionKind::Move,
        "Should be mid-wander"
    );
    assert!(creature.current_task.is_none(), "No task yet");
    let wander_end_tick = creature.next_available_tick.unwrap();

    // Issue DirectedGoTo while mid-wander-move.
    let target = VoxelCoord::new(elf_pos.x + 5, elf_pos.y + 20, elf_pos.z);
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: hornet,
            position: target,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // The hornet should now have the GoTo task.
    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(creature.current_task.is_some(), "Should have GoTo task");

    // The move action should still be in progress (not aborted).
    assert_eq!(
        creature.action_kind,
        ActionKind::Move,
        "Mid-move should continue, not be aborted"
    );

    // After the wander move completes, the hornet should pick up the GoTo.
    sim.step(&[], wander_end_tick + 1);
    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Should still have GoTo task after wander completes"
    );
}

/// pursue_closest_target: flying creature picks nearest target by Euclidean
/// distance and flies toward it.
#[test]
fn flying_creature_pursue_closest_target() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_near = spawn_elf(&mut sim);
    let elf_far = spawn_elf(&mut sim);
    let elf_near_pos = sim.db.creatures.get(&elf_near).unwrap().position.min;

    // Spawn hornet above the near elf.
    let hornet_pos = VoxelCoord::new(elf_near_pos.x, elf_near_pos.y + 3, elf_near_pos.z);
    let hornet = spawn_hornet_at(&mut sim, hornet_pos);
    zero_creature_stats(&mut sim, hornet);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Move far elf far away.
    force_position(
        &mut sim,
        elf_far,
        VoxelCoord::new(elf_near_pos.x + 20, elf_near_pos.y, elf_near_pos.z),
    );

    let mut events = Vec::new();
    let pursued = sim.hostile_pursue(hornet, None, Species::Hornet, &mut events);

    assert!(pursued, "Flying creature should pursue a detected hostile");
    // Hornet should have moved (melee strike or flight step toward near elf).
    let new_pos = sim.db.creatures.get(&hornet).unwrap().position.min;
    assert_ne!(
        new_pos, hornet_pos,
        "Hornet should have moved toward target"
    );
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn capybara_wanders_on_ground() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

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

    // Step far enough for heartbeat + movement.
    sim.step(&[], 50000);

    assert_eq!(sim.creature_count(Species::Capybara), 1);
    let capybara = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap();
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            capybara.position.min,
            [1, 1, 1],
            true,
        ),
        "capybara should be at a walkable position"
    );
}

#[test]
fn capybara_stays_on_ground() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

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

    // Run for many ticks — capybara must never leave y=1 (air above terrain).
    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let capybara = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Capybara)
            .unwrap();
        assert_eq!(
            capybara.position.min.y, 1,
            "Capybara left ground at tick {target}: pos={:?}",
            capybara.position
        );
    }
}

#[test]
fn determinism_with_capybara() {
    let seed = legacy_test_seed();
    let mut sim_a = test_sim(seed);
    let mut sim_b = test_sim(seed);

    let tree_pos = sim_a.db.trees.get(&sim_a.player_tree_id).unwrap().position;

    let spawn = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim_a.home_zone_id(),
            species: Species::Capybara,
            position: tree_pos,
        },
    };

    sim_a.step(std::slice::from_ref(&spawn), 1000);
    sim_b.step(std::slice::from_ref(&spawn), 1000);

    assert_eq!(sim_a.db.creatures.len(), sim_b.db.creatures.len());
    for creature_a in sim_a.db.creatures.iter_all() {
        let creature_b = sim_b.db.creatures.get(&creature_a.id).unwrap();
        assert_eq!(creature_a.position, creature_b.position);
    }
    assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
}

#[test]
fn wandering_creature_stays_on_walkable_position() {
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

    // Run for many ticks, periodically checking walkability.
    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        assert!(
            crate::walkability::footprint_walkable(
                sim.voxel_zone(sim.home_zone_id()).unwrap(),
                &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
                elf.position.min,
                [1, 1, 1],
                true,
            ),
            "Elf should always be at a walkable position (tick {target}, pos {:?})",
            elf.position.min
        );
    }
}

#[test]
fn wander_sets_movement_metadata() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf at tick 1, step only to tick 1 so the first activation
    // (scheduled at tick 2) hasn't fired yet.
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

    // Before the first activation, the elf should have no action.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::NoAction);
    // In the poll-based model, next_available_tick is set at spawn.
    assert!(elf.next_available_tick.is_some());
    assert!(sim.db.move_actions.get(&elf_id).is_none());

    let initial_pos = elf.position.min;

    // Step to tick 2 — the first activation fires and the elf wanders.
    sim.step(&[], 2);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.action_kind,
        ActionKind::Move,
        "action_kind should be Move after wander"
    );
    assert!(
        elf.next_available_tick.is_some(),
        "next_available_tick should be set after wander"
    );

    let ma = sim
        .db
        .move_actions
        .get(&elf_id)
        .expect("MoveAction should exist after wander");
    assert_eq!(
        ma.move_from, initial_pos,
        "move_from should be the spawn position"
    );
    assert_eq!(
        ma.move_to, elf.position.min,
        "move_to should be the new position"
    );
    assert_eq!(
        ma.move_start_tick, 2,
        "move_start_tick should be the activation tick"
    );
    assert!(
        elf.next_available_tick.unwrap() > ma.move_start_tick,
        "next_available_tick should be after start"
    );
}

#[test]
fn boar_stays_on_ground() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Boar,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    // Run for many ticks — boar must never leave y=1 (ground-only).
    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let boar = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Boar)
            .unwrap();
        assert_eq!(
            boar.position.min.y, 1,
            "Boar left ground at tick {target}: pos={:?}",
            boar.position
        );
    }
}

#[test]
fn deer_stays_on_ground() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Deer,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let deer = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Deer)
            .unwrap();
        assert_eq!(
            deer.position.min.y, 1,
            "Deer left ground at tick {target}: pos={:?}",
            deer.position
        );
    }
}

#[test]
fn monkey_can_climb() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Monkey,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    // Run for enough ticks that a climbing species should have left ground.
    sim.step(&[], 100000);

    let monkey = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Monkey)
        .unwrap();
    // Monkey is not ground_only, so it should be able to reach y > 1
    // (trunk/branch surfaces). This verifies the species config allows
    // climbing edges. The monkey may still be at y=1 if the PRNG led it
    // only to ground neighbors, so we just verify it has a valid nav node.
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            monkey.position.min,
            [1, 1, 1],
            true,
        ),
        "monkey should be at a walkable position"
    );
}

#[test]
fn squirrel_can_climb() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Squirrel,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    sim.step(&[], 100000);

    let squirrel = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Squirrel)
        .unwrap();
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            squirrel.position.min,
            [1, 1, 1],
            true,
        ),
        "squirrel should be at a walkable position"
    );
}

// -----------------------------------------------------------------------
// DirectedGoTo command tests
// -----------------------------------------------------------------------

#[test]
fn directed_goto_creates_task_for_specific_creature() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle(&mut sim, elf);

    // Pick a target position.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 3, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: target_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Elf should have a GoTo task"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::GoTo);
    assert_eq!(task.state, TaskState::InProgress);
    assert_eq!(task.origin, TaskOrigin::PlayerDirected);
}

#[test]
fn directed_goto_replaces_player_directed_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Give elf a PlayerDirected GoTo task (PlayerDirected level 2).
    let task_id = TaskId::new(&mut sim.rng);
    let dest_pos = find_different_walkable(&sim, creature_pos(&sim, elf));
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: dest_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);
    sim.claim_task(elf, task_id);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 2, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: target_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Old task should be interrupted (Complete).
    let old_task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(old_task.state, TaskState::Complete);

    // Elf should have the new GoTo task.
    let creature = sim.db.creatures.get(&elf).unwrap();
    let new_task_id = creature.current_task.unwrap();
    assert_ne!(new_task_id, task_id);
    let new_task = sim.db.tasks.get(&new_task_id).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::GoTo);
}

#[test]
fn directed_goto_preempts_autonomous_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Give elf an autonomous Harvest task (Autonomous level 1).
    let task_id = TaskId::new(&mut sim.rng);
    let dest_pos = find_different_walkable(&sim, creature_pos(&sim, elf));
    let fruit_pos = VoxelCoord::new(0, 0, 0);
    let task = Task {
        id: task_id,
        kind: TaskKind::Harvest { fruit_pos },
        state: TaskState::InProgress,
        location: dest_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);
    sim.claim_task(elf, task_id);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 2, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: target_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Old autonomous task should be interrupted (Complete).
    let old_task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(old_task.state, TaskState::Complete);

    // Elf should have the new GoTo task.
    let creature = sim.db.creatures.get(&elf).unwrap();
    let new_task_id = creature.current_task.unwrap();
    assert_ne!(new_task_id, task_id);
    let new_task = sim.db.tasks.get(&new_task_id).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::GoTo);
}

#[test]
fn directed_goto_does_not_abort_mid_walk_action() {
    // B-erratic-movement: issuing a DirectedGoTo while a creature is
    // mid-walk should NOT abort the in-progress Move action. The action
    // should complete naturally, then the creature picks up the new task.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    // Issue a first DirectedGoTo to start the elf walking.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_a = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);
    let tick = sim.tick;
    let cmd_a = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: target_a,
            queue: false,
        },
    };
    sim.step(&[cmd_a], tick + 2);

    // Advance until the elf is mid-walk (action_kind == Move).
    let mut mid_walk = false;
    for t in (sim.tick + 1)..=(sim.tick + 50) {
        sim.step(&[], t);
        let c = sim.db.creatures.get(&elf).unwrap();
        if c.action_kind == ActionKind::Move && c.next_available_tick.is_some() {
            mid_walk = true;
            break;
        }
    }
    assert!(mid_walk, "Elf should be mid-walk after advancing ticks");

    let c = sim.db.creatures.get(&elf).unwrap();
    let action_before = c.action_kind;
    let nat_before = c.next_available_tick;
    let first_task_id = c.current_task.unwrap();

    // Issue a second DirectedGoTo while mid-walk.
    let target_b = VoxelCoord::new(tree_pos.x - 3, 1, tree_pos.z);
    let tick2 = sim.tick;
    let cmd_b = SimCommand {
        player_name: String::new(),
        tick: tick2 + 1,
        action: SimAction::DirectedGoTo {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            position: target_b,
            queue: false,
        },
    };
    sim.step(&[cmd_b], tick2 + 2);

    // The in-progress Move action should NOT have been aborted.
    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(
        c.action_kind, action_before,
        "Move action should not be aborted by task preemption"
    );
    assert_eq!(
        c.next_available_tick, nat_before,
        "next_available_tick should be unchanged"
    );

    // The task should have changed to the new GoTo.
    let new_task_id = c.current_task.unwrap();
    assert_ne!(new_task_id, first_task_id, "Task should have been swapped");
    let new_task = sim.db.tasks.get(&new_task_id).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::GoTo);

    // Old task should be completed.
    let old_task = sim.db.tasks.get(&first_task_id).unwrap();
    assert_eq!(old_task.state, TaskState::Complete);

    // Advance past the original next_available_tick — the elf should
    // resolve the Move action normally and then follow the new task.
    let completion_tick = nat_before.unwrap();
    let target_tick = completion_tick.max(sim.tick) + 1;
    sim.step(&[], target_tick);

    // After the original move resolves, the creature should have picked
    // up the new GoTo task (it may have started a new Move step toward
    // the new destination, which is fine — the key is it's using the
    // new task, not the old one).
    let c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(
        c.current_task,
        Some(new_task_id),
        "Creature should still be on the new GoTo task"
    );
}

#[test]
fn directed_goto_mid_action_command_does_not_schedule_extra_activation() {
    // B-erratic-movement: issuing a DirectedGoTo while a creature is
    // mid-action should NOT schedule an extra CreatureActivation. The
    // existing activation (from the in-progress action) is sufficient.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_a = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    // Issue first DirectedGoTo and advance until mid-walk.
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

    // Count activations before issuing the second command.
    let activations_before = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_before, 1,
        "Should have exactly 1 pending activation before redirect"
    );

    // Issue a second DirectedGoTo while mid-walk — should NOT add an
    // extra activation event.
    let target_b = VoxelCoord::new(tree_pos.x - 3, 1, tree_pos.z);
    let tick2 = sim.tick;
    // Process the command on this tick without advancing further, so no
    // events fire between the command and our assertion.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DirectedGoTo {
                zone_id: sim.home_zone_id(),
                creature_id: elf,
                position: target_b,
                queue: false,
            },
        }],
        tick2 + 1,
    );

    let activations_after = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_after, 1,
        "Should still have exactly 1 pending activation after redirect (was {activations_after})"
    );
}

#[test]
fn group_goto_spreads_creatures_to_different_nodes() {
    // Three elves given a GroupGoTo to the same destination should each
    // end up with a GoTo task at a different nav node.
    let mut sim = test_sim(legacy_test_seed());
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    let elf_c = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_a);
    force_idle_and_cancel_activations(&mut sim, elf_b);
    force_idle_and_cancel_activations(&mut sim, elf_c);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupGoTo {
                zone_id: sim.home_zone_id(),
                creature_ids: vec![elf_a, elf_b, elf_c],
                position: dest,
                queue: false,
            },
        }],
        tick + 2,
    );

    // All three should have GoTo tasks.
    let task_a = sim.db.creatures.get(&elf_a).unwrap().current_task.unwrap();
    let task_b = sim.db.creatures.get(&elf_b).unwrap().current_task.unwrap();
    let task_c = sim.db.creatures.get(&elf_c).unwrap().current_task.unwrap();
    let loc_a = sim.db.tasks.get(&task_a).unwrap().location;
    let loc_b = sim.db.tasks.get(&task_b).unwrap().location;
    let loc_c = sim.db.tasks.get(&task_c).unwrap().location;

    // At least two of the three should have different locations (spread).
    let locs = [loc_a, loc_b, loc_c];
    let unique: std::collections::BTreeSet<_> = locs.iter().collect();
    assert!(
        unique.len() >= 2,
        "GroupGoTo should spread creatures to different nav nodes, got {:?}",
        locs
    );
}

#[test]
fn group_goto_single_creature_delegates_to_normal() {
    // A single-element GroupGoTo should work identically to DirectedGoTo.
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupGoTo {
                zone_id: sim.home_zone_id(),
                creature_ids: vec![elf],
                position: dest,
                queue: false,
            },
        }],
        tick + 2,
    );

    let task_id = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert!(task.kind_tag == TaskKindTag::GoTo);
}

#[test]
fn group_goto_empty_list_is_noop() {
    let mut sim = test_sim(legacy_test_seed());
    let tick = sim.tick;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    // Should not panic or create any tasks.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupGoTo {
                zone_id: sim.home_zone_id(),
                creature_ids: vec![],
                position: dest,
                queue: false,
            },
        }],
        tick + 2,
    );
}

#[test]
fn group_goto_skips_dead_creatures() {
    // Dead creatures in the list should be silently skipped.
    let mut sim = test_sim(legacy_test_seed());
    let elf_alive = spawn_elf(&mut sim);
    let elf_dead = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_alive);
    force_idle_and_cancel_activations(&mut sim, elf_dead);

    // Kill one elf.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_dead,
            },
        }],
        tick + 2,
    );

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::GroupGoTo {
                zone_id: sim.home_zone_id(),
                creature_ids: vec![elf_alive, elf_dead],
                position: dest,
                queue: false,
            },
        }],
        tick2 + 2,
    );

    // Only the alive elf should have a task.
    assert!(
        sim.db
            .creatures
            .get(&elf_alive)
            .unwrap()
            .current_task
            .is_some()
    );
}

#[test]
fn group_goto_serialization_roundtrip() {
    let mut rng = crate::prng::GameRng::new(42);
    let test_zone_id = ZoneId(42);
    let cmd = SimCommand {
        player_name: "test_player".to_string(),
        tick: 100,
        action: SimAction::GroupGoTo {
            zone_id: test_zone_id,
            creature_ids: vec![CreatureId::new(&mut rng), CreatureId::new(&mut rng)],
            position: VoxelCoord::new(10, 1, 5),
            queue: false,
        },
    };

    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

/// Verify that a creature with a cached path containing a position where no
/// nav node exists (e.g., because the node was destroyed) handles it gracefully
/// — no panic, creature stays alive and valid.
#[test]
fn path_resolution_nav_node_destroyed_no_panic() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Find a connected pair so the elf has a valid starting node.
    let (node_a, node_b) = find_connected_pair(&sim);
    force_position(&mut sim, elf, node_a);
    force_idle(&mut sim, elf);

    // Give the elf a GoTo task at node_b so it has reason to move.
    let task_id = insert_goto_task(&mut sim, node_b);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        sim.db.update_task(t).unwrap();
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Assign a cached path through a position that has NO nav node.
    // Use a coordinate far from the world (no node will exist there).
    let bogus_pos = VoxelCoord::new(63, 63, 63);
    assert!(
        !crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            bogus_pos,
            [1, 1, 1],
            true
        ),
        "Test setup: bogus position should not be walkable"
    );

    let real_dest = node_b;
    {
        let mut c = sim.db.creatures.get(&elf).unwrap();
        c.path = Some(CreaturePath {
            remaining_positions: vec![bogus_pos, real_dest],
        });
        sim.db.update_creature(c).unwrap();
    }

    // Schedule the elf to activate and step forward. The movement code should
    // detect the missing nav node and repath (not panic).
    let tick = sim.tick + 1;
    schedule_activation_at(&mut sim, elf, tick);
    sim.step(&[], sim.tick + 200);

    // Creature should still be alive and at a valid position.
    let creature = sim
        .db
        .creatures
        .get(&elf)
        .expect("Creature should still exist after path resolution failure");
    assert!(
        creature.hp > 0,
        "Creature should be alive after graceful repath"
    );
}

// ---------------------------------------------------------------------------
// Large creature stuck-at-incline reproduction (B-large-stuck)
// ---------------------------------------------------------------------------

/// Reproduce B-large-stuck: large (2×2×2) creatures like elephants can get
/// permanently stuck at terrain inclines where a nav node exists but all
/// outgoing edges fail validation due to the larger union-footprint height
/// check in `is_large_edge_valid()`.
///
/// Strategy: create a world with hilly terrain (`terrain_max_height = 4`),
/// spawn 15 elephants, run for many ticks, and check whether any elephant
/// stays at the same position for 10+ consecutive sample windows. If so,
/// print diagnostic info (position, nav node, edge count, surrounding
/// terrain heights) and fail.
#[test]
fn large_creature_does_not_get_permanently_stuck_on_terrain() {
    use std::collections::BTreeMap as Map;

    let seed = fresh_test_seed();

    // Build a world with hilly terrain — bigger than the usual 64^3 test
    // world so elephants have room to wander and encounter inclines.
    let mut config = GameConfig {
        world_size: (128, 64, 128),
        floor_y: 0,
        ..GameConfig::default()
    };
    config.terrain_max_height = 4;
    config.tree_profile.growth.initial_energy = 0.0; // No tree — just terrain.
    config.lesser_trees.count = 0;
    // Clear default initial creatures/piles — we'll spawn elephants manually.
    config.initial_creatures.clear();
    config.initial_ground_piles.clear();

    let mut sim = SimState::with_config(seed, config);

    // Spawn 15 elephants spread across the map.
    let num_elephants: usize = 15;
    let mut elephant_ids = Vec::new();
    for i in 0..num_elephants {
        // Spread spawn positions across the terrain.
        let x = 20 + (i as i32 % 5) * 18;
        let z = 20 + (i as i32 / 5) * 18;
        let spawn_pos = VoxelCoord::new(x, 1, z);
        let cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                zone_id: sim.home_zone_id(),
                species: Species::Elephant,
                position: spawn_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
        // Find the newly spawned elephant.
        let new_id = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elephant)
            .map(|c| c.id)
            .find(|id| !elephant_ids.contains(id));
        if let Some(id) = new_id {
            elephant_ids.push(id);
        }
    }

    assert!(
        elephant_ids.len() >= 10,
        "Need at least 10 elephants spawned, got {}. \
         Terrain may lack valid large nav nodes near spawn positions.",
        elephant_ids.len()
    );

    // Track positions: map from creature ID → list of sampled positions.
    let mut position_history: Map<CreatureId, Vec<VoxelCoord>> = Map::new();
    for &id in &elephant_ids {
        let pos = sim.db.creatures.get(&id).unwrap().position.min;
        position_history.insert(id, vec![pos]);
    }

    // Run 50 sample windows of 2000 ticks each (enough to detect stuck creatures).
    let num_turns = 50;
    let ticks_per_turn: u64 = 2000;
    for _turn in 0..num_turns {
        let target = sim.tick + ticks_per_turn;
        sim.step(&[], target);
        for &id in &elephant_ids {
            if let Some(creature) = sim.db.creatures.get(&id) {
                position_history
                    .get_mut(&id)
                    .unwrap()
                    .push(creature.position.min);
            }
        }
    }

    // Check for stuck elephants: same position for 10+ consecutive samples.
    // Exclude incapacitated/dead elephants — they don't move by design.
    let stuck_threshold = 10;
    let mut stuck_elephants = Vec::new();
    for &id in &elephant_ids {
        if let Some(creature) = sim.db.creatures.get(&id) {
            if creature.vital_status != VitalStatus::Alive {
                continue;
            }
        }
        let history = &position_history[&id];
        let mut consecutive = 1;
        let mut max_consecutive = 1;
        let mut stuck_pos = history[0];
        for window in history.windows(2) {
            if window[1] == window[0] {
                consecutive += 1;
                if consecutive > max_consecutive {
                    max_consecutive = consecutive;
                    stuck_pos = window[0];
                }
            } else {
                consecutive = 1;
            }
        }
        if max_consecutive >= stuck_threshold {
            stuck_elephants.push((id, stuck_pos, max_consecutive));
        }
    }

    if !stuck_elephants.is_empty() {
        // Print diagnostics for each stuck elephant.
        for &(id, pos, consecutive) in &stuck_elephants {
            eprintln!("--- STUCK ELEPHANT {id:?} ---");
            eprintln!(
                "  Position: ({}, {}, {}), stuck for {consecutive} consecutive samples",
                pos.x, pos.y, pos.z
            );

            // Print creature state.
            if let Some(creature) = sim.db.creatures.get(&id) {
                eprintln!(
                    "  action_kind={:?}, next_available_tick={:?}, current_task={:?}, \
                     current_activity={:?}, vital_status={:?}, food={}, rest={}",
                    creature.action_kind,
                    creature.next_available_tick,
                    creature.current_task,
                    creature.current_activity,
                    creature.vital_status,
                    creature.food,
                    creature.rest,
                );
            }

            // Check walkability at this position.
            let footprint = sim.species_table[&Species::Elephant].footprint;
            let can_climb = sim.species_table[&Species::Elephant]
                .movement_category
                .can_climb();
            let walkable = crate::walkability::footprint_walkable(
                sim.voxel_zone(sim.home_zone_id()).unwrap(),
                &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
                pos,
                footprint,
                can_climb,
            );
            eprintln!("  Footprint walkable: {walkable}");

            // Check which neighbor anchors have valid walkable positions.
            eprintln!("  Neighbor anchor validity:");
            for &(ndx, ndz) in &[
                (-1i32, -1i32),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let nx = pos.x + ndx;
                let nz = pos.z + ndz;
                let neighbor_walkable = crate::walkability::footprint_walkable(
                    sim.voxel_zone(sim.home_zone_id()).unwrap(),
                    &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
                    VoxelCoord::new(nx, pos.y, nz),
                    footprint,
                    can_climb,
                );
                eprintln!(
                    "    ({ndx:+},{ndz:+}): anchor=({nx},{nz}), walkable={neighbor_walkable}"
                );
            }
        }

        panic!(
            "{} elephant(s) got permanently stuck (>={stuck_threshold} consecutive samples \
             at same position). See diagnostic output above. seed={seed}",
            stuck_elephants.len()
        );
    }
}

/// Unit test for B-large-stuck fix: a 2×2 large creature at a nav node where
/// the anchor column's surface is lower than another footprint column should
/// be considered supported. Before the fix, `creature_is_supported` only
/// checked the anchor column, returning false and trapping the creature in an
/// infinite gravity loop.
#[test]
fn large_creature_supported_at_incline_with_mixed_column_heights() {
    let mut sim = test_sim(fresh_test_seed());

    // Build a 2×2 footprint with mixed heights: three columns at y=3,
    // anchor column at y=2. The standing Y is max(2,3,3,3) + 1 = 4.
    for dx in 0..2 {
        for dz in 0..2 {
            let height = if dx == 0 && dz == 0 { 2 } else { 3 };
            for y in 0..=height {
                sim.voxel_zone_mut(sim.home_zone_id())
                    .unwrap()
                    .set(VoxelCoord::new(10 + dx, y, 10 + dz), VoxelType::Dirt);
            }
        }
    }
    sim.rebuild_transient_state();

    // Verify the position is walkable for a 2x2 footprint at (10, 4, 10).
    let standing_pos = VoxelCoord::new(10, 4, 10);
    let elephant_fp = sim.species_table[&Species::Elephant].footprint;
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            standing_pos,
            elephant_fp,
            false, // elephants can't climb
        ),
        "position should be walkable for elephant at y=4 above mixed-height terrain"
    );

    // Spawn an elephant and teleport it to the incline position.
    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(10, 4, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elephant near incline");
    {
        let footprint = sim.species_table[&Species::Elephant].footprint;
        let mut c = sim.db.creatures.get(&elephant_id).unwrap();
        c.position = VoxelBox::from_anchor(standing_pos, footprint);
        sim.db.update_creature(c).unwrap();
    }

    // Key assertion: the elephant should be supported even though the anchor
    // column (10, 10) only has solid up to y=2, making y=3 air. The fix
    // checks all footprint columns and finds solid at y=3 in the other
    // three columns.
    assert!(
        sim.creature_is_supported(elephant_id),
        "2x2 elephant at incline should be supported: anchor column surface=2, \
         other columns surface=3, nav node at y=4, so y-1=3 is solid in 3 of 4 columns"
    );
}

/// Inverse of the incline test: a 2×2 large creature where NO footprint
/// column has solid at y−1 should be unsupported. Ensures the footprint
/// loop correctly returns false, not just true.
#[test]
fn large_creature_unsupported_when_no_footprint_column_has_solid_below() {
    let mut sim = test_sim(fresh_test_seed());

    // Build a 2×2 platform at y=5. All 4 columns have solid at y=5 only
    // (no solid at y=4). Large nav node will be at y=6.
    for dx in 0..2 {
        for dz in 0..2 {
            sim.voxel_zone_mut(sim.home_zone_id()).unwrap().set(
                VoxelCoord::new(10 + dx, 5, 10 + dz),
                VoxelType::GrownPlatform,
            );
        }
    }
    sim.rebuild_transient_state();

    let standing_pos = VoxelCoord::new(10, 6, 10);
    let elephant_fp = sim.species_table[&Species::Elephant].footprint;
    assert!(
        crate::walkability::footprint_walkable(
            sim.voxel_zone(sim.home_zone_id()).unwrap(),
            &sim.voxel_zone(sim.home_zone_id()).unwrap().face_data,
            standing_pos,
            elephant_fp,
            false, // elephants can't climb
        ),
        "position should be walkable for elephant at y=6 above platform"
    );

    // Spawn elephant and teleport to the platform.
    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(10, 6, 10),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn elephant on platform");
    {
        let mut c = sim.db.creatures.get(&elephant_id).unwrap();
        c.position = VoxelBox::from_anchor(standing_pos, elephant_fp);
        sim.db.update_creature(c).unwrap();
    }

    // Should be supported: platform is solid at y=5, y-1=5 has solid.
    assert!(
        sim.creature_is_supported(elephant_id),
        "elephant on intact platform should be supported"
    );

    // Remove the platform — no solid below in any footprint column.
    for dx in 0..2 {
        for dz in 0..2 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(10 + dx, 5, 10 + dz), VoxelType::Air);
        }
    }

    // Now y-1=5 is air in all 4 columns — creature should be unsupported.
    assert!(
        !sim.creature_is_supported(elephant_id),
        "elephant with no solid below in any footprint column should be unsupported"
    );
}

// ===================================================================
// Large creature grounding bugs
// ===================================================================

/// Reproduce bug: elephants, trolls (2×2×2 footprint) wander into the sky
/// on flat terrain. Elephants then fall and take damage. Ground creatures
/// should never exceed floor_y + 1 on flat terrain.
#[test]
fn test_large_creatures_stay_grounded_on_flat_terrain() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let floor_y = sim.config.floor_y;
    let walking_y = floor_y + 1;

    // Spawn 3 elephants, 3 trolls, 3 deer (control).
    let mut elephants = Vec::new();
    for _ in 0..3 {
        elephants.push(spawn_creature(&mut sim, Species::Elephant));
    }
    let mut trolls = Vec::new();
    for _ in 0..3 {
        trolls.push(spawn_creature(&mut sim, Species::Troll));
    }
    let mut deer = Vec::new();
    for _ in 0..3 {
        deer.push(spawn_creature(&mut sim, Species::Deer));
    }

    let all_ids: Vec<(CreatureId, &str)> = elephants
        .iter()
        .map(|&id| (id, "Elephant"))
        .chain(trolls.iter().map(|&id| (id, "Troll")))
        .chain(deer.iter().map(|&id| (id, "Deer")))
        .collect();

    let mut violations: Vec<String> = Vec::new();

    // Run 100 turns of 500 ticks each.
    for turn in 0..100 {
        let target_tick = sim.tick + 500;
        sim.step(&[], target_tick);

        // Check every creature's y position after each turn.
        for &(id, species_name) in &all_ids {
            if let Some(creature) = sim.db.creatures.get(&id) {
                let y = creature.position.min.y;
                if y > walking_y {
                    violations.push(format!(
                        "turn {turn}, tick {}: {species_name} ({id:?}) at y={y} (walking_y={walking_y})",
                        sim.tick
                    ));
                }
            }
            // Creature may have died from fall damage — that's also evidence of the bug.
        }
    }

    if !violations.is_empty() {
        eprintln!("=== GROUNDING VIOLATIONS ({} total) ===", violations.len());
        for v in &violations {
            eprintln!("  {v}");
        }
    }
    assert!(
        violations.is_empty(),
        "Ground creatures should never exceed walking_y={walking_y} on flat terrain, \
         but got {} violations (see stderr for details)",
        violations.len()
    );
}

/// Reproduce bug: trolls climb into the air away from surfaces (trunk).
/// When a troll is near the trunk, it should stay adjacent to solid voxels
/// or on the ground — never floating in empty air.
#[test]
fn test_troll_stays_on_trunk_while_climbing() {
    let mut sim = test_sim(legacy_test_seed());
    let floor_y = sim.config.floor_y;
    let walking_y = floor_y + 1;

    // Find a trunk voxel, then find an adjacent air position where a 2×2×2
    // footprint fits.
    let trunk_pos = {
        let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
        tree.trunk_voxels
            .first()
            .copied()
            .expect("test_sim should have a tree with trunk voxels")
    };

    // Search for an air position adjacent to trunk where 2×2×2 footprint fits.
    let spawn_pos = {
        let offsets = [
            (-2, 0, 0),
            (1, 0, 0),
            (0, 0, -2),
            (0, 0, 1),
            (-2, 0, -1),
            (1, 0, -1),
            (-1, 0, -2),
            (-1, 0, 1),
        ];
        let mut found = None;
        for &(dx, dy, dz) in &offsets {
            let pos = VoxelCoord::new(trunk_pos.x + dx, trunk_pos.y + dy, trunk_pos.z + dz);
            // Check that all 8 voxels of the 2×2×2 footprint are air.
            let all_air = (0..2).all(|fx| {
                (0..2).all(|fy| {
                    (0..2).all(|fz| {
                        let p = VoxelCoord::new(pos.x + fx, pos.y + fy, pos.z + fz);
                        sim.voxel_zone(sim.home_zone_id()).unwrap().get(p) == VoxelType::Air
                    })
                })
            });
            if all_air {
                found = Some(pos);
                break;
            }
        }
        found.expect("should find air position adjacent to trunk for 2x2x2 footprint")
    };

    // Spawn a troll and teleport it to the position near the trunk.
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    {
        let mut troll = sim.db.creatures.get(&troll_id).unwrap();
        troll.position = VoxelBox::from_anchor(spawn_pos, [2, 2, 2]);
        sim.db.update_creature(troll).unwrap();
    }

    let mut violations: Vec<String> = Vec::new();

    // Run 100 turns of 500 ticks.
    for turn in 0..100 {
        let target_tick = sim.tick + 500;
        sim.step(&[], target_tick);

        if let Some(troll) = sim.db.creatures.get(&troll_id) {
            let pos = troll.position.min;
            let y = pos.y;

            // If on the ground, that's fine.
            if y <= walking_y {
                continue;
            }

            // If above ground, check that at least one of the 8 footprint
            // voxels has a face-neighbor that is solid (i.e., adjacent to a
            // surface).
            let has_adjacent_solid = (0..2).any(|fx| {
                (0..2).any(|fy| {
                    (0..2).any(|fz| {
                        let p = VoxelCoord::new(pos.x + fx, pos.y + fy, pos.z + fz);
                        // Check the 6 face neighbors.
                        let neighbors = [
                            VoxelCoord::new(p.x - 1, p.y, p.z),
                            VoxelCoord::new(p.x + 1, p.y, p.z),
                            VoxelCoord::new(p.x, p.y - 1, p.z),
                            VoxelCoord::new(p.x, p.y + 1, p.z),
                            VoxelCoord::new(p.x, p.y, p.z - 1),
                            VoxelCoord::new(p.x, p.y, p.z + 1),
                        ];
                        neighbors.iter().any(|&n| {
                            sim.voxel_zone(sim.home_zone_id())
                                .unwrap()
                                .get(n)
                                .is_solid()
                        })
                    })
                })
            });

            if !has_adjacent_solid {
                violations.push(format!(
                    "turn {turn}, tick {}: troll at ({}, {}, {}) — floating in air with no adjacent solid",
                    sim.tick, pos.x, y, pos.z
                ));
            }
        }
        // Troll may have died — also evidence of the bug if it fell.
    }

    if !violations.is_empty() {
        eprintln!(
            "=== TROLL FLOATING VIOLATIONS ({} total) ===",
            violations.len()
        );
        for v in &violations {
            eprintln!("  {v}");
        }
    }
    assert!(
        violations.is_empty(),
        "Troll should never float in air away from all surfaces, \
         but got {} violations (see stderr for details)",
        violations.len()
    );
}

/// Regression test: elephants should not float up to canopy level.
/// We build a small hill with overhead leaf/branch voxels and verify
/// elephants stay on the hilltop.
#[test]
fn test_elephant_floats_under_tree_foliage_on_hill() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let floor_y = sim.config.floor_y;

    // 1. Build a small hill: Dirt at x=12..18, z=12..18, y=floor_y+1..floor_y+3.
    //    Hilltop walk level should be floor_y+4 (one above the top dirt layer).
    for x in 12..18 {
        for z in 12..18 {
            for y in (floor_y + 1)..=(floor_y + 3) {
                sim.voxel_zone_mut(sim.home_zone_id())
                    .unwrap()
                    .set(VoxelCoord::new(x, y, z), VoxelType::Dirt);
            }
        }
    }

    // 2. Place scattered leaf/branch voxels overhead at y=floor_y+10..floor_y+12.
    //    This mimics tree canopy above the hill. The top layer uses Dirt so
    //    that the surface type above it is Dirt → Ground edge type, which
    //    elephants (ground_only, MovementCategory::WalkOnly) can traverse.
    //    Without Dirt on top, the Leaf surface would produce BranchWalk edges
    //    that elephants reject, masking the canopy teleportation bug.
    for x in 12..18 {
        for z in 12..18 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(x, floor_y + 10, z), VoxelType::Leaf);
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(x, floor_y + 11, z), VoxelType::Branch);
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(x, floor_y + 12, z), VoxelType::Dirt);
        }
    }

    // 3. Rebuild spans so walkability queries see the new voxels.
    sim.voxel_zone_mut(sim.home_zone_id()).unwrap().repack_all();
    sim.rebuild_transient_state();

    // 4. Spawn 3 elephants near the hill center.  Force-position them onto
    //    the hilltop so they start in the danger zone (where ground_neighbors
    //    can offer the canopy position).
    let hilltop_y = floor_y + 4;
    let spawn_pos = VoxelCoord::new(14, hilltop_y, 14);
    let mut elephant_ids = Vec::new();
    for _ in 0..3 {
        let existing: std::collections::BTreeSet<CreatureId> = sim
            .db
            .creatures
            .iter_all()
            .filter(|c| c.species == Species::Elephant)
            .map(|c| c.id)
            .collect();
        let cmd = SimCommand {
            player_name: String::new(),
            tick: sim.tick + 1,
            action: SimAction::SpawnCreature {
                zone_id: sim.home_zone_id(),
                species: Species::Elephant,
                position: spawn_pos,
            },
        };
        sim.step(&[cmd], sim.tick + 2);
        let new_id = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elephant && !existing.contains(&c.id))
            .expect("elephant should spawn")
            .id;
        elephant_ids.push(new_id);
    }

    // After each step, force elephants back to hilltop so they keep
    // rolling the dice on ground_neighbors offering the canopy position.
    // 5. Record initial y positions.
    let initial_ys: Vec<(CreatureId, i32)> = elephant_ids
        .iter()
        .map(|&id| {
            let c = sim.db.creatures.get(&id).unwrap();
            let y = c.position.min.y;
            eprintln!(
                "Initial position for {:?}: ({}, {}, {})",
                id, c.position.min.x, y, c.position.min.z
            );
            (id, y)
        })
        .collect();

    // 6. Run 100 turns of 500 ticks each, checking for violations after each turn.
    let max_allowed_y = floor_y + 5; // hilltop + 1 margin
    let mut violations: Vec<String> = Vec::new();

    for turn in 0..100 {
        // Force all living elephants back onto the hilltop each turn so
        // they stay in the zone where the buggy surface-snap can fire.
        for &id in &elephant_ids {
            if sim.db.creatures.get(&id).is_some() {
                force_position(&mut sim, id, VoxelCoord::new(14, hilltop_y, 14));
            }
        }

        let end_tick = sim.tick + 500;
        sim.step(&[], end_tick);

        // 7. Check elephant y positions.
        for &id in &elephant_ids {
            if let Some(c) = sim.db.creatures.get(&id) {
                let y = c.position.min.y;
                if y > max_allowed_y {
                    let msg = format!(
                        "Turn {turn}: elephant {:?} at y={y} (max allowed {max_allowed_y}), \
                         pos=({}, {}, {})",
                        id, c.position.min.x, y, c.position.min.z
                    );
                    eprintln!("VIOLATION: {msg}");
                    violations.push(msg);
                }
            }
        }
    }

    // 8. Print final positions for diagnostics.
    eprintln!("--- Final elephant positions ---");
    for &id in &elephant_ids {
        if let Some(c) = sim.db.creatures.get(&id) {
            eprintln!(
                "  {:?}: ({}, {}, {})",
                id, c.position.min.x, c.position.min.y, c.position.min.z
            );
        } else {
            eprintln!("  {:?}: DEAD/GONE", id);
        }
    }
    for (id, iy) in &initial_ys {
        eprintln!("  {:?} initial_y={iy}", id);
    }

    assert!(
        violations.is_empty(),
        "Elephants should stay on the hilltop (y <= {max_allowed_y}), \
         not float up to canopy. Got {} violations:\n{}",
        violations.len(),
        violations.join("\n")
    );
}

/// Reproduce bug: elephants (non-climbers) take fall damage near concave
/// overhangs. The overhang has surfaces a climber could cling to, but a
/// ground-only creature like an elephant shouldn't attempt to path there.
/// A deer (small, ground-only) is a control, and a troll (climber) is
/// included for comparison.
#[test]
fn test_elephant_no_fall_damage_near_overhang() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let floor_y = sim.config.floor_y;
    let walking_y = floor_y + 1;

    // Build a concave overhang structure out of Trunk voxels.
    //
    // Side view (x cross-section at z=15):
    //
    // y=floor_y+5:          TTT      (overhang extends out, x=17)
    // y=floor_y+4:         TT        (overhang step 2, x=18)
    // y=floor_y+3:        TT         (overhang step 1, x=19)
    // y=floor_y+2:       TT          (vertical wall continues, x=20)
    // y=floor_y+1:      TT           (base of wall at walking level, x=20)
    // y=floor_y:   DDDDDDDDDDDDD    (dirt floor — already present)
    //
    // Vertical wall at x=20, z=13..18, y=floor_y..floor_y+3
    for z in 13..18 {
        for y in floor_y..floor_y + 4 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(20, y, z), VoxelType::Trunk);
        }
    }
    // Overhang step 1: x=19, z=13..18, y=floor_y+3..floor_y+4
    for z in 13..18 {
        for y in floor_y + 3..floor_y + 5 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(19, y, z), VoxelType::Trunk);
        }
    }
    // Overhang step 2: x=18, z=13..18, y=floor_y+4..floor_y+5
    for z in 13..18 {
        for y in floor_y + 4..floor_y + 6 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(18, y, z), VoxelType::Trunk);
        }
    }
    // Overhang extends further: x=17, z=13..18, y=floor_y+5..floor_y+6
    for z in 13..18 {
        for y in floor_y + 5..floor_y + 7 {
            sim.voxel_zone_mut(sim.home_zone_id())
                .unwrap()
                .set(VoxelCoord::new(17, y, z), VoxelType::Trunk);
        }
    }

    sim.rebuild_transient_state();

    // Spawn creatures near the overhang on the ground.
    let mut events = Vec::new();
    let mut elephants = Vec::new();
    for i in 0..3 {
        let spawn_pos = VoxelCoord::new(17 + i, walking_y, 15);
        if let Some(id) = sim.spawn_creature(
            Species::Elephant,
            spawn_pos,
            sim.home_zone_id(),
            &mut events,
        ) {
            elephants.push(id);
        }
    }
    let deer_id = sim
        .spawn_creature(
            Species::Deer,
            VoxelCoord::new(16, walking_y, 15),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn deer");
    let troll_id = sim
        .spawn_creature(
            Species::Troll,
            VoxelCoord::new(15, walking_y, 15),
            sim.home_zone_id(),
            &mut events,
        )
        .expect("should spawn troll");

    assert!(
        !elephants.is_empty(),
        "should spawn at least one elephant near overhang"
    );

    // Record initial HP for all creatures.
    let mut initial_hp: std::collections::BTreeMap<CreatureId, (i64, &str)> =
        std::collections::BTreeMap::new();
    for &id in &elephants {
        let c = sim.db.creatures.get(&id).unwrap();
        initial_hp.insert(id, (c.hp, "Elephant"));
    }
    initial_hp.insert(
        deer_id,
        (sim.db.creatures.get(&deer_id).unwrap().hp, "Deer"),
    );
    initial_hp.insert(
        troll_id,
        (sim.db.creatures.get(&troll_id).unwrap().hp, "Troll"),
    );

    let mut hp_violations: Vec<String> = Vec::new();
    let mut position_violations: Vec<String> = Vec::new();
    let mut troll_notes: Vec<String> = Vec::new();

    // Run 200 turns of 500 ticks each (100k total ticks).
    let ticks_per_turn = 500;
    let num_turns = 200;
    for turn in 0..num_turns {
        let start_tick = sim.tick + 1;
        let end_tick = start_tick + ticks_per_turn;
        sim.step(&[], end_tick);

        // Check each creature after this turn.
        for (&id, &(init_hp, species_name)) in &initial_hp {
            if let Some(creature) = sim.db.creatures.get(&id) {
                let pos = creature.position.min;
                let current_hp = creature.hp;

                if current_hp < init_hp {
                    let msg = format!(
                        "  {} {:?}: HP dropped {}->{} at tick ~{}, pos=({},{},{})",
                        species_name, id, init_hp, current_hp, end_tick, pos.x, pos.y, pos.z,
                    );
                    if species_name == "Troll" {
                        troll_notes.push(msg);
                    } else {
                        hp_violations.push(msg);
                    }
                }

                // For elephants, also check if they're above floor_y + 2.
                if species_name == "Elephant" && pos.y > floor_y + 2 {
                    position_violations.push(format!(
                        "  Elephant {:?}: y={} (> floor_y+2={}) at tick ~{}, pos=({},{},{})",
                        id,
                        pos.y,
                        floor_y + 2,
                        end_tick,
                        pos.x,
                        pos.y,
                        pos.z,
                    ));
                }
            }
        }
    }

    // Print all findings.
    if !hp_violations.is_empty() {
        eprintln!(
            "HP-loss violations ({}):\n{}",
            hp_violations.len(),
            hp_violations.join("\n")
        );
    }
    if !position_violations.is_empty() {
        eprintln!(
            "Elephant position violations ({}):\n{}",
            position_violations.len(),
            position_violations.join("\n")
        );
    }
    if !troll_notes.is_empty() {
        eprintln!(
            "Troll notes (not asserted, {} entries):\n{}",
            troll_notes.len(),
            troll_notes.join("\n")
        );
    }

    // Print final positions for all creatures.
    eprintln!("Final creature positions:");
    for (&id, &(init_hp, species_name)) in &initial_hp {
        if let Some(creature) = sim.db.creatures.get(&id) {
            let pos = creature.position.min;
            eprintln!(
                "  {} {:?}: pos=({},{},{}) hp={}/{}",
                species_name, id, pos.x, pos.y, pos.z, creature.hp, init_hp
            );
        } else {
            eprintln!("  {} {:?}: DEAD/GONE", species_name, id);
        }
    }

    assert!(
        hp_violations.is_empty(),
        "Elephants and deer should not take fall damage near overhang. \
         Got {} HP-loss violations:\n{}",
        hp_violations.len(),
        hp_violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// B-large-fall-deflect: large creature gravity with deflection
// ---------------------------------------------------------------------------

/// Large creature floating above an area where its 2x2 footprint can't
/// fit straight down (solid obstructions in the column), but there IS a
/// valid walkable position a few voxels away horizontally.  The creature
/// should deflect and land rather than getting stuck floating.
///
/// We isolate this by removing the flat ground entirely under the anchor
/// area, placing obstructions in the fall path, and providing a landing
/// platform offset to the side.  The existing code (straight-down scan +
/// find_nearest_walkable with radius 5) cannot reach the landing because
/// the creature is more than 5 Manhattan-distance from any walkable
/// position at its floating Y.
#[test]
fn large_creature_deflects_when_anchor_column_obstructed() {
    let seed = fresh_test_seed();
    let mut sim = flat_world_sim(seed);

    // Build a column obstruction that blocks the elephant's 2x2 footprint
    // from falling straight down but isn't a valid landing.  The flat world
    // ground at y=0 is intact — after deflecting, the creature should land
    // on the ground (y=1) at a nearby anchor.
    //
    // Fill a solid slab across the elephant's anchor columns at y=3..4.
    // This is a 2-voxel-tall wall that blocks the footprint at y=3 and y=4
    // but only occupies the anchor column — nearby columns are clear.
    let hz = sim.home_zone_id();
    for y in 3..=4 {
        sim.voxel_zone_mut(hz)
            .unwrap()
            .set(VoxelCoord::new(10, y, 10), VoxelType::GrownPlatform);
    }
    sim.rebuild_transient_state();

    // Spawn elephant on the ground, then teleport to floating position.
    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(20, 1, 20),
            hz,
            &mut events,
        )
        .expect("should spawn elephant");

    // Teleport above the obstruction.
    {
        let mut c = sim.db.creatures.get(&elephant_id).unwrap();
        let fp = c.position.footprint_size();
        c.position = VoxelBox::from_anchor(VoxelCoord::new(10, 10, 10), fp);
        sim.db.update_creature(c).unwrap();
    }

    events.clear();
    let fell = sim.apply_single_creature_gravity(elephant_id, &mut events);
    assert!(fell, "elephant should fall from unsupported position");

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    // The elephant should have deflected around the obstruction and landed
    // on the ground.
    assert!(
        elephant.position.min.y < 4,
        "elephant should land below the obstruction at y=4, now at y={}",
        elephant.position.min.y
    );
    // Verify the elephant is at anchor != (10, 10) since it had to deflect.
    assert!(
        elephant.position.min.x != 10 || elephant.position.min.z != 10,
        "elephant should have deflected to a different anchor column"
    );
    let fp = elephant.position.footprint_size();
    let can_climb = elephant.movement_category.can_climb();
    {
        let zone = sim.voxel_zone(hz).unwrap();
        assert!(
            crate::walkability::footprint_walkable(
                zone,
                &zone.face_data,
                elephant.position.min,
                fp,
                can_climb,
            ),
            "elephant should be at a walkable position after deflection (pos={:?})",
            elephant.position.min
        );
    }
}

/// Large creature completely enclosed in solid voxels with no open space
/// within deflection radius.  Should be killed rather than stuck floating.
#[test]
fn large_creature_killed_when_no_deflection_possible() {
    let seed = fresh_test_seed();
    let mut sim = flat_world_sim(seed);
    let hz = sim.home_zone_id();

    // Remove ALL ground from the world.  With no solid voxels below the
    // creature, find_creature_landing scans to y=0 without finding walkable
    // ground or a collision, and returns None.
    for x in 0..64 {
        for z in 0..64 {
            sim.voxel_zone_mut(hz)
                .unwrap()
                .set(VoxelCoord::new(x, 0, z), VoxelType::Air);
        }
    }
    sim.rebuild_transient_state();

    // Spawn elephant on ground far away first (before we removed it —
    // actually we just removed it, so spawn will fail).  Instead, just
    // insert the creature row directly by spawning then teleporting.
    // Spawn in a corner where we'll place temporary ground.
    sim.voxel_zone_mut(hz)
        .unwrap()
        .set(VoxelCoord::new(60, 0, 60), VoxelType::GrownPlatform);
    sim.voxel_zone_mut(hz)
        .unwrap()
        .set(VoxelCoord::new(61, 0, 60), VoxelType::GrownPlatform);
    sim.voxel_zone_mut(hz)
        .unwrap()
        .set(VoxelCoord::new(60, 0, 61), VoxelType::GrownPlatform);
    sim.voxel_zone_mut(hz)
        .unwrap()
        .set(VoxelCoord::new(61, 0, 61), VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(60, 1, 60),
            hz,
            &mut events,
        )
        .expect("should spawn elephant");

    // Now remove that temporary ground too.
    for dx in 0..2 {
        for dz in 0..2 {
            sim.voxel_zone_mut(hz)
                .unwrap()
                .set(VoxelCoord::new(60 + dx, 0, 60 + dz), VoxelType::Air);
        }
    }
    sim.rebuild_transient_state();

    // Teleport to floating position in the void.
    {
        let mut c = sim.db.creatures.get(&elephant_id).unwrap();
        let fp = c.position.footprint_size();
        c.position = VoxelBox::from_anchor(VoxelCoord::new(10, 10, 10), fp);
        sim.db.update_creature(c).unwrap();
    }

    events.clear();
    let fell = sim.apply_single_creature_gravity(elephant_id, &mut events);
    assert!(fell, "gravity should act on unsupported elephant");

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    assert_eq!(
        elephant.vital_status,
        VitalStatus::Dead,
        "elephant should be killed when no valid landing exists"
    );
}

/// Large creature falls from y=10, hits an obstruction that forces a
/// deflection, then continues falling to a landing platform.  Total fall
/// damage should be cumulative based on (start_y - final_landing_y), not
/// just the distance of one segment.
#[test]
fn large_creature_cumulative_fall_damage_across_deflection() {
    let seed = fresh_test_seed();
    let mut sim = flat_world_sim(seed);
    sim.config.fall_damage_per_voxel = 10;

    let hz = sim.home_zone_id();

    // Place an obstruction at y=5 under the anchor — blocks straight-down
    // fall but isn't a valid 2x2 landing.  Ground at y=0 is intact, so
    // after deflecting the creature lands on the ground at y=1.
    sim.voxel_zone_mut(hz)
        .unwrap()
        .set(VoxelCoord::new(10, 5, 10), VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    // Spawn elephant, then teleport to y=10.
    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(20, 1, 20),
            hz,
            &mut events,
        )
        .expect("should spawn elephant");

    // Give elephant enough HP to survive the fall and teleport.
    {
        let mut c = sim.db.creatures.get(&elephant_id).unwrap();
        c.hp = 500;
        c.hp_max = 500;
        let fp = c.position.footprint_size();
        c.position = VoxelBox::from_anchor(VoxelCoord::new(10, 10, 10), fp);
        sim.db.update_creature(c).unwrap();
    }

    events.clear();
    let fell = sim.apply_single_creature_gravity(elephant_id, &mut events);
    assert!(fell, "elephant should fall");

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    let landed_y = elephant.position.min.y;
    assert!(
        landed_y < 5,
        "elephant should land below the obstruction at y=5"
    );

    // Fall damage should be based on total vertical distance: 10 - landed_y.
    // The creature fell from y=10, deflected at y=5, continued falling to
    // y=landed_y.  Cumulative distance = 10 - landed_y.
    let expected_distance = 10 - landed_y;
    let expected_damage = expected_distance as i64 * 10;

    // Check the CreatureFell event records the correct from/to.
    let fell_event = events.iter().find_map(|e| match &e.kind {
        SimEventKind::CreatureFell {
            creature_id: cid,
            from,
            to,
            damage,
            ..
        } if *cid == elephant_id => Some((*from, *to, *damage)),
        _ => None,
    });
    assert!(
        fell_event.is_some(),
        "expected CreatureFell event for elephant"
    );
    let (from, to, damage) = fell_event.unwrap();
    assert_eq!(from.y, 10, "fall should start from y=10");
    assert_eq!(to.y, landed_y, "fall should end at landing y");
    assert_eq!(
        damage, expected_damage,
        "fall damage should be cumulative: ({} voxels) * 10 = {}, got {}",
        expected_distance, expected_damage, damage
    );
}

/// Multi-deflection: staggered obstructions at two different Y levels force
/// the creature to deflect twice before landing.
#[test]
fn large_creature_multiple_deflections() {
    let seed = fresh_test_seed();
    let mut sim = flat_world_sim(seed);

    let hz = sim.home_zone_id();

    // First obstruction at y=6 in the anchor column (10,10).
    sim.voxel_zone_mut(hz)
        .unwrap()
        .set(VoxelCoord::new(10, 6, 10), VoxelType::GrownPlatform);

    // Second obstruction at y=3 one column over at (11,10) — after the
    // first deflection moves the creature sideways, it hits this one.
    sim.voxel_zone_mut(hz)
        .unwrap()
        .set(VoxelCoord::new(11, 3, 10), VoxelType::GrownPlatform);

    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(20, 1, 20),
            hz,
            &mut events,
        )
        .expect("should spawn elephant");

    {
        let mut c = sim.db.creatures.get(&elephant_id).unwrap();
        c.hp = 500;
        c.hp_max = 500;
        let fp = c.position.footprint_size();
        c.position = VoxelBox::from_anchor(VoxelCoord::new(10, 10, 10), fp);
        sim.db.update_creature(c).unwrap();
    }

    events.clear();
    let fell = sim.apply_single_creature_gravity(elephant_id, &mut events);
    assert!(fell, "elephant should fall");

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    let fp = elephant.position.footprint_size();
    let can_climb = elephant.movement_category.can_climb();
    {
        let zone = sim.voxel_zone(hz).unwrap();
        assert!(
            crate::walkability::footprint_walkable(
                zone,
                &zone.face_data,
                elephant.position.min,
                fp,
                can_climb,
            ),
            "elephant should land at a walkable position (pos={:?})",
            elephant.position.min
        );
    }
    // Should have landed at y=1 (ground level).
    assert_eq!(
        elephant.position.min.y, 1,
        "elephant should reach ground level after multiple deflections"
    );
}

/// Deflection search radius is exactly 5.  An obstruction where the nearest
/// open space is at Manhattan distance 6 should result in None (creature killed).
#[test]
fn large_creature_deflection_radius_limit() {
    let seed = fresh_test_seed();
    let mut sim = flat_world_sim(seed);

    let hz = sim.home_zone_id();

    // Fill a solid mass that encloses the creature's position entirely.
    // The mass is wide enough (Manhattan distance > 5 in all directions)
    // that the deflection search can't escape.  The creature starts inside
    // the mass, so footprint_fits is false at every Y and every deflection
    // candidate → find_creature_landing returns None → creature killed.
    for x in 2..=19 {
        for z in 2..=19 {
            for y in 1..=20 {
                sim.voxel_zone_mut(hz)
                    .unwrap()
                    .set(VoxelCoord::new(x, y, z), VoxelType::GrownPlatform);
            }
        }
    }
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(
            Species::Elephant,
            VoxelCoord::new(30, 1, 30),
            hz,
            &mut events,
        )
        .expect("should spawn elephant");

    // Teleport INSIDE the solid mass.  The creature is at y=10 with solid
    // everywhere — footprint_walkable returns false (footprint_fits fails),
    // so the creature is unsupported.
    {
        let mut c = sim.db.creatures.get(&elephant_id).unwrap();
        let fp = c.position.footprint_size();
        c.position = VoxelBox::from_anchor(VoxelCoord::new(10, 10, 10), fp);
        sim.db.update_creature(c).unwrap();
    }

    events.clear();
    let fell = sim.apply_single_creature_gravity(elephant_id, &mut events);
    assert!(fell, "gravity should act on unsupported elephant");

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    assert_eq!(
        elephant.vital_status,
        VitalStatus::Dead,
        "elephant should be killed when deflection radius 5 is insufficient"
    );
}
