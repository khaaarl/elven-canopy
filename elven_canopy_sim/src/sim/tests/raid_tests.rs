//! Tests for the raid system: attack-move commands (creation, walking,
//! engagement, resume-after-kill, nearest target, preemption, flee exemption),
//! and debug raid triggering (hostility, notifications, awareness).
//! Corresponds to `sim/raid.rs`.

use super::*;

// -----------------------------------------------------------------------
// F-attack-move: AttackMove task tests
// -----------------------------------------------------------------------

#[test]
fn test_attack_move_creates_task() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle(&mut sim, elf);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(creature.current_task.is_some(), "Elf should have a task");
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackMove);
    assert_eq!(task.state, TaskState::InProgress);
    assert_eq!(task.origin, TaskOrigin::PlayerDirected);
    assert!(task.target_creature.is_none());

    // Extension data should exist with the destination.
    let move_data = sim.task_attack_move_data(task.id).unwrap();
    assert_eq!(move_data.destination, dest);
}

#[test]
fn test_attack_move_walks_to_destination() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle(&mut sim, elf);

    // Pick a destination a few voxels away (no hostiles present).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let dest = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let task_id = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Run enough ticks for the elf to walk there.
    sim.step(&[], tick + 20_000);

    // Task should be complete.
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "AttackMove task should complete on arrival at destination"
    );
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Elf should be idle after arrival"
    );
}

#[test]
fn test_attack_move_engages_hostile() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Use connected nav nodes to ensure valid positions.
    let (node_a, node_b) = find_connected_pair(&sim);
    force_to_node(&mut sim, elf, node_a);
    force_to_node(&mut sim, goblin, node_b);
    force_idle(&mut sim, elf);
    force_guaranteed_hits(&mut sim, elf);
    suppress_activation(&mut sim, elf);
    suppress_activation(&mut sim, goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    // Use a far node on the nav graph as the destination.
    let dest_node = sim.nav_graph.live_nodes().last().map(|n| n.id).unwrap();
    let dest = sim.nav_graph.node(dest_node).position;

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    // Run long enough for detection and engagement.
    sim.step(&[cmd], tick + 10_000);

    // Goblin should have taken damage (elf detected and engaged).
    let goblin = sim.db.creatures.get(&goblin).unwrap();
    assert!(
        goblin.hp < goblin.hp_max || goblin.vital_status == VitalStatus::Dead,
        "Goblin should take damage from attack-move engagement: hp {}/{}",
        goblin.hp,
        goblin.hp_max
    );
}

#[test]
fn test_attack_move_resumes_after_kill() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place goblin near the elf.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

    let dest = VoxelCoord::new(elf_pos.x + 8, elf_pos.y, elf_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    let task_id = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Kill the goblin so the elf will disengage and resume walking.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 3,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    // Run long enough for elf to resume and reach destination.
    sim.step(&[kill_cmd], tick + 30_000);

    // Task should be complete (elf walked to destination after kill).
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "AttackMove task should complete after target dies and elf walks to destination"
    );
}

#[test]
fn test_attack_move_nearest_target() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin_near = spawn_species(&mut sim, Species::Goblin);
    let goblin_far = spawn_species(&mut sim, Species::Goblin);

    // Place near goblin closer, far goblin further.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let near_pos = VoxelCoord::new(elf_pos.x + 2, elf_pos.y, elf_pos.z);
    let far_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin_near, near_pos);
    force_position(&mut sim, goblin_far, far_pos);
    // Idle the elf and suppress it so it doesn't wander before the
    // command fires. The AttackMove command will override nat.
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

    let dest = VoxelCoord::new(elf_pos.x + 10, elf_pos.y, elf_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    // Run a few ticks for detection.
    sim.step(&[cmd], tick + 100);

    // Check that elf is targeting the near goblin.
    let creature = sim.db.creatures.get(&elf).unwrap();
    let task_id = creature
        .current_task
        .expect("Elf should still have an attack-move task");
    let task = sim.db.tasks.get(&task_id).unwrap();
    let target = task
        .target_creature
        .expect("Elf should have detected a hostile and set target_creature");
    assert_eq!(
        target, goblin_near,
        "Should target nearest hostile, not the far one"
    );
}

#[test]
fn test_attack_move_preempts_lower_priority() {
    let mut sim = test_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);

    // Give elf a GoTo task (PlayerDirected level 2).
    let far_node = sim.nav_graph.live_nodes().last().map(|n| n.id).unwrap();
    let goto_task_id = insert_goto_task(&mut sim, far_node);
    sim.claim_task(elf, goto_task_id);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            creature_id: elf,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Old task should be interrupted.
    let old_task = sim.db.tasks.get(&goto_task_id).unwrap();
    assert_eq!(
        old_task.state,
        TaskState::Complete,
        "GoTo task should be interrupted by AttackMove"
    );

    // Elf should have the attack-move task.
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(creature.current_task.is_some());
    let new_task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::AttackMove);
}

#[test]
fn test_attack_move_flee_exempt() {
    let mut sim = test_sim(legacy_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, &mut events)
        .expect("should spawn elf");

    // Elf is in Flee group (civilian) — should flee by default.
    assert!(sim.should_flee(elf_id, Species::Elf));

    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    // Issue a player-directed attack-move (creates a PlayerCombat-level task).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::AttackMove {
            creature_id: elf_id,
            destination: dest,
            queue: false,
        },
    };
    sim.step(&[cmd], 1);

    // PlayerCombat task overrides flee behavior.
    assert!(
        !sim.should_flee(elf_id, Species::Elf),
        "Elf with PlayerCombat attack-move task should not flee"
    );
}

#[test]
fn test_attack_move_serde_roundtrip() {
    let mut rng = GameRng::new(42);
    let task_id = TaskId::new(&mut rng);
    let location = VoxelCoord::new(5, 0, 0);

    let task = Task {
        id: task_id,
        kind: TaskKind::AttackMove,
        state: TaskState::InProgress,
        location,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    let json = serde_json::to_string(&task).unwrap();
    let restored: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, task_id);
    assert!(matches!(restored.kind, TaskKind::AttackMove));
    assert_eq!(restored.state, TaskState::InProgress);
    assert_eq!(restored.origin, TaskOrigin::PlayerDirected);
    assert!(restored.target_creature.is_none());
}

// -----------------------------------------------------------------------
// TriggerRaid command tests
// -----------------------------------------------------------------------

#[test]
fn trigger_raid_spawns_creatures() {
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);

    let hostile_species = sim
        .db
        .civilizations
        .get(&hostile_civ)
        .unwrap()
        .primary_species
        .to_species()
        .unwrap();
    let expected_count = sim.species_table[&hostile_species].raid_size;

    let pre_count = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_civ))
        .count();

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let post_count = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_civ))
        .count();

    let spawned = post_count - pre_count;
    assert!(spawned > 0, "trigger_raid should spawn at least one raider");
    assert_eq!(
        spawned, expected_count as usize,
        "should spawn raid_size creatures"
    );
}

#[test]
fn trigger_raid_assigns_attack_move() {
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    // All raiders from the hostile civ should have tasks.
    let raiders: Vec<_> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_civ) && c.vital_status == VitalStatus::Alive)
        .collect();

    assert!(!raiders.is_empty(), "should have spawned raiders");

    for raider in &raiders {
        assert!(
            raider.current_task.is_some(),
            "raider {} should have a task assigned",
            raider.id
        );
        let task = sim.db.tasks.get(&raider.current_task.unwrap()).unwrap();
        assert_eq!(
            task.kind_tag,
            TaskKindTag::AttackMove,
            "raider should have an AttackMove task"
        );
    }
}

#[test]
fn trigger_raid_spawns_at_perimeter() {
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);

    // Compute the actual terrain bounding box from ground nodes.
    let ground_nodes = sim.nav_graph.ground_node_ids();
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_z = i32::MAX;
    let mut max_z = i32::MIN;
    for &nid in &ground_nodes {
        let pos = sim.nav_graph.node(nid).position;
        min_x = min_x.min(pos.x);
        max_x = max_x.max(pos.x);
        min_z = min_z.min(pos.z);
        max_z = max_z.max(pos.z);
    }

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let raiders: Vec<_> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_civ) && c.vital_status == VitalStatus::Alive)
        .collect();

    // Allow a margin of 5 voxels from the terrain edge for nav snapping.
    let margin = 5;
    for raider in &raiders {
        let pos = raider.position.min;
        let near_north = pos.z <= min_z + margin;
        let near_south = pos.z >= max_z - margin;
        let near_east = pos.x >= max_x - margin;
        let near_west = pos.x <= min_x + margin;
        assert!(
            near_north || near_south || near_east || near_west,
            "raider at {:?} should be near terrain edge (x=[{}..{}], z=[{}..{}])",
            pos,
            min_x,
            max_x,
            min_z,
            max_z,
        );
    }
}

#[test]
fn trigger_raid_raiders_cluster_together() {
    // Raiders should spawn clustered near one point on the map edge,
    // not scattered across the entire perimeter.
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let raiders: Vec<_> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_civ) && c.vital_status == VitalStatus::Alive)
        .collect();

    if raiders.len() >= 2 {
        // All raiders should be within a reasonable distance of each other.
        // With the nearest-to-anchor selection, they should cluster tightly.
        let max_spread = 20; // generous upper bound for clustering
        for i in 0..raiders.len() {
            for j in (i + 1)..raiders.len() {
                let a = raiders[i].position.min;
                let b = raiders[j].position.min;
                let dx = (a.x - b.x).abs();
                let dz = (a.z - b.z).abs();
                assert!(
                    dx + dz <= max_spread,
                    "raiders at {:?} and {:?} are too far apart (Manhattan dist {})",
                    a,
                    b,
                    dx + dz,
                );
            }
        }
    }
}

#[test]
fn trigger_raid_no_hostile_civs() {
    let mut sim = test_sim(legacy_test_seed());
    remove_all_hostile_rels(&mut sim);

    let creature_count_before = sim.db.creatures.iter_all().count();

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let creature_count_after = sim.db.creatures.iter_all().count();
    assert_eq!(
        creature_count_before, creature_count_after,
        "no creatures should be spawned when no hostile civs exist"
    );
}

#[test]
fn trigger_raid_notification() {
    let mut sim = test_sim(legacy_test_seed());
    ensure_hostile_civ(&mut sim);

    let notif_count_before = sim.db.notifications.iter_all().count();

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let notif_count_after = sim.db.notifications.iter_all().count();
    assert!(
        notif_count_after > notif_count_before,
        "trigger_raid should create a notification"
    );
}

#[test]
fn trigger_raid_command_serde_roundtrip() {
    let cmd = SimCommand {
        player_name: "test".to_string(),
        tick: 1,
        action: SimAction::TriggerRaid,
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

#[test]
fn trigger_raid_single_activation_per_raider() {
    // Raid-spawned creatures should have exactly 1 pending activation
    // (next_available_tick is Some). Verifies that spawn + attack-move
    // doesn't leave the creature in an inconsistent activation state.
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let raiders: Vec<_> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_civ) && c.vital_status == VitalStatus::Alive)
        .collect();

    assert!(!raiders.is_empty(), "should have spawned raiders");

    for raider in &raiders {
        // In the poll-based model, a creature has exactly 1 pending
        // activation when next_available_tick is Some.
        let activation_count = sim.count_pending_activations_for(raider.id);
        assert_eq!(
            activation_count, 1,
            "raider {} should have exactly 1 pending activation, got {}",
            raider.id, activation_count
        );
    }
}

#[test]
fn trigger_raid_zero_raid_size_does_not_panic() {
    // If a species somehow has raid_size=0 and is selected for a raid,
    // the sim must not panic (division by zero in find_perimeter_positions).
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);

    // Set the raiding species' raid_size to 0.
    let hostile_species = sim
        .db
        .civilizations
        .get(&hostile_civ)
        .unwrap()
        .primary_species
        .to_species()
        .unwrap();
    sim.species_table
        .get_mut(&hostile_species)
        .unwrap()
        .raid_size = 0;

    let creature_count_before = sim.db.creatures.iter_all().count();
    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    // Should be a no-op — no creatures spawned, no panic.
    let creature_count_after = sim.db.creatures.iter_all().count();
    assert_eq!(creature_count_before, creature_count_after);
}

#[test]
fn trigger_raid_no_player_civ() {
    // If player_civ_id is None, trigger_raid should be a silent no-op.
    let mut sim = test_sim(legacy_test_seed());
    sim.player_civ_id = None;

    let creature_count_before = sim.db.creatures.iter_all().count();
    let notif_count_before = sim.db.notifications.iter_all().count();

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    assert_eq!(creature_count_before, sim.db.creatures.iter_all().count());
    assert_eq!(notif_count_before, sim.db.notifications.iter_all().count());
}

#[test]
fn trigger_raid_deterministic_same_seed() {
    // Two identically-seeded sims should produce identical raids.
    let seed = legacy_test_seed();
    let mut sim_a = test_sim(seed);
    let mut sim_b = test_sim(seed);

    let hostile_a = ensure_hostile_civ(&mut sim_a);
    let hostile_b = ensure_hostile_civ(&mut sim_b);
    assert_eq!(hostile_a, hostile_b);

    let mut events_a = Vec::new();
    let mut events_b = Vec::new();
    sim_a.trigger_raid(&mut events_a);
    sim_b.trigger_raid(&mut events_b);

    let raiders_a: Vec<_> = sim_a
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_a) && c.vital_status == VitalStatus::Alive)
        .map(|c| (c.species, c.position))
        .collect();
    let raiders_b: Vec<_> = sim_b
        .db
        .creatures
        .iter_all()
        .filter(|c| c.civ_id == Some(hostile_b) && c.vital_status == VitalStatus::Alive)
        .map(|c| (c.species, c.position))
        .collect();

    assert_eq!(raiders_a, raiders_b, "raids should be deterministic");
}

#[test]
fn trigger_raid_notification_includes_species_and_direction() {
    let mut sim = test_sim(legacy_test_seed());
    ensure_hostile_civ(&mut sim);

    let notif_count_before = sim.db.notifications.iter_all().count();
    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let new_notifs: Vec<_> = sim
        .db
        .notifications
        .iter_all()
        .skip(notif_count_before)
        .collect();

    assert!(!new_notifs.is_empty(), "should have a notification");
    let msg = &new_notifs.last().unwrap().message;

    // Must contain a direction.
    let has_direction = msg.contains("north")
        || msg.contains("south")
        || msg.contains("east")
        || msg.contains("west");
    assert!(
        has_direction,
        "notification should mention direction: {msg}"
    );

    // Must contain "raiding party" and a count.
    assert!(
        msg.contains("raiding party"),
        "notification should say 'raiding party': {msg}"
    );
    assert!(
        msg.contains("raiders"),
        "notification should mention raider count: {msg}"
    );
}

#[test]
fn species_config_backward_compat_raid_size() {
    // Old save files won't have raid_size. Verify it defaults to 1.
    let config = GameConfig::default();
    let goblin_data = &config.species[&Species::Goblin];
    let json = serde_json::to_string(goblin_data).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("raid_size");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: crate::species::SpeciesData = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.raid_size, 1, "raid_size should default to 1");
}

#[test]
fn trigger_raid_finds_hostile_via_reverse_relationship() {
    // If a civ considers the player hostile (them→player) but the player
    // doesn't know about them, the raid should still find them as a target.
    let mut sim = test_sim(legacy_test_seed());
    let player_civ = sim.player_civ_id.unwrap();

    // Remove ALL hostile relationships in both directions.
    remove_all_hostile_rels(&mut sim);

    // Find a goblin/orc/troll civ and create only a reverse hostile
    // relationship (they hate us, but we don't know them).
    let hostile_species_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| {
            c.id != player_civ
                && matches!(
                    c.primary_species,
                    CivSpecies::Goblin | CivSpecies::Orc | CivSpecies::Troll
                )
        })
        .map(|c| c.id)
        .expect("worldgen should produce at least one hostile-species civ");

    // Remove any pre-existing relationship from hostile_species_civ → player_civ
    // so that discover_civ doesn't no-op due to idempotency.
    let existing: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&hostile_species_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.to_civ == player_civ)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in existing {
        sim.db.remove_civ_relationship(&pk).unwrap();
    }

    sim.discover_civ(hostile_species_civ, player_civ, CivOpinion::Hostile);

    let creature_count_before = sim.db.creatures.iter_all().count();
    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let creature_count_after = sim.db.creatures.iter_all().count();
    assert!(
        creature_count_after > creature_count_before,
        "raid should spawn creatures when a civ considers the player hostile (reverse direction)"
    );
}

#[test]
fn trigger_raid_ignores_forward_only_hostility() {
    // If the player hates a civ but they don't hate us, no raid should come
    // from them — raids require them→player hostility.
    let mut sim = test_sim(legacy_test_seed());
    let player_civ = sim.player_civ_id.unwrap();

    remove_all_hostile_rels(&mut sim);

    let hostile_species_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| {
            c.id != player_civ
                && matches!(
                    c.primary_species,
                    CivSpecies::Goblin | CivSpecies::Orc | CivSpecies::Troll
                )
        })
        .map(|c| c.id)
        .expect("worldgen should produce at least one hostile-species civ");

    // Player hates them, but they don't hate us.
    sim.discover_civ(player_civ, hostile_species_civ, CivOpinion::Hostile);

    let creature_count_before = sim.db.creatures.iter_all().count();
    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let creature_count_after = sim.db.creatures.iter_all().count();
    assert_eq!(
        creature_count_before, creature_count_after,
        "forward-only hostility (we hate them) should not trigger a raid"
    );
}

#[test]
fn trigger_raid_human_civ_aborts_with_notification() {
    // If the only hostile civ has CivSpecies::Human (no sim creature type),
    // trigger_raid should abort with a "no creature type" notification.
    let mut sim = test_sim(legacy_test_seed());
    let player_civ = sim.player_civ_id.unwrap();
    remove_all_hostile_rels(&mut sim);

    // Find any non-player civ and make it a Human civ that hates us.
    let any_ai_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| c.id != player_civ)
        .map(|c| c.id)
        .unwrap();
    let mut civ = sim.db.civilizations.get(&any_ai_civ).unwrap();
    civ.primary_species = CivSpecies::Human;
    sim.db.update_civilization(civ).unwrap();
    // Remove any existing relationship from this civ toward the player
    // (worldgen may have created a Neutral one), so discover_civ can
    // insert a Hostile one.
    let _ = sim.db.remove_civ_relationship(&(any_ai_civ, player_civ));
    sim.discover_civ(any_ai_civ, player_civ, CivOpinion::Hostile);

    let creature_count_before = sim.db.creatures.iter_all().count();
    let notif_count_before = sim.db.notifications.iter_all().count();
    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    assert_eq!(
        creature_count_before,
        sim.db.creatures.iter_all().count(),
        "no creatures should spawn for a Human civ (no creature type)"
    );
    let new_notif = sim
        .db
        .notifications
        .iter_all()
        .skip(notif_count_before)
        .last()
        .expect("should have a notification");
    assert!(
        new_notif.message.contains("no creature type"),
        "notification should mention missing creature type: {}",
        new_notif.message
    );
}

#[test]
fn trigger_raid_creates_player_awareness_of_raiding_civ() {
    // When a raid happens, the player civ should discover and hate the raiding
    // civ. Without this, elves don't see raiders as hostile (yellow on minimap).
    let mut sim = test_sim(legacy_test_seed());
    let player_civ = sim.player_civ_id.unwrap();
    remove_all_hostile_rels(&mut sim);

    // Set up: a civ hates us, but we don't know about them.
    let hostile_species_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| {
            c.id != player_civ
                && matches!(
                    c.primary_species,
                    CivSpecies::Goblin | CivSpecies::Orc | CivSpecies::Troll
                )
        })
        .map(|c| c.id)
        .unwrap();

    // Remove any existing relationship from hostile→player so discover_civ
    // is not a no-op (worldgen may have already created a non-hostile one).
    let existing_rev: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&hostile_species_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.to_civ == player_civ)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in existing_rev {
        sim.db.remove_civ_relationship(&pk).unwrap();
    }
    // Also remove any existing forward relationship (player→hostile) so the
    // "before raid" assertion passes.
    let existing_fwd: Vec<_> = sim
        .db
        .civ_relationships
        .by_from_civ(&player_civ, tabulosity::QueryOpts::ASC)
        .into_iter()
        .filter(|r| r.to_civ == hostile_species_civ)
        .map(|r| (r.from_civ, r.to_civ))
        .collect();
    for pk in existing_fwd {
        sim.db.remove_civ_relationship(&pk).unwrap();
    }

    // They hate us (reverse only — we don't know about them).
    sim.discover_civ(hostile_species_civ, player_civ, CivOpinion::Hostile);

    // Verify we currently see them as Neutral (no forward relationship).
    assert_eq!(
        sim.diplomatic_relation(Some(player_civ), None, Some(hostile_species_civ), None),
        DiplomaticRelation::Neutral,
        "before raid: player should not yet know the raiding civ"
    );

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    // After the raid, the player should know about and hate the raiding civ.
    assert_eq!(
        sim.diplomatic_relation(Some(player_civ), None, Some(hostile_species_civ), None),
        DiplomaticRelation::Hostile,
        "after raid: player should consider the raiding civ hostile"
    );
}

#[test]
fn trigger_raid_elves_see_raiders_as_hostile() {
    // Raiders must be considered hostile by elves so combat triggers.
    let mut sim = test_sim(legacy_test_seed());
    let hostile_civ = ensure_hostile_civ(&mut sim);
    let player_civ = sim.player_civ_id.unwrap();

    let mut events = Vec::new();
    sim.trigger_raid(&mut events);

    let raider = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.civ_id == Some(hostile_civ) && c.vital_status == VitalStatus::Alive)
        .expect("should have spawned a raider");

    // From the player civ's perspective, the raider should be hostile.
    assert_eq!(
        sim.diplomatic_relation(Some(player_civ), None, Some(hostile_civ), None),
        DiplomaticRelation::Hostile,
        "player should see raiding civ as hostile"
    );

    // From the raider's perspective, the player should also be hostile.
    assert_eq!(
        sim.diplomatic_relation(Some(hostile_civ), None, Some(player_civ), None),
        DiplomaticRelation::Hostile,
        "raiding civ should see player as hostile"
    );
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn group_attack_move_spreads_creatures() {
    // GroupAttackMove should create AttackMove tasks at spread destinations.
    let mut sim = test_sim(legacy_test_seed());
    let elf_a = spawn_elf(&mut sim);
    let elf_b = spawn_elf(&mut sim);
    force_idle_and_cancel_activations(&mut sim, elf_a);
    force_idle_and_cancel_activations(&mut sim, elf_b);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let dest = VoxelCoord::new(tree_pos.x + 5, 1, tree_pos.z);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::GroupAttackMove {
                creature_ids: vec![elf_a, elf_b],
                destination: dest,
                queue: false,
            },
        }],
        tick + 2,
    );

    // Both should have AttackMove tasks.
    let task_a = sim.db.creatures.get(&elf_a).unwrap().current_task.unwrap();
    let task_b = sim.db.creatures.get(&elf_b).unwrap().current_task.unwrap();
    assert_eq!(
        sim.db.tasks.get(&task_a).unwrap().kind_tag,
        TaskKindTag::AttackMove
    );
    assert_eq!(
        sim.db.tasks.get(&task_b).unwrap().kind_tag,
        TaskKindTag::AttackMove
    );

    // Their task locations should differ (spread).
    let loc_a = sim.db.tasks.get(&task_a).unwrap().location;
    let loc_b = sim.db.tasks.get(&task_b).unwrap().location;
    assert_ne!(
        loc_a, loc_b,
        "GroupAttackMove should spread to different nav nodes"
    );
}

#[test]
fn group_attack_move_serialization_roundtrip() {
    let mut rng = crate::prng::GameRng::new(42);
    let cmd = SimCommand {
        player_name: "test_player".to_string(),
        tick: 100,
        action: SimAction::GroupAttackMove {
            creature_ids: vec![CreatureId::new(&mut rng), CreatureId::new(&mut rng)],
            destination: VoxelCoord::new(10, 1, 5),
            queue: false,
        },
    };

    let json = serde_json::to_string(&cmd).unwrap();
    let restored: SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}
