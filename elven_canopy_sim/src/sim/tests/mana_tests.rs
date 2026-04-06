//! Tests for the mana system: mana regeneration, overflow, build/craft
//! mana costs, wasted actions, mana requirements for tasks, and grow
//! recipe mana integration.
//! Corresponds to `sim/mana.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Mana system — Phase A: data model
// ---------------------------------------------------------------------------

#[test]
fn elf_spawns_with_mana() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    // mp_max is WIL-scaled from species base, so read from the creature.
    assert!(elf.mp_max > 0, "elves should be magical");
    assert_eq!(elf.mp, elf.mp_max, "elf should spawn at full mana");
    assert_eq!(elf.wasted_action_count, 0);
}

#[test]
fn capybara_spawns_without_mana() {
    let mut sim = test_sim(legacy_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);
    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(capy.mp, 0);
    assert_eq!(capy.mp_max, 0);
}

#[test]
fn goblin_spawns_without_mana() {
    let mut sim = test_sim(legacy_test_seed());
    let gob_id = spawn_creature(&mut sim, Species::Goblin);
    let gob = sim.db.creatures.get(&gob_id).unwrap();
    assert_eq!(gob.mp, 0);
    assert_eq!(gob.mp_max, 0);
}

#[test]
fn mana_fields_serde_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Drain some mana so mp != mp_max.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = c.mp_max / 2;
    c.wasted_action_count = 2;
    sim.db.update_creature(c).unwrap();

    let elf_mp_max = sim.db.creatures.get(&elf_id).unwrap().mp_max;

    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: crate::db::SimDb = serde_json::from_str(&json).unwrap();
    let elf = restored.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.mp, elf_mp_max / 2);
    assert_eq!(elf.mp_max, elf_mp_max);
    assert_eq!(elf.wasted_action_count, 2);
}

#[test]
fn mana_fields_default_on_old_save() {
    // Simulate deserializing an old save that lacks mp/mp_max/wasted_action_count
    // by serializing a creature, stripping the mana fields, and deserializing.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let json = serde_json::to_string(&elf).unwrap();

    // Strip mana fields to simulate an old save format.
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("mp");
    obj.remove("mp_max");
    obj.remove("wasted_action_count");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: crate::db::Creature = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.mp, 0);
    assert_eq!(restored.mp_max, 0);
    assert_eq!(restored.wasted_action_count, 0);
}

#[test]
fn game_config_has_mana_cost_defaults() {
    let config = GameConfig::default();
    assert!(config.default_mana_cost > 0);
    assert!(config.mana_abandon_threshold > 0);
    assert!(config.platform_mana_cost > 0);
    assert!(config.grow_mana_cost > 0);
}

#[test]
fn species_mana_config_only_elves_magical() {
    let config = GameConfig::default();
    for (species, data) in &config.species {
        if *species == Species::Elf {
            assert!(data.mp_max > 0, "elves should have mana");
            assert!(data.ticks_per_mp_regen > 0, "elves should regen mana");
        } else {
            assert_eq!(data.mp_max, 0, "{species:?} should be nonmagical");
            assert_eq!(
                data.ticks_per_mp_regen, 0,
                "{species:?} should not regen mana"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Mana system — Remainder accumulation
// ---------------------------------------------------------------------------

#[test]
fn mana_regen_remainder_accumulates_across_heartbeats() {
    // When heartbeat_interval / effective_tpr has a remainder, the leftover
    // ticks must accumulate across heartbeats to avoid losing fractional MP.
    let mut config = test_config();
    // Set ticks_per_mp_regen high so each heartbeat produces < 1 MP base,
    // forcing remainder accumulation to matter.
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 5000;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let elf_id = spawn_elf(&mut sim);

    // Zero stats to remove scaling (divisor = identity at stat 0).
    zero_creature_stats(&mut sim, elf_id);

    // Drain mana to 0.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    c.mp_regen_remainder = 0;
    sim.db.update_creature(c).unwrap();

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    // ticks_per_mp_regen=5000, heartbeat=3000, stat=0 (identity divisor).
    // Each heartbeat: total_ticks = remainder + 3000.
    // HB1: 0+3000 / 5000 = 0 MP, remainder 3000
    // HB2: 3000+3000 / 5000 = 1 MP, remainder 1000
    // HB3: 1000+3000 / 5000 = 0 MP, remainder 4000
    // HB4: 4000+3000 / 5000 = 1 MP, remainder 2000
    // After 4 heartbeats: 2 MP total.

    // Advance 4 heartbeats.
    sim.step(&[], sim.tick + heartbeat * 4 + 1);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.mp, 2,
        "should gain 2 MP over 4 heartbeats via remainder accumulation, got {}",
        elf.mp,
    );
    assert_eq!(
        elf.mp_regen_remainder, 2000,
        "remainder should be 2000, got {}",
        elf.mp_regen_remainder,
    );
}

#[test]
fn mp_regen_remainder_serde_roundtrip() {
    let mut sim = test_sim(fresh_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp_regen_remainder = 1234;
    sim.db.update_creature(c).unwrap();

    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: crate::db::SimDb = serde_json::from_str(&json).unwrap();
    let elf = restored.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.mp_regen_remainder, 1234);
}

#[test]
fn mp_regen_remainder_defaults_on_old_save() {
    // Old saves without mp_regen_remainder should default to 0.
    let mut sim = test_sim(fresh_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let json = serde_json::to_string(&elf).unwrap();

    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().remove("mp_regen_remainder");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: crate::db::Creature = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.mp_regen_remainder, 0);
}

// ---------------------------------------------------------------------------
// Mana system — Phase B: generation and overflow
// ---------------------------------------------------------------------------

#[test]
fn elf_mana_regenerates_on_heartbeat() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf_mp_max = sim.species_table[&Species::Elf].mp_max;

    // Drain the elf's mana to half.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = elf_mp_max / 2;
    sim.db.update_creature(c).unwrap();
    let mp_before = sim.db.creatures.get(&elf_id).unwrap().mp;

    // Advance past one heartbeat (elf heartbeat = 3000 ticks).
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let mp_after = sim.db.creatures.get(&elf_id).unwrap().mp;
    assert!(
        mp_after > mp_before,
        "mana should increase: {mp_before} -> {mp_after}"
    );
    // Should not exceed max.
    assert!(mp_after <= elf_mp_max);
}

#[test]
fn elf_mana_does_not_exceed_max() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf_mp_max = sim.db.creatures.get(&elf_id).unwrap().mp_max;

    // Start at max — generation should not push above max.
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().mp, elf_mp_max);

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let mp_after = sim.db.creatures.get(&elf_id).unwrap().mp;
    assert_eq!(mp_after, elf_mp_max);
}

#[test]
fn elf_mana_overflow_goes_to_tree() {
    // Overflow is 1:1: any regen MP beyond mp_max goes directly to the tree.
    // Use zero stats so effective_tpr = base ticks_per_mp_regen (no scaling).
    let mut sim = test_sim(fresh_test_seed());
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let tree_id = sim.player_tree_id;
    let elf_mp_max = sim.db.creatures.get(&elf_id).unwrap().mp_max;

    // Elf starts at full mana → all generation overflows to tree.
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().mp, elf_mp_max);

    // Zero remainder and tree mana for a clean measurement.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp_regen_remainder = 0;
    sim.db.update_creature(c).unwrap();
    let mut info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    info.mana_stored = 0;
    sim.db.update_great_tree_info(info).unwrap();

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let tpr = sim.species_table[&Species::Elf].ticks_per_mp_regen;
    sim.step(&[], sim.tick + heartbeat + 1);

    let info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    // With zero stats, effective_tpr = tpr (identity divisor).
    // Regen = heartbeat / tpr. All of it overflows since elf is at max.
    let expected_overflow = heartbeat as i64 / tpr as i64;
    assert_eq!(
        info.mana_stored, expected_overflow,
        "tree should gain exactly {expected_overflow} MP (1:1 overflow), got {}",
        info.mana_stored
    );
}

#[test]
fn tree_mana_capped_at_capacity() {
    let mut sim = test_sim(legacy_test_seed());
    let _elf_id = spawn_creature(&mut sim, Species::Elf);
    let tree_id = sim.player_tree_id;

    // Fill tree to capacity.
    let cap = sim.db.great_tree_infos.get(&tree_id).unwrap().mana_capacity;
    let mut info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    info.mana_stored = cap;
    sim.db.update_great_tree_info(info).unwrap();

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    assert_eq!(
        info.mana_stored, cap,
        "tree mana should not exceed capacity"
    );
}

#[test]
fn wild_creature_mana_overflow_is_lost() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;

    // Spawn an elf, then remove its civ to make it "wild."
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.civ_id = None;
    sim.db.update_creature(elf).unwrap();

    // Set tree mana to 0.
    let mut info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    info.mana_stored = 0;
    sim.db.update_great_tree_info(info).unwrap();

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    assert_eq!(
        info.mana_stored, 0,
        "wild elf overflow should not reach the tree"
    );
}

#[test]
fn elf_mana_regen_scaled_by_wil_int_average() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let tpr = sim.species_table[&Species::Elf].ticks_per_mp_regen;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let wil = sim.trait_int(elf_id, TraitKind::Willpower, 0);
    let int = sim.trait_int(elf_id, TraitKind::Intelligence, 0);
    let avg_stat = (wil + int) / 2;

    // Drain the elf's mana to 0 and zero remainder for clean measurement.
    let mp_max = sim.db.creatures.get(&elf_id).unwrap().mp_max;
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    c.mp_regen_remainder = 0;
    sim.db.update_creature(c).unwrap();

    sim.step(&[], sim.tick + heartbeat + 1);

    let mp_after = sim.db.creatures.get(&elf_id).unwrap().mp;
    let effective_tpr = crate::stats::apply_stat_divisor(tpr as i64, avg_stat).max(1);
    let expected_regen = heartbeat as i64 / effective_tpr;
    // Regen should not exceed mp_max.
    let expected = expected_regen.min(mp_max);
    assert_eq!(
        mp_after, expected,
        "mana regen should use ticks_per_mp_regen ({tpr}) scaled by avg(WIL={wil}, INT={int})={avg_stat}: \
         expected {expected}, got {mp_after}"
    );
}

// ---------------------------------------------------------------------------
// Mana system — Phase C: construction mana drain
// ---------------------------------------------------------------------------

#[test]
fn mana_cost_per_action_uses_build_type() {
    let sim = test_sim(legacy_test_seed());
    let platform_cost = sim.mana_cost_per_action(Some(BuildType::Platform));
    let default_cost = sim.mana_cost_per_action(None);

    assert!(platform_cost > 0, "platform cost should be positive");
    assert!(default_cost > 0, "default cost should be positive");
}

#[test]
fn build_action_drains_elf_mana() {
    // Disable mana regen so we can observe net drain clearly.
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 1;
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    let air_coord = find_air_adjacent_to_trunk(&sim);
    let elf_id = spawn_elf(&mut sim);
    let mp_max = sim.db.creatures.get(&elf_id).unwrap().mp_max;

    // Designate a 1-voxel platform.
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

    // Run until build completes.
    sim.step(&[], sim.tick + 400_000);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        elf.mp < mp_max,
        "mana should have decreased from {mp_max}: got {}",
        elf.mp
    );
    // Voxel should be placed (build succeeded).
    assert_eq!(
        sim.voxel_zone(sim.home_zone_id()).unwrap().get(air_coord),
        VoxelType::GrownPlatform
    );
}

#[test]
fn build_wasted_action_no_progress() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 1;
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let air_coord = find_air_adjacent_to_trunk(&sim);
    let elf_id = spawn_elf(&mut sim);

    // Drain all elf mana.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    sim.db.update_creature(c).unwrap();

    // Designate and let the elf attempt to build.
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

    let project_id = *sim.db.blueprints.iter_keys().next().unwrap();

    // Step enough for elf to arrive and attempt work actions.
    sim.step(&[], sim.tick + 200_000);

    // Voxel should NOT have been placed — elf has no mana.
    assert_eq!(
        sim.voxel_zone(sim.home_zone_id()).unwrap().get(air_coord),
        VoxelType::Air,
        "no voxel should be placed when elf has no mana"
    );

    // Blueprint should still be Designated (not Complete).
    let bp = sim.db.blueprints.get(&project_id).unwrap();
    assert_eq!(bp.state, BlueprintState::Designated);
}

#[test]
fn build_abandon_after_wasted_actions() {
    // Test the try_drain_mana + abandon mechanism directly by manually
    // setting up task state and calling try_drain_mana.
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 1;
    config.mana_abandon_threshold = 2;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    let air_coord = find_air_adjacent_to_trunk(&sim);
    let elf_id = spawn_elf(&mut sim);

    // Designate build.
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

    let task_id = sim
        .db
        .blueprints
        .iter_all()
        .next()
        .unwrap()
        .task_id
        .unwrap();

    // Force: assign elf to the task, set task to InProgress, drain mana to 0.
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.current_task = Some(task_id);
    elf.mp = 0;
    sim.db.update_creature(elf).unwrap();
    let mut task = sim.db.tasks.get(&task_id).unwrap();
    task.state = TaskState::InProgress;
    sim.db.update_task(task).unwrap();

    let cost = sim.mana_cost_per_action(Some(BuildType::Platform));

    // First wasted action — count becomes 1, threshold is 2: no abandon.
    let result1 = sim.try_drain_mana(elf_id, cost);
    assert!(!result1, "should fail: no mana");
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.wasted_action_count, 1);
    assert!(elf.current_task.is_some(), "should still have task");

    // Second wasted action — count reaches 2: trigger abandon.
    let result2 = sim.try_drain_mana(elf_id, cost);
    assert!(!result2, "should fail: no mana");

    // Task should be Available (abandoned).
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Available,
        "task should revert to Available after mana abandon"
    );

    // Elf should be unassigned with counter reset.
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.current_task.is_none(), "elf should be unassigned");
    assert_eq!(elf.wasted_action_count, 0, "counter should reset");
}

// ---------------------------------------------------------------------------
// Mana system — Phase D: task claiming eligibility
// ---------------------------------------------------------------------------

#[test]
fn nonmagical_creature_cannot_claim_build_task() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 1;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    // Spawn a goblin (nonmagical but can climb — unlike capybara which is
    // ground-only and would be filtered by pathfinding, not the mana check).
    let gob_id = spawn_creature(&mut sim, Species::Goblin);
    assert_eq!(
        sim.db.creatures.get(&gob_id).unwrap().mp_max,
        0,
        "goblin should be nonmagical"
    );

    // Insert a Build task with no species restriction at the goblin's location.
    // We need a valid ProjectId — create a fake one.
    let project_id = ProjectId::new(&mut sim.rng);
    let task_id = TaskId::new(&mut sim.rng);
    let gob_node = creature_pos(&sim, gob_id);
    let task = Task {
        id: task_id,
        kind: TaskKind::Build { project_id },
        state: TaskState::Available,
        location: gob_node,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);

    // Goblin should NOT find this task (nonmagical, can't do Build).
    let found = sim.find_available_task(gob_id);
    assert!(
        found.is_none(),
        "nonmagical goblin should not find Build tasks"
    );
}

#[test]
fn elf_with_no_mana_skips_build_task() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 1;
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

    let elf_id = spawn_elf(&mut sim);

    // Drain all mana.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    sim.db.update_creature(c).unwrap();

    // Insert a Build task at the elf's location.
    let project_id = ProjectId::new(&mut sim.rng);
    let task_id = TaskId::new(&mut sim.rng);
    let elf_node = creature_pos(&sim, elf_id);
    let task = Task {
        id: task_id,
        kind: TaskKind::Build { project_id },
        state: TaskState::Available,
        location: elf_node,
        progress: 0,
        total_cost: 1,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);

    // Elf with 0 mana should NOT find the Build task.
    let found = sim.find_available_task(elf_id);
    assert!(
        found.is_none(),
        "elf with 0 mana should not find Build tasks"
    );
}

#[test]
fn elf_with_mana_claims_build_task() {
    let mut sim = build_test_sim();
    let elf_id = spawn_elf(&mut sim);

    // Elf starts at full mana — should be able to find Build tasks.
    let air_coord = find_air_adjacent_to_trunk(&sim);
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

    let task_id = sim
        .db
        .blueprints
        .iter_all()
        .next()
        .unwrap()
        .task_id
        .unwrap();

    // Elf should find the task.
    let found = sim.find_available_task(elf_id);
    assert_eq!(found, Some(task_id), "elf with mana should find Build task");
}

#[test]
fn successful_build_resets_wasted_counter() {
    let mut sim = build_test_sim();
    let air_coord = find_air_adjacent_to_trunk(&sim);
    let elf_id = spawn_elf(&mut sim);

    // Set a non-zero wasted count, but leave enough mana to build.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.wasted_action_count = 2;
    sim.db.update_creature(c).unwrap();

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

    // Run until build completes.
    sim.step(&[], sim.tick + 400_000);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.wasted_action_count, 0,
        "wasted counter should reset after successful build"
    );
}

// ---------------------------------------------------------------------------
// Mana system — Once-over: additional coverage
// ---------------------------------------------------------------------------

#[test]
fn mana_cost_per_action_carve_uses_default() {
    let sim = test_sim(legacy_test_seed());
    let carve_cost = sim.mana_cost_per_action(Some(BuildType::Carve));
    let default_cost = sim.mana_cost_per_action(None);
    assert_eq!(carve_cost, default_cost, "carve should use default cost");
}

#[test]
fn mana_cost_per_action_zero_cost_returns_zero() {
    let mut config = test_config();
    config.platform_mana_cost = 0;
    let sim = SimState::with_config(legacy_test_seed(), config);
    let cost = sim.mana_cost_per_action(Some(BuildType::Platform));
    assert_eq!(cost, 0, "zero cost config should yield zero cost");
}

#[test]
fn try_drain_mana_exact_boundary() {
    // When mp == cost exactly, drain should succeed (>=, not >).
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let cost = sim.mana_cost_per_action(Some(BuildType::Platform));
    assert!(cost > 0);

    // Set mp to exactly the cost.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = cost;
    sim.db.update_creature(c).unwrap();

    // Give the elf a task so abandon logic has something to work with.
    let task_id = TaskId::new(&mut sim.rng);
    let elf_node = creature_pos(&sim, elf_id);
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: elf_node,
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
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.current_task = Some(task_id);
    sim.db.update_creature(elf).unwrap();

    let result = sim.try_drain_mana(elf_id, cost);
    assert!(result, "should succeed when mp == cost exactly");
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.mp, 0, "mp should be drained to 0");
    assert_eq!(elf.wasted_action_count, 0, "counter should be reset");
}

#[test]
fn nonmagical_creature_cannot_claim_furnish_task() {
    let mut sim = test_sim(legacy_test_seed());
    let gob_id = spawn_creature(&mut sim, Species::Goblin);
    assert_eq!(sim.db.creatures.get(&gob_id).unwrap().mp_max, 0);

    let task_id = TaskId::new(&mut sim.rng);
    let gob_node = creature_pos(&sim, gob_id);
    let task = Task {
        id: task_id,
        kind: TaskKind::Furnish {
            structure_id: StructureId(999),
        },
        state: TaskState::Available,
        location: gob_node,
        progress: 0,
        total_cost: 1,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), task);

    let found = sim.find_available_task(gob_id);
    assert!(
        found.is_none(),
        "nonmagical goblin should not find Furnish tasks"
    );
}

#[test]
fn multiple_elves_overflow_to_same_tree() {
    let mut sim = test_sim(fresh_test_seed());
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf1);
    zero_creature_stats(&mut sim, elf2);
    let tree_id = sim.player_tree_id;

    // Zero remainder and tree mana for clean measurement.
    for elf_id in [elf1, elf2] {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.mp_regen_remainder = 0;
        sim.db.update_creature(c).unwrap();
    }
    let mut info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    info.mana_stored = 0;
    sim.db.update_great_tree_info(info).unwrap();

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let tpr = sim.species_table[&Species::Elf].ticks_per_mp_regen;
    let single_elf_overflow = heartbeat as i64 / tpr as i64;
    sim.step(&[], sim.tick + heartbeat + 1);

    let info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    // Two elves at full mp (zero stats), each overflow = heartbeat / tpr.
    // Total tree gain = 2 × single_elf_overflow.
    assert_eq!(
        info.mana_stored,
        2 * single_elf_overflow,
        "tree should receive overflow from both elves: expected {}, got {}",
        2 * single_elf_overflow,
        info.mana_stored
    );
}

#[test]
fn config_backward_compat_mana_fields_from_old_json() {
    // Serialize a GameConfig, strip the mana cost fields, deserialize,
    // and verify serde defaults are correct.
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("platform_mana_cost");
    obj.remove("default_mana_cost");
    obj.remove("grow_mana_cost");
    obj.remove("mana_abandon_threshold");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: GameConfig = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.platform_mana_cost, 2);
    assert_eq!(restored.default_mana_cost, 2);
    assert_eq!(restored.grow_mana_cost, 2);
    assert_eq!(restored.mana_abandon_threshold, 3);
}

#[test]
fn species_config_backward_compat_mana_fields() {
    // Serialize elf SpeciesData, strip mp_max and ticks_per_mp_regen, verify defaults.
    let config = GameConfig::default();
    let elf_data = &config.species[&Species::Elf];
    let json = serde_json::to_string(elf_data).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("mp_max");
    obj.remove("ticks_per_mp_regen");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: crate::species::SpeciesData = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.mp_max, 0);
    assert_eq!(restored.ticks_per_mp_regen, 0);
}

#[test]
fn config_ignores_removed_mana_base_generation_rate_mm() {
    // Old configs with the removed field should still deserialize cleanly.
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Add the old field that was removed in F-mana-scale.
    value
        .as_object_mut()
        .unwrap()
        .insert("mana_base_generation_rate_mm".to_string(), 1000.into());
    let modified = serde_json::to_string(&value).unwrap();

    let restored: GameConfig = serde_json::from_str(&modified).unwrap();
    // Should parse fine — unknown fields are silently ignored.
    assert!(restored.platform_mana_cost > 0);
}

#[test]
fn zero_regen_elf_does_not_overflow_to_tree() {
    // An elf with ticks_per_mp_regen=0 should generate no mana and send
    // nothing to the tree, even if at full MP.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let _elf_id = spawn_elf(&mut sim);
    let tree_id = sim.player_tree_id;

    let mut info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    info.mana_stored = 0;
    sim.db.update_great_tree_info(info).unwrap();

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let info = sim.db.great_tree_infos.get(&tree_id).unwrap();
    assert_eq!(
        info.mana_stored, 0,
        "zero-regen elf should not overflow to tree"
    );
}

#[test]
fn grow_cost_uses_grow_config_not_default() {
    let mut config = test_config();
    config.grow_mana_cost = 5;
    config.default_mana_cost = 2;
    let sim = SimState::with_config(fresh_test_seed(), config);
    assert_eq!(sim.mana_cost_for_grow_action(), 5);
    assert_eq!(sim.mana_cost_per_action(None), 2);
}

#[test]
fn starting_mana_less_than_capacity() {
    let config = GameConfig::default();
    assert!(
        config.starting_mana < config.starting_mana_capacity,
        "starting_mana ({}) should be less than starting_mana_capacity ({})",
        config.starting_mana,
        config.starting_mana_capacity
    );
}

#[test]
fn cleanup_early_exit_resets_wasted_action_count() {
    // When cleanup_and_unassign_task takes the early-exit path (task already
    // deleted), wasted_action_count must still be reset.
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);

    // Create a task, assign elf, set nonzero wasted count.
    let task_id = TaskId::new(&mut sim.rng);
    let elf_node = creature_pos(&sim, elf_id);
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: elf_node,
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
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.current_task = Some(task_id);
    elf.wasted_action_count = 2;
    sim.db.update_creature(elf).unwrap();

    // Delete the task to trigger the early-exit path.
    // Must clear current_task FK reference on the elf before removing the task.
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.current_task = None;
    sim.db.update_creature(elf).unwrap();
    sim.db.remove_task(&task_id).unwrap();

    // Now cleanup — should take the early-exit (task gone) and reset counter.
    sim.cleanup_and_unassign_task(elf_id, task_id);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.current_task.is_none());
    assert_eq!(
        elf.wasted_action_count, 0,
        "early-exit cleanup must reset wasted_action_count"
    );
}

#[test]
fn requires_mana_correct_for_all_task_kinds() {
    // Build and Furnish require mana; all others do not.
    assert!(TaskKindTag::Build.requires_mana());
    assert!(TaskKindTag::Furnish.requires_mana());
    assert!(!TaskKindTag::GoTo.requires_mana());
    assert!(!TaskKindTag::EatBread.requires_mana());
    assert!(!TaskKindTag::EatFruit.requires_mana());
    assert!(!TaskKindTag::Sleep.requires_mana());
    assert!(!TaskKindTag::Haul.requires_mana());
    assert!(!TaskKindTag::Craft.requires_mana());
    assert!(!TaskKindTag::Harvest.requires_mana());
    assert!(!TaskKindTag::Mope.requires_mana());
    assert!(!TaskKindTag::AcquireItem.requires_mana());
    assert!(!TaskKindTag::AcquireMilitaryEquipment.requires_mana());
    assert!(!TaskKindTag::AttackMove.requires_mana());
    assert!(!TaskKindTag::AttackTarget.requires_mana());
}

#[test]
fn drained_elf_can_still_claim_non_mana_tasks() {
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);
    let elf_id = spawn_elf(&mut sim);

    // Drain all mana.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    sim.db.update_creature(c).unwrap();

    // Insert a GoTo task (non-mana) at the elf's location.
    let elf_node = creature_pos(&sim, elf_id);
    let task_id = insert_goto_task(&mut sim, elf_node);

    // Elf with 0 mana should still find the GoTo task.
    let found = sim.find_available_task(elf_id);
    assert_eq!(
        found,
        Some(task_id),
        "drained elf should still find non-mana tasks"
    );
}

// ---------------------------------------------------------------------------
// Mana system — F-mana-depleted-vfx: wasted position buffer
// ---------------------------------------------------------------------------

#[test]
fn mana_wasted_position_recorded_on_failed_drain() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;

    // Give the elf a task so try_drain_mana has something to work with.
    let task_id = TaskId::new(&mut sim.rng);
    let elf_node = creature_pos(&sim, elf_id);
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: elf_node,
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
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.current_task = Some(task_id);
    elf.mp = 0;
    sim.db.update_creature(elf).unwrap();

    assert!(
        sim.voxel_zone(sim.home_zone_id())
            .unwrap()
            .mana_wasted_positions
            .is_empty()
    );

    let cost = sim.mana_cost_per_action(Some(BuildType::Platform));
    sim.try_drain_mana(elf_id, cost);

    assert_eq!(
        sim.voxel_zone(sim.home_zone_id())
            .unwrap()
            .mana_wasted_positions
            .len(),
        1
    );
    assert_eq!(
        sim.voxel_zone(sim.home_zone_id())
            .unwrap()
            .mana_wasted_positions[0],
        elf_pos
    );
}

#[test]
fn mana_wasted_positions_cleared_each_step() {
    let mut sim = test_sim(legacy_test_seed());
    // Manually push a position to simulate a previous step.
    sim.voxel_zone_mut(sim.home_zone_id())
        .unwrap()
        .mana_wasted_positions
        .push(VoxelCoord::new(0, 0, 0));
    assert_eq!(
        sim.voxel_zone(sim.home_zone_id())
            .unwrap()
            .mana_wasted_positions
            .len(),
        1
    );

    // step() should clear the buffer.
    sim.step(&[], sim.tick + 1);
    assert!(
        sim.voxel_zone(sim.home_zone_id())
            .unwrap()
            .mana_wasted_positions
            .is_empty(),
        "buffer should be cleared at start of step()"
    );
}

#[test]
fn successful_drain_does_not_record_position() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Give the elf a task.
    let task_id = TaskId::new(&mut sim.rng);
    let elf_node = creature_pos(&sim, elf_id);
    let task = Task {
        id: task_id,
        kind: TaskKind::GoTo,
        state: TaskState::InProgress,
        location: elf_node,
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
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.current_task = Some(task_id);
    sim.db.update_creature(elf).unwrap();

    let cost = sim.mana_cost_per_action(Some(BuildType::Platform));
    let result = sim.try_drain_mana(elf_id, cost);
    assert!(result, "drain should succeed — elf has full mana");
    assert!(
        sim.voxel_zone(sim.home_zone_id())
            .unwrap()
            .mana_wasted_positions
            .is_empty(),
        "no position recorded on success"
    );
}

// ---------------------------------------------------------------------------
// Grow-verb mana drain (F-mana-grow-recipes)
// ---------------------------------------------------------------------------

#[test]
fn grow_mana_cost_config_defaults() {
    let config = GameConfig::default();
    assert!(
        config.grow_mana_cost > 0,
        "grow mana cost should have a positive default"
    );
    assert!(
        config.grow_recipes.grow_work_ticks_per_action > 0,
        "grow work ticks per action should have a positive default"
    );
}

#[test]
fn mana_cost_for_grow_action_positive() {
    let sim = test_sim(legacy_test_seed());
    let cost = sim.mana_cost_for_grow_action();
    assert!(cost > 0, "grow action mana cost should be positive: {cost}");
}
#[test]
fn grow_craft_task_has_action_count_total_cost() {
    // Grow recipes should have total_cost = ceil(work_ticks / per_action)
    // instead of total_cost = work_ticks.
    let mut sim = test_sim(legacy_test_seed());
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
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

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
    sim.spawn_creature(Species::Elf, anchor, sim.home_zone_id(), &mut events);
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
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    config.mana_abandon_threshold = 2;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

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
    sim.spawn_creature(Species::Elf, anchor, sim.home_zone_id(), &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Drain all elf mana.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    sim.db.update_creature(c).unwrap();

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
    let mut sim = test_sim(legacy_test_seed());
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
    sim.spawn_creature(Species::Capybara, anchor, sim.home_zone_id(), &mut events);
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
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

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
    sim.spawn_creature(Species::Elf, anchor, sim.home_zone_id(), &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Drain all mana — non-Grow craft should still succeed.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    sim.db.update_creature(c).unwrap();

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
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

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
    sim.spawn_creature(Species::Elf, anchor, sim.home_zone_id(), &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Drain all mana.
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 0;
    sim.db.update_creature(c).unwrap();

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
    obj.remove("grow_mana_cost");
    if let Some(grow_obj) = obj.get_mut("grow_recipes").and_then(|v| v.as_object_mut()) {
        grow_obj.remove("grow_work_ticks_per_action");
    }
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: GameConfig = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.grow_mana_cost, 2);
    assert_eq!(restored.grow_recipes.grow_work_ticks_per_action, 1000);
}

#[test]
fn drained_elf_cannot_claim_grow_craft_task() {
    // An elf with mp > 0 but below the grow cost should not claim Grow tasks.
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .ticks_per_mp_regen = 0;
    let mut sim = SimState::with_config(legacy_test_seed(), config);

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
    sim.spawn_creature(Species::Elf, anchor, sim.home_zone_id(), &mut events);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Set mp to 1 — below the grow cost (which is mp_max / 1000 * 20).
    let mut c = sim.db.creatures.get(&elf_id).unwrap();
    c.mp = 1;
    sim.db.update_creature(c).unwrap();

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
    let mut sim = SimState::with_config(legacy_test_seed(), config);

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

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn capybara_generates_no_mana() {
    let mut sim = test_sim(legacy_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    let heartbeat = sim.species_table[&Species::Capybara].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat + 1);

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    assert_eq!(capy.mp, 0);
    assert_eq!(capy.mp_max, 0);
}

// ---------------------------------------------------------------------------
// Mana system — Phase B2: stat-scaled mana pool and regeneration
// ---------------------------------------------------------------------------

#[test]
fn elf_mp_max_scaled_by_willpower() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let species_mp_max = sim.species_table[&Species::Elf].mp_max;
    let wil = sim.trait_int(elf_id, TraitKind::Willpower, 0);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let expected = crate::stats::apply_stat_multiplier(species_mp_max, wil).max(1);
    assert_eq!(
        elf.mp_max, expected,
        "mp_max should be species base ({species_mp_max}) scaled by WIL ({wil}): expected {expected}, got {}",
        elf.mp_max
    );
    // Creature should spawn at full (stat-scaled) mana.
    assert_eq!(elf.mp, elf.mp_max);
}

#[test]
fn elf_mp_max_unaffected_when_wil_is_zero() {
    let mut sim = test_sim(legacy_test_seed());
    // Override elf WIL distribution to mean=0, stdev=0.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .mean = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .stdev = 0;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let species_mp_max = sim.species_table[&Species::Elf].mp_max;

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    // WIL=0 means 1× multiplier, so mp_max == species base.
    assert_eq!(elf.mp_max, species_mp_max);
}

#[test]
fn elf_mp_max_reduced_by_negative_willpower() {
    let mut sim = test_sim(legacy_test_seed());
    // Force negative WIL: mean=-50, stdev=0.
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .mean = -50;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .stat_distributions
        .get_mut(&TraitKind::Willpower)
        .unwrap()
        .stdev = 0;

    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let species_mp_max = sim.species_table[&Species::Elf].mp_max;
    let elf = sim.db.creatures.get(&elf_id).unwrap();

    // WIL=-50 → 2^(-0.5) ≈ 0.707× multiplier, so mp_max < species base.
    assert!(
        elf.mp_max < species_mp_max,
        "negative WIL should reduce mp_max: got {} (species base {})",
        elf.mp_max,
        species_mp_max
    );
    assert!(elf.mp_max >= 1, "mp_max floor is 1");
    assert_eq!(elf.mp, elf.mp_max, "should spawn at full mana");
}

#[test]
fn nonmagical_creature_mp_max_unaffected_by_stats() {
    let mut sim = test_sim(legacy_test_seed());
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    let capy = sim.db.creatures.get(&capy_id).unwrap();
    // mp_max=0 species should stay 0 regardless of any stats.
    assert_eq!(capy.mp_max, 0);
    assert_eq!(capy.mp, 0);
}
