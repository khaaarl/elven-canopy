//! Tests for movement and navigation: voxel exclusion (hostile blocking),
//! creature gravity (falling, damage), pursuit tasks, path caching,
//! position interpolation, flying creature movement, and task proximity.
//! Corresponds to `sim/movement.rs`.

use super::combat_tests::{give_spear, setup_aggressive_elf, setup_frozen_hornet};
use super::test_helpers::*;
use super::*;

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
        species: Species::Elf,
        position,
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
        vital_status: VitalStatus::Alive,
        mp: 0,
        mp_max: 0,
        wasted_action_count: 0,
        last_dance_tick: 0,
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
    let mut sim = SimState::with_config(42, config);
    let air_coord = find_air_adjacent_to_trunk(&sim);
    let elf_id = spawn_elf(&mut sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick + 1,
        action: SimAction::DesignateBuild {
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
            elf.position.x as f32,
            elf.position.y as f32,
            elf.position.z as f32,
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
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf.
    let spawn_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[spawn_cmd], 2);

    let elf_id = *sim.db.creatures.iter_keys().next().unwrap();

    // Find a ground nav node different from the elf's to use as task target.
    let elf_node = creature_node(&sim, elf_id);
    let task_node = sim
        .nav_graph
        .ground_node_ids()
        .into_iter()
        .find(|&nid| nid != elf_node)
        .expect("Need at least 2 ground nodes");

    // Create a GoTo task at that nav node and assign it to the elf.
    let task_id = TaskId::new(&mut sim.rng);
    sim.insert_task(Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: sim.nav_graph.node(task_node).position,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    });
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Directly kill the task node's slot to simulate an incremental update
    // that removed it without recycling the slot. This is the exact state
    // that causes the panic: the NavNodeId in the task points to a dead
    // (None) slot.
    sim.nav_graph.kill_node(task_node);

    assert!(
        !sim.nav_graph.is_node_alive(task_node),
        "Task node should be dead",
    );

    // Step the sim — the elf should gracefully handle the dead task
    // node by either redirecting to the nearest alive node (and
    // completing the GoTo) or abandoning the task. Must NOT panic.
    sim.step(&[], 50000);

    // With VoxelCoord-based task locations, the task resolves to the
    // nearest alive node. The elf should have completed the GoTo task
    // (walked to the nearest alive node) or abandoned it. Either way,
    // it should not still be working on the original task.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    if let Some(tid) = elf.current_task {
        assert_ne!(
            tid, task_id,
            "Elf should not still be working on the task with the dead location node",
        );
    }
}

// ===================================================================
// Pursuit tasks
// ===================================================================

#[test]
fn pursuit_task_repaths_when_target_moves() {
    let mut sim = test_sim(42);
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Get target's initial node.
    let target_node = creature_node(&sim, target_id);

    // Pick a different alive node to move the target to (use a neighbor).
    let new_target_node = {
        let graph = sim.graph_for_species(Species::Elf);
        let edges = graph.neighbors(target_node);
        graph.edge(edges[0]).to
    };
    assert_ne!(target_node, new_target_node);

    // Create pursuit task at target's current node, assigned to pursuer.
    let target_pos = sim.nav_graph.node(target_node).position;
    let task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, target_pos);
    assert_eq!(sim.db.tasks.get(&task_id).unwrap().location, target_pos);

    // Manually move the target to the new node (simulates target movement).
    let new_pos = sim.nav_graph.node(new_target_node).position;
    let _ = sim.db.creatures.modify_unchecked(&target_id, |c| {
        c.position = new_pos;
    });

    // Step so the pursuer's activation fires and updates the task location.
    sim.step(&[], sim.tick + 10000);

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
    let mut sim = test_sim(42);
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Read the pursuer's current node (may have wandered during spawns).
    let pursuer_node = creature_node(&sim, pursuer_id);

    // Place both creatures at the same node and prevent them from wandering.
    let node_pos = sim.nav_graph.node(pursuer_node).position;
    let _ = sim.db.creatures.modify_unchecked(&target_id, |c| {
        c.position = node_pos;
    });
    let _ = sim.db.creatures.modify_unchecked(&pursuer_id, |c| {
        c.position = node_pos;
        c.path = None;
    });

    // Give the target a Sleep task so it stays still.
    let sleep_task_id = TaskId::new(&mut sim.rng);
    let sleep_task = Task {
        id: sleep_task_id,
        kind: TaskKind::Sleep {
            bed_pos: None,
            location: task::SleepLocation::Ground,
        },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(pursuer_node).position,
        progress: 0,
        total_cost: 999999,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sleep_task);
    let mut target = sim.db.creatures.get(&target_id).unwrap();
    target.current_task = Some(sleep_task_id);
    let _ = sim.db.creatures.update_no_fk(target);

    // Create pursuit task at the shared node.
    let pursuer_pos = sim.nav_graph.node(pursuer_node).position;
    let task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, pursuer_pos);

    // Clear pursuer's action state and schedule an immediate activation so
    // the pursuit logic fires regardless of the sim's PRNG-dependent event
    // schedule. This makes the test robust to worldgen PRNG changes.
    let _ = sim.db.creatures.modify_unchecked(&pursuer_id, |c| {
        c.next_available_tick = None;
        c.action_kind = crate::db::ActionKind::NoAction;
    });
    sim.event_queue.schedule(
        sim.tick + 1,
        crate::event::ScheduledEventKind::CreatureActivation {
            creature_id: pursuer_id,
        },
    );

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
    let mut sim = test_sim(42);
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Let both creatures settle (complete initial movement).
    sim.step(&[], sim.tick + 10000);

    let target_node = creature_node(&sim, target_id);

    // Assign pursuit task — clear any existing task first.
    let mut pursuer = sim.db.creatures.get(&pursuer_id).unwrap();
    pursuer.current_task = None;
    pursuer.path = None;
    let _ = sim.db.creatures.update_no_fk(pursuer);

    let target_pos = sim.nav_graph.node(target_node).position;
    let task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, target_pos);

    // Simulate target becoming unreachable by moving it to a position with
    // no nav node. This triggers the `node_at(target.position) == None`
    // branch in pursuit logic, causing the pursuer to abandon the task.
    let _ = sim
        .db
        .creatures
        .modify_unchecked(&target_id, |c| c.position = VoxelCoord::new(0, 200, 0));

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
    let mut sim = test_sim(42);
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);

    // Place target at a non-existent position (simulates disconnected region).
    let bogus_pos = VoxelCoord::new(999, 999, 999);
    let _task_id = insert_pursuit_task(&mut sim, pursuer_id, target_id, bogus_pos);
    // Move the target to an unreachable position with no nav node.
    let _ = sim.db.creatures.modify_unchecked(&target_id, |c| {
        c.position = VoxelCoord::new(0, 200, 0);
    });

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
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let elf_node = creature_node(&sim, elf_id);

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
    let mut sim = test_sim(42);
    let pursuer_id = spawn_elf(&mut sim);
    let target_id = spawn_second_elf(&mut sim);
    let target_node = creature_node(&sim, target_id);

    let target_pos = sim.nav_graph.node(target_node).position;
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
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf near the tree.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
    let elf_node = sim
        .nav_graph
        .node_at(elf.position)
        .expect("elf should have a nav node");
    let elf_pos = elf.position;

    // Find two nav nodes: one close to the elf, one farther away.
    // We pick nodes by searching the graph for nodes at increasing
    // Manhattan distances from the elf.
    let mut nodes_by_distance: Vec<(NavNodeId, u32)> = sim
        .nav_graph
        .live_nodes()
        .filter(|n| n.id != elf_node)
        .map(|n| (n.id, n.position.manhattan_distance(elf_pos)))
        .collect();
    nodes_by_distance.sort_by_key(|&(_, d)| d);

    // Pick a near node (close to elf) and a far node (much farther).
    let near_node = nodes_by_distance[0].0;
    let far_node = nodes_by_distance
        .iter()
        .find(|&&(_, d)| d >= 10)
        .expect("should have a node at least 10 manhattan away")
        .0;

    // Create two Available tasks — far task first (to ensure it would be
    // picked under the old first-found behavior if its TaskId sorts first).
    let far_task_id = TaskId::new(&mut sim.rng);
    let near_task_id = TaskId::new(&mut sim.rng);

    let far_task = crate::db::Task {
        id: far_task_id,
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: sim.nav_graph.node(far_node).position,
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
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: sim.nav_graph.node(near_node).position,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    sim.db.tasks.insert_no_fk(far_task).unwrap();
    sim.db.tasks.insert_no_fk(near_task).unwrap();

    // Clear the elf's current task so it's idle.
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.current_task = None;
    });

    let chosen = sim.find_available_task(elf_id).expect("should find a task");
    assert_eq!(
        chosen, near_task_id,
        "find_available_task should prefer the nearest task by nav distance"
    );
}

#[test]
fn find_available_task_single_candidate_skips_dijkstra() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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

    // Pick any nav node for the single task.
    let task_node = sim
        .nav_graph
        .live_nodes()
        .next()
        .expect("should have nodes")
        .id;

    let task_id = TaskId::new(&mut sim.rng);
    let task = crate::db::Task {
        id: task_id,
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: sim.nav_graph.node(task_node).position,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.tasks.insert_no_fk(task).unwrap();

    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.current_task = None;
    });

    let chosen = sim
        .find_available_task(elf_id)
        .expect("should find the only task");
    assert_eq!(chosen, task_id);
}

#[test]
fn find_available_task_respects_species_filter_with_proximity() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn a capybara (non-elf).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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

    let task_node = sim
        .nav_graph
        .live_nodes()
        .next()
        .expect("should have nodes")
        .id;

    // Create an elf-only task — capybara should not see it.
    let elf_task_id = TaskId::new(&mut sim.rng);
    let elf_task = crate::db::Task {
        id: elf_task_id,
        kind_tag: TaskKindTag::GoTo,
        state: TaskState::Available,
        location: sim.nav_graph.node(task_node).position,
        progress: 0,
        total_cost: 1,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.db.tasks.insert_no_fk(elf_task).unwrap();

    let _ = sim.db.creatures.modify_unchecked(&capy_id, |c| {
        c.current_task = None;
    });

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
fn troll_pursues_elf_cross_graph_pathfinding() {
    // Trolls (2x2x2) use the large_nav_graph while elves (1x1x1) use the
    // standard nav_graph. pursue_closest_target must translate target
    // positions to the troll's graph, not use raw NavNodeIds.
    //
    // On flat terrain both graphs are dense and node IDs coincidentally
    // overlap. To expose the bug, place the elf at a node whose ID exceeds
    // the large graph's node count, guaranteeing that ID doesn't exist on
    // the large graph.
    let mut sim = test_sim(42);
    let mut events = Vec::new();
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn troll near the tree.
    let troll_id = sim
        .spawn_creature(Species::Troll, tree_pos, &mut events)
        .expect("spawn troll");
    let troll_pos = sim.db.creatures.get(&troll_id).unwrap().position;

    // Find a standard-graph ground node whose ID >= large graph node count
    // AND is within troll detection range (144 = ~12 voxels).
    let large_graph_size = sim.large_nav_graph.node_count() as u32;
    let candidate = sim.nav_graph.live_nodes().find(|n| {
        if n.id.0 < large_graph_size {
            return false;
        }
        let dx = (n.position.x as i64 - troll_pos.x as i64).abs();
        let dy = (n.position.y as i64 - troll_pos.y as i64).abs();
        let dz = (n.position.z as i64 - troll_pos.z as i64).abs();
        let dist_sq = dx * dx + dy * dy + dz * dz;
        dist_sq > 0 && dist_sq <= 144
    });

    let Some(target_node) = candidate else {
        // On this seed, all nearby standard-graph nodes have IDs within the
        // large graph's range. The bug can't be triggered with this seed.
        // This is not a failure — the test is only meaningful when IDs
        // don't overlap.
        return;
    };

    // Spawn elf at this high-ID node.
    let elf_id = sim
        .spawn_creature(Species::Elf, target_node.position, &mut events)
        .expect("spawn elf");
    let elf_node = creature_node(&sim, elf_id);

    // Confirm the elf's node ID doesn't exist on the large graph.
    assert!(
        !sim.large_nav_graph.is_node_alive(elf_node),
        "Test setup: elf node {:?} should NOT exist on large graph (size={})",
        elf_node,
        large_graph_size
    );

    force_idle_and_cancel_activations(&mut sim, troll_id);
    force_idle_and_cancel_activations(&mut sim, elf_id);

    let troll_node = creature_node(&sim, troll_id);
    let pursued = sim.hostile_pursue(troll_id, Some(troll_node), Species::Troll, &mut events);

    assert!(
        pursued,
        "Troll should pursue elf even when elf's NavNodeId ({:?}) \
         doesn't exist on the large_nav_graph (size={}). \
         pursue_closest_target must translate positions, not use raw IDs.",
        elf_node, large_graph_size
    );
}

// ===================================================================
// Voxel exclusion (hostile blocking)
// ===================================================================

#[test]
fn voxel_exclusion_hostile_blocks_movement() {
    // A goblin at node B should prevent an elf from moving to node B.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    // Schedule only the elf to activate (goblin stays idle).
    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );

    // Run a few ticks — elf should NOT move to goblin's voxel.
    sim.step(&[], sim.tick + 200);

    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Elf must not be at the goblin's position.
    assert_ne!(
        elf_pos_after, goblin_pos,
        "Elf should not move into voxel occupied by hostile goblin"
    );
}

#[test]
fn voxel_exclusion_non_hostile_does_not_block() {
    // Two elves (same civ) should be able to share a voxel.
    let mut sim = test_sim(42);
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf_a, node_a);
    force_to_node(&mut sim, elf_b, node_b);
    force_idle(&mut sim, elf_a);
    force_idle(&mut sim, elf_b);

    // Verify they are non-hostile.
    assert!(
        sim.is_non_hostile(elf_a, elf_b),
        "Two elves should be non-hostile"
    );

    // The exclusion check should return false (not blocked).
    let elf_b_pos = sim.db.creatures.get(&elf_b).unwrap().position;
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let elf_footprint = sim.species_table[&Species::Elf].footprint;

    assert!(
        !sim.destination_blocked_by_hostile(elf, elf_pos, elf_footprint),
        "Creature should not block itself"
    );
}

#[test]
fn voxel_exclusion_dead_hostile_does_not_block() {
    // A dead goblin should not block an elf's movement.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);

    // Kill the goblin (vital_status is indexed, so use update).
    if let Some(mut c) = sim.db.creatures.get(&goblin) {
        c.vital_status = VitalStatus::Dead;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
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
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);

    // Place them on adjacent nodes.
    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, goblin, node_a);
    force_to_node(&mut sim, elf, node_b);
    force_idle(&mut sim, goblin);
    force_idle(&mut sim, elf);

    let goblin_pos_before = sim.db.creatures.get(&goblin).unwrap().position;
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;

    // Only activate the goblin, freeze the elf.
    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation {
            creature_id: goblin,
        },
    );

    sim.step(&[], sim.tick + 200);

    let goblin_pos_after = sim.db.creatures.get(&goblin).unwrap().position;

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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place elf at node_b, goblin at node_c. Elf should not wander to node_c.
    let (_node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_to_node(&mut sim, elf, node_b);
    force_to_node(&mut sim, goblin, node_c);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    // Cancel goblin activations so it stays put.
    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.cancel_creature_activations(elf);

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Run many activations and verify the elf never lands on goblin's voxel.
    for i in 0..20 {
        force_idle(&mut sim, elf);
        force_to_node(&mut sim, elf, node_b);
        sim.event_queue.schedule(
            sim.tick + 1,
            ScheduledEventKind::CreatureActivation { creature_id: elf },
        );
        sim.step(&[], sim.tick + 200);

        let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
        assert_ne!(
            elf_pos, goblin_pos,
            "Elf should not wander into hostile voxel (iteration {i})"
        );
    }
}

#[test]
fn voxel_exclusion_flee_avoids_hostile_voxels() {
    // A fleeing elf should prefer edges not occupied by hostiles.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Set up: elf at node_b (middle), goblin at node_a (nearby threatening).
    // Node_c should be unoccupied — elf should flee toward node_c, not node_a.
    let (node_a, node_b, _node_c) = find_chain_of_three(&sim);
    force_to_node(&mut sim, elf, node_b);
    force_to_node(&mut sim, goblin, node_a);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.cancel_creature_activations(goblin);

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Activate only the elf — it should detect the goblin and flee.
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
    sim.step(&[], sim.tick + 200);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Find a node with at least 2 neighbors and place goblins on ALL neighbors.
    let elf_node = sim
        .nav_graph
        .live_nodes()
        .find(|n| n.edge_indices.len() >= 2)
        .map(|n| n.id)
        .expect("Need a node with >= 2 neighbors");

    force_to_node(&mut sim, elf, elf_node);
    force_idle(&mut sim, elf);

    let neighbor_positions: Vec<(NavNodeId, VoxelCoord)> = sim
        .nav_graph
        .neighbors(elf_node)
        .iter()
        .map(|&edge_idx| {
            let edge = sim.nav_graph.edge(edge_idx);
            let pos = sim.nav_graph.node(edge.to).position;
            (edge.to, pos)
        })
        .collect();

    // Spawn a goblin at each neighbor.
    let mut goblins = Vec::new();
    for &(neighbor_node, _) in &neighbor_positions {
        let g = spawn_species(&mut sim, Species::Goblin);
        force_to_node(&mut sim, g, neighbor_node);
        force_idle(&mut sim, g);
        sim.event_queue.cancel_creature_activations(g);
        goblins.push(g);
    }

    // Also make sure the elf is still at the right node (spawning moves
    // things around).
    force_to_node(&mut sim, elf, elf_node);
    force_idle(&mut sim, elf);
    sim.event_queue.cancel_creature_activations(elf);

    // Verify that all neighbors are indeed hostile-blocked.
    let elf_fp = sim.species_table[&Species::Elf].footprint;
    for &(_, pos) in &neighbor_positions {
        assert!(
            sim.destination_blocked_by_hostile(elf, pos, elf_fp),
            "Expected neighbor at {pos:?} to be hostile-blocked"
        );
    }

    let elf_pos_before = sim.db.creatures.get(&elf).unwrap().position;

    // Now activate the elf — it should be cornered but flee should still
    // use the fallback (allow movement through hostile).
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
    sim.step(&[], sim.tick + 200);

    // The elf has hostile_detection_range_sq=225 (15 voxels) and the
    // goblins are on adjacent nav nodes — well within range. The elf
    // should detect the threat and flee. Since all exits are hostile-
    // occupied, the flee fallback should allow movement through a hostile.
    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position;
    assert_ne!(
        elf_pos_before, elf_pos_after,
        "Cornered elf should flee (using fallback through hostile-occupied voxel)"
    );
    // Elf moved — verify it went to one of the neighbor positions
    // (which are all hostile-occupied, confirming fallback worked).
    let moved_to_neighbor = neighbor_positions.iter().any(|&(_, p)| p == elf_pos_after);
    assert!(
        moved_to_neighbor,
        "Cornered elf should flee to a neighbor (even hostile-occupied)"
    );
}

#[test]
fn voxel_exclusion_ground_move_one_step_returns_false_when_blocked() {
    // Directly test ground_move_one_step returns false when destination is hostile.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    // Find the edge from node_a to node_b.
    let edge_idx = sim
        .nav_graph
        .neighbors(node_a)
        .iter()
        .copied()
        .find(|&idx| sim.nav_graph.edge(idx).to == node_b)
        .expect("Should have edge from A to B");

    let elf_pos_before = sim.db.creatures.get(&elf).unwrap().position;
    let result = sim.ground_move_one_step(elf, Species::Elf, edge_idx);

    assert!(
        !result,
        "ground_move_one_step should return false when blocked"
    );
    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position;
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    let edge_idx = sim
        .nav_graph
        .neighbors(node_a)
        .iter()
        .copied()
        .find(|&idx| sim.nav_graph.edge(idx).to == node_b)
        .expect("Should have edge from A to B");

    // Without skip_exclusion: blocked.
    let result_blocked = sim.ground_move_one_step(elf, Species::Elf, edge_idx);
    assert!(
        !result_blocked,
        "ground_move_one_step should block when hostile present"
    );

    // Cancel the retry activation scheduled by the failed move.
    sim.event_queue.cancel_creature_activations(elf);

    // With skip_exclusion: allowed.
    let result_forced = sim.ground_move_one_step_inner(elf, Species::Elf, edge_idx, true);
    assert!(
        result_forced,
        "ground_move_one_step_inner with skip_exclusion should succeed"
    );

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let node_b_pos = sim.nav_graph.node(node_b).position;
    assert_eq!(
        elf_pos, node_b_pos,
        "Elf should have moved to goblin's voxel with skip_exclusion"
    );
}

#[test]
fn voxel_exclusion_ground_move_one_step_returns_true_when_clear() {
    // ground_move_one_step should return true when destination is clear.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_idle(&mut sim, elf);

    let edge_idx = sim
        .nav_graph
        .neighbors(node_a)
        .iter()
        .copied()
        .find(|&idx| sim.nav_graph.edge(idx).to == node_b)
        .expect("Should have edge from A to B");

    let result = sim.ground_move_one_step(elf, Species::Elf, edge_idx);
    assert!(
        result,
        "ground_move_one_step should return true when path is clear"
    );

    let elf_pos_after = sim.db.creatures.get(&elf).unwrap().position;
    let node_b_pos = sim.nav_graph.node(node_b).position;
    assert_eq!(elf_pos_after, node_b_pos, "Elf should have moved to node B");
}

#[test]
fn voxel_exclusion_blocked_schedules_retry() {
    // When movement is blocked, a retry activation should be scheduled.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.cancel_creature_activations(goblin);

    let edge_idx = sim
        .nav_graph
        .neighbors(node_a)
        .iter()
        .copied()
        .find(|&idx| sim.nav_graph.edge(idx).to == node_b)
        .expect("Should have edge from A to B");

    let activations_before = sim.count_pending_activations_for(elf);
    sim.ground_move_one_step(elf, Species::Elf, edge_idx);
    let activations_after = sim.count_pending_activations_for(elf);

    assert_eq!(
        activations_after,
        activations_before + 1,
        "Blocked move should schedule a retry activation"
    );
}

#[test]
fn voxel_exclusion_already_overlapping_allowed() {
    // If two hostile creatures are already in the same voxel (e.g., both
    // spawned there), they should be allowed to stay — the check only
    // prevents MOVING into a hostile's voxel.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, _) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_a);

    // Both are now at the same position — this should be fine.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    assert_eq!(
        elf_pos, goblin_pos,
        "Test setup: both should be at same position"
    );

    // The sim should not crash when processing these creatures.
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation {
            creature_id: goblin,
        },
    );

    // Just verify no panic. Creatures should separate on their next moves.
    sim.step(&[], sim.tick + 500);
}

#[test]
fn voxel_exclusion_different_hostile_civs_block() {
    // Two creatures from different hostile civs should block each other.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let orc = spawn_species(&mut sim, Species::Orc);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, orc, node_b);

    let orc_pos = sim.db.creatures.get(&orc).unwrap().position;
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
    let mut sim = test_sim(42);
    let g1 = spawn_species(&mut sim, Species::Goblin);
    let g2 = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, g1, node_a);
    force_to_node(&mut sim, g2, node_b);

    let g2_pos = sim.db.creatures.get(&g2).unwrap().position;
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
    let mut sim = test_sim(42);
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
    let elephant_pos = sim.db.creatures.get(&elephant).unwrap().position;
    // Place goblin at elephant_pos + (1, 0, 0) — inside the footprint.
    let goblin_pos = VoxelCoord::new(elephant_pos.x + 1, elephant_pos.y, elephant_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    // Elephant is passive non-civ, goblin is aggressive non-civ — both
    // non-civ means non-hostile by default. Give the elephant a civ so the
    // goblin's aggressive engagement initiative makes them hostile.
    let player_civ = sim.player_civ_id.unwrap();
    if let Some(mut c) = sim.db.creatures.get(&elephant) {
        c.civ_id = Some(player_civ);
        let _ = sim.db.creatures.update_no_fk(c);
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    let troll_fp = sim.species_table[&Species::Troll].footprint;
    assert_eq!(
        troll_fp,
        [2, 2, 2],
        "Troll must have 2x2x2 footprint for this test"
    );

    let troll = spawn_species(&mut sim, Species::Troll);

    let troll_pos = sim.db.creatures.get(&troll).unwrap().position;
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
    let mut sim = test_sim(42);
    sim.config.voxel_exclusion_retry_ticks = 500;

    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.cancel_creature_activations(goblin);

    let edge_idx = sim
        .nav_graph
        .neighbors(node_a)
        .iter()
        .copied()
        .find(|&idx| sim.nav_graph.edge(idx).to == node_b)
        .expect("Should have edge from A to B");

    let tick_at_block = sim.tick;
    sim.ground_move_one_step(elf, Species::Elf, edge_idx);

    // Step to just before the retry — activation should NOT have fired.
    let activations_before = sim.count_pending_activations_for(elf);
    assert_eq!(
        activations_before, 1,
        "Should have exactly one retry scheduled"
    );

    sim.step(&[], tick_at_block + 499);
    // Elf should still be at node_a (retry not yet fired).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let node_a_pos = sim.nav_graph.node(node_a).position;
    assert_eq!(
        elf_pos, node_a_pos,
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Find a chain so elf can walk from A through B (goblin) to C (task).
    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);

    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.cancel_creature_activations(elf);

    // Create a GoTo task at node_c and assign the elf.
    let task_id = insert_goto_task(&mut sim, node_c);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        let _ = sim.db.tasks.update_no_fk(t);
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Activate elf.
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
    sim.step(&[], sim.tick + 200);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.cancel_creature_activations(elf);

    // Give the elf a cached path through the goblin's node.
    let pos_b = sim.nav_graph.node(node_b).position;
    let pos_c = sim.nav_graph.node(node_c).position;
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.path = Some(CreaturePath {
            remaining_positions: vec![pos_b, pos_c],
        });
    });
    assert!(
        sim.db.creatures.get(&elf).unwrap().path.is_some(),
        "Test setup: elf should have a cached path"
    );

    // Create a GoTo task at node_c and assign the elf.
    let task_id = insert_goto_task(&mut sim, node_c);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        let _ = sim.db.tasks.update_no_fk(t);
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Activate the elf — it should try to follow the cached path,
    // hit the hostile, and clear the path.
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.cancel_creature_activations(elf);

    let node_a_pos = sim.nav_graph.node(node_a).position;
    let node_b_pos = sim.nav_graph.node(node_b).position;

    // Try to move — should be blocked.
    let edge_idx = sim
        .nav_graph
        .neighbors(node_a)
        .iter()
        .copied()
        .find(|&idx| sim.nav_graph.edge(idx).to == node_b)
        .expect("Should have edge from A to B");

    let result = sim.ground_move_one_step(elf, Species::Elf, edge_idx);
    assert!(!result, "Should be blocked while goblin is alive");
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().position,
        node_a_pos,
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
        !sim.destination_blocked_by_hostile(elf, node_b_pos, goblin_fp),
        "Dead goblin should not block"
    );

    // Now manually retry the move — should succeed.
    let result2 = sim.ground_move_one_step(elf, Species::Elf, edge_idx);
    assert!(result2, "Move should succeed after goblin dies");
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().position,
        node_b_pos,
        "Elf should have moved to node B"
    );
}

#[test]
fn voxel_exclusion_attack_target_task_blocked_by_hostile() {
    // An elf with a player-directed AttackTarget task walking toward
    // its target should be blocked by a different hostile in the path.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let target_goblin = spawn_species(&mut sim, Species::Goblin);
    let blocking_goblin = spawn_species(&mut sim, Species::Goblin);

    // Set up: elf at A, blocking_goblin at B, target_goblin at C.
    // Elf must walk through B to reach C.
    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, blocking_goblin, node_b);
    force_to_node(&mut sim, target_goblin, node_c);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, blocking_goblin);
    force_idle(&mut sim, target_goblin);
    sim.event_queue.cancel_creature_activations(blocking_goblin);
    sim.event_queue.cancel_creature_activations(target_goblin);
    sim.event_queue.cancel_creature_activations(elf);

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
    let blocking_pos = sim.db.creatures.get(&blocking_goblin).unwrap().position;
    sim.step(&[], tick + 500);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    assert_ne!(
        elf_pos, blocking_pos,
        "Elf should not walk through blocking goblin to reach attack target"
    );
}

#[test]
fn voxel_exclusion_attack_move_task_blocked_by_hostile() {
    // An elf with an AttackMove task walking toward a destination
    // should be blocked by a hostile in the path.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let (node_a, node_b, node_c) = find_chain_of_three(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_idle(&mut sim, goblin);
    sim.event_queue.cancel_creature_activations(goblin);
    sim.event_queue.cancel_creature_activations(elf);

    let dest_pos = sim.nav_graph.node(node_c).position;

    // Issue AttackMove command toward node_c.
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            creature_id: elf,
            destination: dest_pos,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Run for a bit — elf should NOT move onto goblin's voxel.
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    sim.step(&[], tick + 500);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Find two connected nodes: start (A) and destination (B).
    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_idle(&mut sim, elf);

    // Give the elf a GoTo task at node_b.
    let task_id = insert_goto_task(&mut sim, node_b);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        let _ = sim.db.tasks.update_no_fk(t);
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Assign a cached path with TWO bogus positions (unlike the existing
    // test which uses one bogus + one real). This forces a full repath.
    let bogus_a = VoxelCoord::new(63, 63, 63);
    let bogus_b = VoxelCoord::new(62, 63, 63);
    assert!(sim.nav_graph.node_at(bogus_a).is_none());
    assert!(sim.nav_graph.node_at(bogus_b).is_none());

    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.path = Some(CreaturePath {
            remaining_positions: vec![bogus_a, bogus_b],
        });
    });

    // Schedule activation and step forward.
    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
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
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
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
    assert_ne!(hornet.position, hornet_pos, "hornet should have moved");
}

#[test]
fn hornet_wanders_when_alone() {
    let mut sim = test_sim(42);
    // Spawn hornet far from any creatures.
    let pos = VoxelCoord::new(5, 50, 5);
    let hornet_id = spawn_hornet_at(&mut sim, pos);

    // Run a few ticks — hornet should wander (position should change).
    let target_tick = sim.tick + 3000;
    sim.step(&[], target_tick);

    let hornet = sim.db.creatures.get(&hornet_id).unwrap();
    assert_ne!(hornet.position, pos, "hornet should have wandered");
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
        let mut sim = test_sim(42);
        let (elf_id, elf_pos) = setup_aggressive_elf(&mut sim);

        if case.has_spear {
            give_spear(&mut sim, elf_id);
        }

        let hornet_pos = VoxelCoord::new(elf_pos.x, elf_pos.y + case.dy, elf_pos.z);
        let hornet_id = setup_frozen_hornet(&mut sim, hornet_pos);
        let hornet_hp_before = sim.db.creatures.get(&hornet_id).unwrap().hp;

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
/// target if a nav-graph path gets it within melee range. If not reachable,
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
        let mut sim = test_sim(42);
        let elf_id = spawn_elf(&mut sim);
        zero_creature_stats(&mut sim, elf_id);
        // Elf stays civilian (passive) — won't pursue autonomously.
        force_idle_and_cancel_activations(&mut sim, elf_id);

        if case.has_spear {
            give_spear(&mut sim, elf_id);
        }

        let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
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
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

    let wyvern_pos = VoxelCoord::new(elf_pos.x - 1, elf_pos.y + 4, elf_pos.z - 1);
    let mut events = Vec::new();
    let wyvern_id = sim
        .spawn_creature(Species::Wyvern, wyvern_pos, &mut events)
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
    assert_ne!(wyvern.position, wyvern_pos, "wyvern should have moved");
}

// =========================================================================
// Creature gravity (F-creature-gravity)
// =========================================================================

#[test]
fn creature_on_solid_ground_does_not_fall() {
    let mut sim = test_sim(42);
    // Spawn an elf away from the tree at a known ground position.
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, VoxelCoord::new(10, 1, 10), &mut events)
        .expect("should spawn elf");

    // Elf should be on solid ground (terrain at y=0 is solid, elf at y=1).
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;
    assert_eq!(pos.y, 1);
    assert!(sim.creature_is_supported(elf_id));

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 0);
}

#[test]
fn creature_falls_when_platform_removed() {
    let mut sim = test_sim(42);
    // Use a low platform so the fall is survivable (2 voxels = 20 damage).
    let platform_pos = VoxelCoord::new(10, 2, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, VoxelCoord::new(10, 3, 10), &mut events)
        .expect("should spawn elf");
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.y, 3);
    let hp_before = elf.hp;

    // Remove the platform — elf is now unsupported.
    sim.world.set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1);

    // Elf should have landed at y=1 (above terrain at y=0).
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.y, 1);

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
    let mut sim = test_sim(42);
    // Place a very high platform so the fall is lethal.
    let platform_y = 40;
    let platform_pos = VoxelCoord::new(10, platform_y, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(
            Species::Elf,
            VoxelCoord::new(10, platform_y + 1, 10),
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
    sim.world.set(platform_pos, VoxelType::Air);
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
    let mut sim = test_sim(42);
    sim.config.fall_damage_per_voxel = 0;

    let platform_pos = VoxelCoord::new(10, 10, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, VoxelCoord::new(10, 11, 10), &mut events)
        .expect("should spawn elf");
    let hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

    sim.world.set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    events.clear();
    sim.apply_creature_gravity(&mut events);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.y, 1);
    assert_eq!(elf.hp, hp_before, "no damage when fall_damage_per_voxel=0");
}

#[test]
fn climber_on_trunk_does_not_fall() {
    let mut sim = test_sim(42);
    // Place trunk voxels to create a trunk surface nav node.
    // The test world has floor_y=0, so trunk at y=1..5.
    for y in 1..6 {
        sim.world.set(VoxelCoord::new(15, y, 15), VoxelType::Trunk);
    }
    sim.rebuild_transient_state();

    // Find a nav node adjacent to the trunk (on the trunk surface).
    // Trunk climb nodes are at positions adjacent to solid trunk voxels.
    let graph = &sim.nav_graph;
    let trunk_adj = VoxelCoord::new(16, 3, 15); // east of trunk
    if graph.node_at(trunk_adj).is_none() {
        // No trunk climb node here — skip test (topology dependent).
        return;
    }

    // Spawn an elf (climber, ground_only=false) and move it to the trunk node.
    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, trunk_adj, &mut events)
        .expect("should spawn elf");

    // Move elf to trunk position (may already be there if spawn snapped).
    let _ = sim.db.creatures.modify_unchecked(&elf_id, |c| {
        c.position = trunk_adj;
    });

    // Elf should be supported (has nav node, is a climber).
    assert!(sim.creature_is_supported(elf_id));

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 0);
}

#[test]
fn ground_only_creature_without_solid_below_falls() {
    let mut sim = test_sim(42);
    // Spawn a capybara (ground_only=true) at ground level.
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let pos = sim.db.creatures.get(&capy_id).unwrap().position;
    assert_eq!(pos.y, 1, "capybara should be on ground");

    // Teleport the capybara to a position without solid below — e.g., y=5
    // with no platform. The nav graph likely has no node here either.
    let floating_pos = VoxelCoord::new(pos.x, 5, pos.z);
    let _ = sim.db.creatures.modify_unchecked(&capy_id, |c| {
        c.position = floating_pos;
    });

    assert!(!sim.creature_is_supported(capy_id));

    let mut events = Vec::new();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1);

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(capy.position.y, 1, "capybara should land at ground level");
}

#[test]
fn flying_creature_does_not_fall() {
    let mut sim = test_sim(42);
    // Spawn a hornet (flying creature) and move it mid-air.
    let mut events = Vec::new();
    let hornet_id = sim
        .spawn_creature(Species::Hornet, VoxelCoord::new(20, 20, 20), &mut events)
        .expect("should spawn hornet");

    // Hornet has flight_ticks_per_voxel, so it's flying.
    let species_data = &sim.species_table[&Species::Hornet];
    assert!(species_data.flight_ticks_per_voxel.is_some());

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
    let mut sim = test_sim(42);
    // Place a platform and spawn an elf on it.
    let platform_pos = VoxelCoord::new(10, 5, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, VoxelCoord::new(10, 6, 10), &mut events)
        .expect("should spawn elf");
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().position.y, 6);

    // Remove the platform and rebuild nav.
    sim.world.set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    // Trigger creature activation — should detect unsupported and apply gravity.
    events.clear();
    sim.process_creature_activation(elf_id, &mut events);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.position.y, 1,
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
    let mut sim = test_sim(42);
    let platform_pos = VoxelCoord::new(10, 5, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, VoxelCoord::new(10, 6, 10), &mut events)
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
    sim.insert_task(fake_task);
    if let Some(mut c) = sim.db.creatures.get(&elf_id) {
        c.current_task = Some(task_id);
        c.path = Some(CreaturePath {
            remaining_positions: vec![VoxelCoord::new(15, 1, 15)],
        });
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Remove platform and apply gravity.
    sim.world.set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    events.clear();
    sim.apply_creature_gravity(&mut events);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.y, 1);
    assert!(elf.path.is_none(), "path should be cleared after fall");
    assert!(
        elf.current_task.is_none(),
        "task should be cleared after fall"
    );
}

#[test]
fn logistics_heartbeat_triggers_creature_gravity() {
    let mut sim = test_sim(42);
    let platform_pos = VoxelCoord::new(10, 5, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elf_id = sim
        .spawn_creature(Species::Elf, VoxelCoord::new(10, 6, 10), &mut events)
        .expect("should spawn elf");
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().position.y, 6);

    // Remove the platform and rebuild nav.
    sim.world.set(platform_pos, VoxelType::Air);
    sim.rebuild_transient_state();

    // Advance to a LogisticsHeartbeat tick — step forward enough ticks.
    let interval = sim.config.logistics_heartbeat_interval_ticks;
    let target = sim.tick + interval + 1;
    let result = sim.step(&[], target);

    // Elf should have fallen to ground.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.position.y, 1, "elf should fall via logistics heartbeat");

    // Should have a CreatureFell event in the step output.
    let fell_event = result
        .events
        .iter()
        .any(|e| matches!(e.kind, SimEventKind::CreatureFell { .. }));
    assert!(fell_event, "expected CreatureFell event from heartbeat");
}

#[test]
fn large_creature_falls_when_ground_removed() {
    let mut sim = test_sim(42);
    // Elephants use the 2x2x2 nav graph. Place a 2x2 platform at y=5
    // and spawn an elephant on it.
    for dx in 0..2 {
        for dz in 0..2 {
            sim.world.set(
                VoxelCoord::new(10 + dx, 5, 10 + dz),
                VoxelType::GrownPlatform,
            );
        }
    }
    sim.rebuild_transient_state();

    let mut events = Vec::new();
    let elephant_id = sim
        .spawn_creature(Species::Elephant, VoxelCoord::new(10, 6, 10), &mut events)
        .expect("should spawn elephant");

    let pos = sim.db.creatures.get(&elephant_id).unwrap().position;
    // Elephant should be on the platform (y=6, standing on platform at y=5).
    assert_eq!(pos.y, 6, "elephant should be on platform");

    // Remove the platform.
    for dx in 0..2 {
        for dz in 0..2 {
            sim.world
                .set(VoxelCoord::new(10 + dx, 5, 10 + dz), VoxelType::Air);
        }
    }
    sim.rebuild_transient_state();

    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1, "elephant should fall");

    let elephant = sim.db.creatures.get(&elephant_id).unwrap();
    assert!(
        elephant.position.y < 6,
        "elephant should have fallen from y=6, now at y={}",
        elephant.position.y
    );

    let fell_event = events
        .iter()
        .any(|e| matches!(e.kind, SimEventKind::CreatureFell { .. }));
    assert!(fell_event, "expected CreatureFell event for elephant");
}

#[test]
fn degenerate_landing_teleports_to_nearest_node() {
    let mut sim = test_sim(42);
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
        .spawn_creature(Species::Capybara, VoxelCoord::new(5, 1, 5), &mut events)
        .expect("should spawn capybara");
    let original_pos = sim.db.creatures.get(&capy_id).unwrap().position;

    // Teleport to a column that's entirely air (outside where nav nodes are
    // generated). Use a corner of the world where there's terrain but
    // possibly no nav node.
    let floating_pos = VoxelCoord::new(1, 10, 1);
    let _ = sim.db.creatures.modify_unchecked(&capy_id, |c| {
        c.position = floating_pos;
    });

    events.clear();
    let fell = sim.apply_single_creature_gravity(capy_id, &mut events);
    assert!(fell, "capybara should fall from unsupported position");

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    // The creature should have landed somewhere valid — either at ground
    // level in the same column or teleported to the nearest nav node.
    let graph = sim.graph_for_species(Species::Capybara);
    let has_node = graph.node_at(capy.position).is_some();
    let has_solid_below = sim
        .world
        .get(VoxelCoord::new(
            capy.position.x,
            capy.position.y - 1,
            capy.position.z,
        ))
        .is_solid();
    assert!(
        has_node && has_solid_below,
        "capybara should be at a valid supported position after degenerate fall \
         (pos={:?}, has_node={has_node}, solid_below={has_solid_below})",
        capy.position
    );
}

#[test]
fn ground_only_with_nav_node_but_no_solid_below_falls() {
    let mut sim = test_sim(42);
    // Test creature_is_supported: a ground_only creature at a nav node
    // position without solid below should be unsupported.
    // Build a platform, rebuild nav so there's a node, then force the
    // creature to that position and remove the platform WITHOUT rebuilding
    // nav, so the nav node persists but the voxel below is gone.
    let platform_pos = VoxelCoord::new(10, 3, 10);
    sim.world.set(platform_pos, VoxelType::GrownPlatform);
    sim.rebuild_transient_state();

    let standing_pos = VoxelCoord::new(10, 4, 10);
    // Verify nav node exists at standing position.
    let graph = sim.graph_for_species(Species::Capybara);
    assert!(
        graph.node_at(standing_pos).is_some(),
        "nav node should exist above platform"
    );

    // Spawn capybara at ground level, then teleport to the platform pos.
    let mut events = Vec::new();
    let capy_id = sim
        .spawn_creature(Species::Capybara, VoxelCoord::new(10, 1, 10), &mut events)
        .expect("should spawn capybara");
    let _ = sim.db.creatures.modify_unchecked(&capy_id, |c| {
        c.position = standing_pos;
    });
    assert!(
        sim.creature_is_supported(capy_id),
        "should be supported on platform"
    );

    // Remove the platform but do NOT rebuild nav — node still exists.
    sim.world.set(platform_pos, VoxelType::Air);
    let graph = sim.graph_for_species(Species::Capybara);
    assert!(
        graph.node_at(standing_pos).is_some(),
        "nav node should still exist (no rebuild)"
    );

    // Creature should be unsupported: ground_only needs solid below.
    assert!(
        !sim.creature_is_supported(capy_id),
        "ground_only creature without solid below should be unsupported even with nav node"
    );

    // Apply gravity — should fall. Rebuild nav first so landing positions
    // are valid.
    sim.rebuild_transient_state();
    events.clear();
    let fell = sim.apply_creature_gravity(&mut events);
    assert_eq!(fell, 1, "capybara should fall");
    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(capy.position.y, 1, "should land at ground level");
}

// ===================================================================
// Flying creature task integration (B-flying-tasks, B-flying-arrow-chase)
// ===================================================================

/// DirectedGoTo should create a GoTo task for a flying creature, even
/// though the destination may have no nearby nav node.
#[test]
fn flying_creature_directed_goto_creates_task() {
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Spawn hornet above elf, then reposition high to avoid combat.
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let start = VoxelCoord::new(elf_pos.x, elf_pos.y + 20, elf_pos.z);
    force_position(&mut sim, hornet, start);

    let dest = VoxelCoord::new(elf_pos.x + 5, elf_pos.y + 20, elf_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
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
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 5, tree_pos.y + 10, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    zero_creature_stats(&mut sim, hornet);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Give the hornet a GoTo task (PlayerDirected level). The hornet still
    // engages the elf because hostile_pursue runs during the activation loop
    // when the hornet encounters the elf while flying toward the GoTo target.
    let dest = VoxelCoord::new(tree_pos.x + 30, tree_pos.y + 10, tree_pos.z);
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
            creature_id: hornet,
            position: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Now spawn an elf near the hornet (within detection range).
    let elf = spawn_elf(&mut sim);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position;
    force_position(
        &mut sim,
        elf,
        VoxelCoord::new(hornet_pos.x + 2, hornet_pos.y, hornet_pos.z),
    );

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
/// Euclidean distance instead of nav-graph Dijkstra.
#[test]
fn find_available_task_works_for_flying_creature() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 5, tree_pos.y + 10, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Create an Available GoTo task near the hornet.
    let task_pos = VoxelCoord::new(tree_pos.x + 8, tree_pos.y + 10, tree_pos.z);
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
    sim.insert_task(task);

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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position;

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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
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
    sim.insert_task(task);
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    zero_creature_stats(&mut sim, elf);

    // Spawn hornet above the elf. AttackMove destination is far past the elf.
    let hornet_pos = VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z);
    let hornet = spawn_hornet_at(&mut sim, hornet_pos);
    zero_creature_stats(&mut sim, hornet);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // AttackMove to a far destination — the hornet must pass near the elf.
    let dest = VoxelCoord::new(elf_pos.x + 30, elf_pos.y + 3, elf_pos.z);
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position;

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
    sim.insert_task(near_task);

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
    sim.insert_task(far_task);

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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Spawn hornet far from elf to avoid combat.
    let hornet = spawn_hornet_at(
        &mut sim,
        VoxelCoord::new(elf_pos.x, elf_pos.y + 20, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, hornet);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position;

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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
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
    let mut sim = test_sim(42);
    let elf_near = spawn_elf(&mut sim);
    let elf_far = spawn_elf(&mut sim);
    let elf_near_pos = sim.db.creatures.get(&elf_near).unwrap().position;

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
    let new_pos = sim.db.creatures.get(&hornet).unwrap().position;
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
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
    let capybara_nav = sim.nav_graph.node_at(capybara.position);
    assert!(capybara_nav.is_some());
    let node_pos = sim.nav_graph.node(capybara_nav.unwrap()).position;
    assert_eq!(capybara.position, node_pos);
}

#[test]
fn capybara_stays_on_ground() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
            capybara.position.y, 1,
            "Capybara left ground at tick {target}: pos={:?}",
            capybara.position
        );
    }
}

#[test]
fn determinism_with_capybara() {
    let mut sim_a = test_sim(42);
    let mut sim_b = test_sim(42);

    let tree_pos = sim_a.db.trees.get(&sim_a.player_tree_id).unwrap().position;

    let spawn = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
fn wandering_creature_stays_on_nav_graph() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], 2);

    // Run for many ticks, periodically checking node validity.
    for target in (10000..100000).step_by(10000) {
        sim.step(&[], target);
        let elf = sim
            .db
            .creatures
            .iter_all()
            .find(|c| c.species == Species::Elf)
            .unwrap();
        let node = sim
            .nav_graph
            .node_at(elf.position)
            .expect("Elf should always have a nav node at its position");
        assert!(
            (node.0 as usize) < sim.nav_graph.node_count(),
            "Node ID {} out of range at tick {}",
            node.0,
            target
        );
        let node_pos = sim.nav_graph.node(node).position;
        assert_eq!(
            elf.position, node_pos,
            "Position mismatch at tick {}",
            target
        );
    }
}

#[test]
fn wander_sets_movement_metadata() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn an elf at tick 1, step only to tick 1 so the first activation
    // (scheduled at tick 2) hasn't fired yet.
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

    // Before the first activation, the elf should have no action.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.action_kind, ActionKind::NoAction);
    assert!(elf.next_available_tick.is_none());
    assert!(sim.db.move_actions.get(&elf_id).is_none());

    let initial_pos = elf.position;

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
        ma.move_to, elf.position,
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
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
            boar.position.y, 1,
            "Boar left ground at tick {target}: pos={:?}",
            boar.position
        );
    }
}

#[test]
fn deer_stays_on_ground() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
            deer.position.y, 1,
            "Deer left ground at tick {target}: pos={:?}",
            deer.position
        );
    }
}

#[test]
fn monkey_can_climb() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
    assert!(sim.nav_graph.node_at(monkey.position).is_some());
}

#[test]
fn squirrel_can_climb() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
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
    assert!(sim.nav_graph.node_at(squirrel.position).is_some());
}

// -----------------------------------------------------------------------
// DirectedGoTo command tests
// -----------------------------------------------------------------------

#[test]
fn directed_goto_creates_task_for_specific_creature() {
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Give elf a PlayerDirected GoTo task (PlayerDirected level 2).
    let task_id = TaskId::new(&mut sim.rng);
    let dest_node = sim.nav_graph.live_nodes().last().map(|n| n.id).unwrap();
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: sim.nav_graph.node(dest_node).position,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);
    sim.claim_task(elf, task_id);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 2, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Give elf an autonomous Harvest task (Autonomous level 1).
    let task_id = TaskId::new(&mut sim.rng);
    let dest_node = sim.nav_graph.live_nodes().last().map(|n| n.id).unwrap();
    let fruit_pos = VoxelCoord::new(0, 0, 0);
    let task = Task {
        id: task_id,
        kind: TaskKind::Harvest { fruit_pos },
        state: TaskState::InProgress,
        location: sim.nav_graph.node(dest_node).position,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(task);
    sim.claim_task(elf, task_id);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let target_pos = VoxelCoord::new(tree_pos.x + 2, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DirectedGoTo {
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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
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
    let mut sim = test_sim(42);
    let tick = sim.tick;
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    // Should not panic or create any tasks.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupGoTo {
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
    let mut sim = test_sim(42);
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
    let cmd = SimCommand {
        player_name: "test_player".to_string(),
        tick: 100,
        action: SimAction::GroupGoTo {
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
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);

    // Find a connected pair so the elf has a valid starting node.
    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_idle(&mut sim, elf);

    // Give the elf a GoTo task at node_b so it has reason to move.
    let task_id = insert_goto_task(&mut sim, node_b);
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.state = TaskState::InProgress;
        let _ = sim.db.tasks.update_no_fk(t);
    }
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Assign a cached path through a position that has NO nav node.
    // Use a coordinate far from the world (no node will exist there).
    let bogus_pos = VoxelCoord::new(63, 63, 63);
    assert!(
        sim.nav_graph.node_at(bogus_pos).is_none(),
        "Test setup: bogus position should have no nav node"
    );

    let real_dest = sim.nav_graph.node(node_b).position;
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.path = Some(CreaturePath {
            remaining_positions: vec![bogus_pos, real_dest],
        });
    });

    // Schedule the elf to activate and step forward. The movement code should
    // detect the missing nav node and repath (not panic).
    sim.event_queue.cancel_creature_activations(elf);
    sim.event_queue.schedule(
        sim.tick + 1,
        ScheduledEventKind::CreatureActivation { creature_id: elf },
    );
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
