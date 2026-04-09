//! Tests for the mood and thought system: thought dedup, expiry, cap,
//! mood scoring, mood tiers, mood config, moping behavior, and
//! integration tests for thought generation from actions.
//! Corresponds to `sim/mood.rs`.

use super::*;

/// Mope test helper: create a sim with mope configuration and optional thoughts.
fn mope_test_setup(
    mope_config: crate::config::MoodConsequencesConfig,
    thoughts: &[ThoughtKind],
) -> (SimState, CreatureId) {
    let mut config = test_config();
    config.mood_consequences = mope_config;
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
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
    sim.step(&[cmd], 1);

    let elf_id = *sim
        .db
        .creatures
        .iter_keys()
        .find(|id| sim.db.creatures.get(id).unwrap().species == Species::Elf)
        .expect("elf should exist");

    for thought in thoughts {
        sim.add_creature_thought(elf_id, thought.clone());
    }

    (sim, elf_id)
}

/// Helper: create a sim_with_elf for thought tests.
fn sim_with_elf_for_thoughts() -> (SimState, CreatureId) {
    let mut sim = test_sim(fresh_test_seed());
    let elf_id = spawn_elf(&mut sim);
    sim.tick = 1000;
    (sim, elf_id)
}

#[test]
fn thought_insert_after_roundtrip_continues_seq() {
    let mut sim = test_sim(fresh_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    sim.tick = 1000;
    sim.add_creature_thought(elf_id, ThoughtKind::AteDining);
    sim.tick = 2000;
    sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);

    // Roundtrip via serde.
    let json = serde_json::to_string(&sim).unwrap();
    let mut restored: SimState = serde_json::from_str(&json).unwrap();

    // Insert a new thought after roundtrip — seq counter must continue.
    // Use a large tick gap to exceed the 150_000-tick dedup cooldown.
    restored.tick = 200_000;
    restored.add_creature_thought(elf_id, ThoughtKind::AteDining);

    let thoughts = restored
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC);
    assert_eq!(
        thoughts.len(),
        3,
        "Expected 3 thoughts, got {}: {:?}",
        thoughts.len(),
        thoughts
            .iter()
            .map(|t| (&t.kind, t.seq, t.tick))
            .collect::<Vec<_>>()
    );
    // All seq values must be unique (no collision).
    let mut seqs: Vec<u64> = thoughts.iter().map(|t| t.seq).collect();
    seqs.sort();
    seqs.dedup();
    assert_eq!(seqs.len(), 3, "seq values must be unique after roundtrip");
}

/// Verify that thoughts loaded from old-format saves (with "next_id" instead
/// of "next_seq") get a correctly recomputed seq counter, so new inserts after
/// load don't collide with existing PKs.
#[test]
fn thought_seq_counter_survives_old_format_roundtrip() {
    let mut sim = test_sim(fresh_test_seed());
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    sim.tick = 1000;
    sim.add_creature_thought(elf_id, ThoughtKind::AteDining);

    let json = serde_json::to_string(&sim).unwrap();
    let mut parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Simulate old format: rename "next_seq" to "next_id" in thoughts table.
    let db = parsed.get_mut("db").unwrap().as_object_mut().unwrap();
    if let Some(val) = db.get("thoughts") {
        if let Some(obj) = val.as_object() {
            let mut new_obj = obj.clone();
            if let Some(counter) = new_obj.remove("next_seq") {
                new_obj.insert("next_id".to_string(), counter);
            }
            db.insert("thoughts".to_string(), serde_json::Value::Object(new_obj));
        }
    }

    let old_json = serde_json::to_string(&parsed).unwrap();
    let mut restored: SimState = serde_json::from_str(&old_json).unwrap();

    // The thought should survive.
    let thoughts = restored
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 1);

    // Insert a new thought — seq counter must have been recomputed from rows.
    restored.tick = 200_000;
    restored.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);

    let thoughts = restored
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 2);
    // Seq values must differ (no collision from counter reset).
    assert_ne!(thoughts[0].seq, thoughts[1].seq);
}

#[test]
fn eat_bread_generates_thought() {
    let mut sim = test_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;

    // Spawn an elf.
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
    let elf_node = creature_pos(&sim, elf_id);

    // Give bread and set food low.
    sim.inv_add_simple_item(
        sim.creature_inv(elf_id),
        inventory::ItemKind::Bread,
        1,
        Some(elf_id),
        None,
    );
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max / 10;
        sim.db.update_creature(c).unwrap();
    }

    // Create EatBread task at current node.
    let task_id = TaskId::new(&mut sim.rng);
    let eat_task = Task {
        id: task_id,
        kind: TaskKind::EatBread,
        state: TaskState::InProgress,
        location: elf_node,
        progress: 0,
        total_cost: 0,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), eat_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Advance enough to start and complete the Eat action.
    sim.step(&[], sim.tick + sim.config.eat_action_ticks + 10);

    let _elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == ThoughtKind::AteAlone),
        "Eating bread should generate AteAlone thought"
    );
    // Piggyback: mood should reflect the AteAlone thought (small penalty).
    let (score, _tier) = sim.mood_for_creature(elf_id);
    assert!(
        score < 0,
        "AteAlone should produce negative mood score, got {score}"
    );
}

// -----------------------------------------------------------------------
// Thought system tests
// -----------------------------------------------------------------------

#[test]
fn thought_dedup_within_cooldown() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    sim.tick = 1001;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 1, "Dedup should prevent second add");
}

#[test]
fn thought_dedup_allows_after_cooldown() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    let cooldown = sim.config.thoughts.dedup_ate_dining_ticks;
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    sim.tick = 1000 + cooldown;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 2, "Should allow add after cooldown expires");
}

#[test]
fn thought_dedup_distinguishes_structure_ids() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::SleptInOwnHome(StructureId(1)));
    sim.tick = 1001;
    sim.add_creature_thought(cid, ThoughtKind::SleptInOwnHome(StructureId(2)));
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
    assert_eq!(
        thoughts.len(),
        2,
        "Different structure IDs are distinct thoughts"
    );
}

#[test]
fn thought_cap_enforced() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.config.thoughts.cap = 5;
    sim.config.thoughts.dedup_ate_dining_ticks = 0; // Disable dedup.
    for i in 0..7 {
        sim.tick = i * 1000;
        sim.add_creature_thought(cid, ThoughtKind::AteDining);
    }
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 5, "Should not exceed cap");
    // Oldest should have been dropped — first remaining is tick 2000.
    assert_eq!(thoughts[0].tick, 2000);
}

#[test]
fn thought_expiry() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    let expiry = sim.config.thoughts.expiry_ate_dining_ticks;
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    // Before expiry: should remain.
    sim.tick = 1000 + expiry - 1;
    sim.expire_creature_thoughts(cid);
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 1, "Should not expire yet");
    // At expiry: should be removed.
    sim.tick = 1000 + expiry;
    sim.expire_creature_thoughts(cid);
    let thoughts = sim
        .db
        .thoughts
        .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 0, "Should expire at expiry tick");
}

#[test]
fn thought_serde_roundtrip_via_simstate() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.tick = 5000;
    sim.add_creature_thought(cid, ThoughtKind::SleptOnGround);
    sim.tick = 6000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();
    let thoughts = restored
        .db
        .thoughts
        .by_creature_id(&cid, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 2);
    assert_eq!(thoughts[0].kind, ThoughtKind::SleptOnGround);
    assert_eq!(thoughts[1].kind, ThoughtKind::AteDining);
}

// -----------------------------------------------------------------------
// Mood tests
// -----------------------------------------------------------------------

#[test]
fn mood_empty_thoughts_is_zero() {
    let (sim, cid) = sim_with_elf_for_thoughts();
    let (score, tier) = sim.mood_for_creature(cid);
    assert_eq!(score, 0);
    assert_eq!(tier, MoodTier::Neutral);
}

#[test]
fn mood_single_positive_thought() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    let (score, tier) = sim.mood_for_creature(cid);
    assert_eq!(score, 60);
    assert_eq!(tier, MoodTier::Content);
}

#[test]
fn mood_single_negative_thought() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::SleptOnGround);
    let (score, tier) = sim.mood_for_creature(cid);
    assert_eq!(score, -100);
    assert_eq!(tier, MoodTier::Unhappy);
}

#[test]
fn mood_mixed_thoughts() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::SleptInOwnHome(StructureId(1)));
    sim.tick = 2000;
    sim.add_creature_thought(cid, ThoughtKind::LowCeiling(StructureId(2)));
    let (score, tier) = sim.mood_for_creature(cid);
    // +80 + (-50) = +30
    assert_eq!(score, 30);
    assert_eq!(tier, MoodTier::Content);
}

#[test]
fn mood_stacking_same_kind() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.config.thoughts.dedup_ate_dining_ticks = 0; // Disable dedup.
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    sim.tick = 2000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    sim.tick = 3000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    let (score, tier) = sim.mood_for_creature(cid);
    // 3 * 60 = 180
    assert_eq!(score, 180);
    assert_eq!(tier, MoodTier::Happy);
}

#[test]
fn mood_tier_boundaries() {
    let cfg = crate::config::MoodConfig::default();
    // Exact boundary values.
    assert_eq!(cfg.tier(-300), MoodTier::Devastated);
    assert_eq!(cfg.tier(-301), MoodTier::Devastated);
    assert_eq!(cfg.tier(-299), MoodTier::Miserable);
    assert_eq!(cfg.tier(-150), MoodTier::Miserable);
    assert_eq!(cfg.tier(-149), MoodTier::Unhappy);
    assert_eq!(cfg.tier(-30), MoodTier::Unhappy);
    assert_eq!(cfg.tier(-29), MoodTier::Neutral);
    assert_eq!(cfg.tier(0), MoodTier::Neutral);
    assert_eq!(cfg.tier(29), MoodTier::Neutral);
    assert_eq!(cfg.tier(30), MoodTier::Content);
    assert_eq!(cfg.tier(149), MoodTier::Content);
    assert_eq!(cfg.tier(150), MoodTier::Happy);
    assert_eq!(cfg.tier(299), MoodTier::Happy);
    assert_eq!(cfg.tier(300), MoodTier::Elated);
    assert_eq!(cfg.tier(301), MoodTier::Elated);
}

#[test]
fn mood_custom_config_weights() {
    let (mut sim, cid) = sim_with_elf_for_thoughts();
    sim.tick = 1000;
    sim.add_creature_thought(cid, ThoughtKind::AteDining);
    sim.config.mood.weight_ate_dining = 200;
    let (score, _) = sim.mood_for_creature(cid);
    assert_eq!(score, 200);
}

#[test]
fn mood_config_serde_roundtrip() {
    let cfg = crate::config::MoodConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let restored: crate::config::MoodConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.weight_ate_dining, cfg.weight_ate_dining);
    assert_eq!(restored.tier_elated_above, cfg.tier_elated_above);
}

#[test]
fn mood_config_backward_compat() {
    // A GameConfig JSON without a "mood" key should deserialize with defaults.
    let sim = test_sim(fresh_test_seed());
    let json = serde_json::to_string(&sim).unwrap();
    // Strip the "mood" key from the JSON to simulate an old save.
    let mut val: serde_json::Value = serde_json::from_str(&json).unwrap();
    val.get_mut("config")
        .and_then(|c| c.as_object_mut())
        .unwrap()
        .remove("mood");
    let stripped = serde_json::to_string(&val).unwrap();
    let restored: SimState = serde_json::from_str(&stripped).unwrap();
    assert_eq!(
        restored.config.mood.weight_ate_dining,
        crate::config::MoodConfig::default().weight_ate_dining
    );
}

#[test]
fn ground_sleep_generates_thought() {
    // Integration test: elf sleeps on ground → has SleptOnGround thought.
    let mut config = test_config();
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0; // No hunger interference.
    elf_species.rest_decay_per_tick = 0; // Manual control of rest.
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Spawn an elf.
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

    // Set rest very low to trigger sleep.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 10 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance past heartbeat to trigger sleep + enough ticks for it to complete.
    let target_tick = 1 + heartbeat_interval + sim.config.sleep_ticks_ground + 1000;
    sim.step(&[], target_tick);

    // Elf should have a SleptOnGround thought.
    let _elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == ThoughtKind::SleptOnGround),
        "Elf should have SleptOnGround thought after ground sleep. thoughts={:?}",
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
    );
    // Piggyback: mood should reflect the negative SleptOnGround thought.
    let (score, _tier) = sim.mood_for_creature(elf_id);
    let expected: i32 = sim
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|t| sim.config.mood.mood_weight(&t.kind))
        .sum();
    assert_eq!(
        score, expected,
        "Mood score should match sum of thought weights"
    );
}

#[test]
fn eating_generates_thought() {
    // Integration test: elf eats fruit → has AteAlone thought.
    let mut sim = test_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let food_max = sim.species_table[&Species::Elf].food_max;
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Ensure the tree has fruit — some seeds produce trees without any.
    ensure_tree_has_fruit(&mut sim);

    // Spawn an elf.
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

    // Make the elf hungry.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.food = food_max * 10 / 100;
        sim.db.update_creature(c).unwrap();
    }

    // Advance enough ticks for the elf to find fruit, walk to it, and eat it.
    // Walk could be up to ~50 voxels at 500 tpv = 25000 ticks.
    let target_tick = 1 + heartbeat_interval + 50_000;
    sim.step(&[], target_tick);

    // Elf should have an AteAlone thought (no dining hall, so emergency eating).
    let _elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == ThoughtKind::AteAlone),
        "Elf should have AteAlone thought after eating. thoughts={:?}",
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
    );
}

#[test]
fn dormitory_sleep_generates_thought() {
    // Integration test: elf sleeps in dormitory → has SleptInDormitory thought.
    let mut config = test_config();
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;

    // Add a dormitory with beds near the tree.
    let bed_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let structure_id = StructureId(999);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            zone_id: sim.home_zone_id(),
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
    let hz = sim.home_zone_id();
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            zone_id: hz,
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

    // Set rest very low to trigger sleep.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 10 / 100;
        sim.db.update_creature(c).unwrap();
    }
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Advance enough ticks for sleep to trigger, walk to bed, and complete.
    // Walk time + sleep time + buffer.
    let target_tick = 1 + heartbeat_interval + 50_000 + sim.config.sleep_ticks_bed;
    sim.step(&[], target_tick);

    let _elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == ThoughtKind::SleptInDormitory(structure_id)),
        "Elf should have SleptInDormitory thought. thoughts={:?}",
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
    );
    // Piggyback: mood should reflect dormitory sleep thought.
    let (score, _tier) = sim.mood_for_creature(elf_id);
    let expected: i32 = sim
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|t| sim.config.mood.mood_weight(&t.kind))
        .sum();
    assert_eq!(
        score, expected,
        "Mood score should match sum of thought weights"
    );
}

#[test]
fn home_sleep_generates_thought() {
    // Integration test: elf sleeps in assigned home → has SleptInOwnHome thought.
    let mut config = test_config();
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;

    // Add a home with a bed near the tree.
    let bed_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let structure_id = StructureId(888);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            zone_id: sim.home_zone_id(),
            project_id,
            build_type: BuildType::Building,
            anchor: bed_pos,
            width: 3,
            depth: 3,
            height: 3,
            completed_tick: 0,
            name: None,
            furnishing: Some(FurnishingType::Home),
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
    let hz = sim.home_zone_id();
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            zone_id: hz,
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

    // Assign the home to the elf.
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

    // Set rest very low to trigger sleep.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 10 / 100;
        sim.db.update_creature(c).unwrap();
    }
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Advance enough ticks for sleep to trigger, walk to bed, and complete.
    let target_tick = 2 + heartbeat_interval + 50_000 + sim.config.sleep_ticks_bed;
    sim.step(&[], target_tick);

    let _elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == ThoughtKind::SleptInOwnHome(structure_id)),
        "Elf should have SleptInOwnHome thought. thoughts={:?}",
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
    );
    // Piggyback: mood should reflect home sleep thought.
    let (score, _tier) = sim.mood_for_creature(elf_id);
    let expected: i32 = sim
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|t| sim.config.mood.mood_weight(&t.kind))
        .sum();
    assert_eq!(
        score, expected,
        "Mood score should match sum of thought weights"
    );
}

#[test]
fn low_ceiling_generates_thought() {
    // Integration test: elf sleeps in height-1 building → LowCeiling thought.
    let mut config = test_config();
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let rest_max = sim.species_table[&Species::Elf].rest_max;

    // Add a dormitory with height=1 (low ceiling).
    let bed_pos = find_walkable(&sim, tree_pos, 10).unwrap();

    let structure_id = StructureId(888);
    let project_id = ProjectId::new(&mut sim.rng);
    let inv_id = sim.create_inventory(crate::db::InventoryOwnerKind::Structure);
    insert_stub_blueprint(&mut sim, project_id);
    sim.db
        .insert_structure(CompletedStructure {
            id: structure_id,
            zone_id: sim.home_zone_id(),
            project_id,
            build_type: BuildType::Building,
            anchor: bed_pos,
            width: 3,
            depth: 3,
            height: 1, // Low ceiling!
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
    let hz = sim.home_zone_id();
    sim.db
        .insert_furniture_auto(|id| crate::db::Furniture {
            id,
            zone_id: hz,
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

    // Set rest very low to trigger sleep.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = rest_max * 10 / 100;
        sim.db.update_creature(c).unwrap();
    }
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Advance enough ticks for sleep to trigger, walk to bed, and complete.
    let target_tick = 1 + heartbeat_interval + 50_000 + sim.config.sleep_ticks_bed;
    sim.step(&[], target_tick);

    let _elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == ThoughtKind::LowCeiling(structure_id)),
        "Elf should have LowCeiling thought from height-1 building. thoughts={:?}",
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
    );
    // Should also have the dormitory sleep thought.
    assert!(
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .iter()
            .any(|t| t.kind == ThoughtKind::SleptInDormitory(structure_id)),
        "Elf should also have SleptInDormitory thought. thoughts={:?}",
        sim.db
            .thoughts
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
    );
    // Piggyback: mood should reflect both dormitory sleep and low ceiling.
    let (score, _tier) = sim.mood_for_creature(elf_id);
    let expected: i32 = sim
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|t| sim.config.mood.mood_weight(&t.kind))
        .sum();
    assert_eq!(
        score, expected,
        "Mood score should match sum of thought weights"
    );
}

// -----------------------------------------------------------------------
// Mood consequences: moping tests
// -----------------------------------------------------------------------

#[test]
fn mope_probability_zero_mean_never_fires() {
    // When mean = 0, mope_mean_ticks returns 0, check_mope should never trigger.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 0,
        mope_mean_ticks_miserable: 0,
        mope_mean_ticks_devastated: 0,
        ..Default::default()
    };
    assert_eq!(cfg.mope_mean_ticks(MoodTier::Unhappy), 0);
    assert_eq!(cfg.mope_mean_ticks(MoodTier::Miserable), 0);
    assert_eq!(cfg.mope_mean_ticks(MoodTier::Devastated), 0);

    // Run many heartbeats with zero-mean config + unhappy elf.
    let (mut sim, _elf_id) = mope_test_setup(
        cfg,
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );

    // Advance many heartbeat cycles.
    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 100);

    // No Mope task should exist.
    let has_mope = sim
        .db
        .tasks
        .iter_all()
        .any(|t| t.kind_tag == TaskKindTag::Mope);
    assert!(!has_mope, "Zero mean should never produce a Mope task");
}

#[test]
fn mope_probability_nonzero_fires_proportionally() {
    // With a very small mean (= heartbeat interval), mope fires ~100% per heartbeat.
    let interval = 3000_u64; // Default elf heartbeat.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: interval, // P ≈ 1.0 per heartbeat
        mope_duration_ticks: 1,            // Short mope so it completes quickly.
        ..Default::default()
    };
    let (mut sim, _elf_id) = mope_test_setup(
        cfg,
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );

    // Run several heartbeats.
    sim.step(&[], sim.tick + interval * 10);

    // With P ≈ 1.0 and 10 heartbeats, at least one Mope should fire.
    let mope_count = sim
        .db
        .tasks
        .iter_all()
        .filter(|t| t.kind_tag == TaskKindTag::Mope)
        .count();
    assert!(
        mope_count >= 1,
        "With mean=elapsed, at least one Mope should fire in 10 heartbeats, got {mope_count}"
    );
}

#[test]
fn mope_config_serde_roundtrip() {
    let cfg = crate::config::MoodConsequencesConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let restored: crate::config::MoodConsequencesConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(
        restored.mope_mean_ticks_unhappy,
        cfg.mope_mean_ticks_unhappy
    );
    assert_eq!(restored.mope_duration_ticks, cfg.mope_duration_ticks);
    assert_eq!(
        restored.mope_can_interrupt_task,
        cfg.mope_can_interrupt_task
    );
}

#[test]
fn mope_config_backward_compat() {
    // A GameConfig JSON without "mood_consequences" key → defaults.
    let sim = test_sim(fresh_test_seed());
    let json = serde_json::to_string(&sim).unwrap();
    let mut val: serde_json::Value = serde_json::from_str(&json).unwrap();
    val.get_mut("config")
        .and_then(|c| c.as_object_mut())
        .unwrap()
        .remove("mood_consequences");
    let stripped = serde_json::to_string(&val).unwrap();
    let restored: SimState = serde_json::from_str(&stripped).unwrap();
    assert_eq!(
        restored.config.mood_consequences.mope_mean_ticks_unhappy,
        crate::config::MoodConsequencesConfig::default().mope_mean_ticks_unhappy
    );
}

#[test]
fn mope_task_completes_and_elf_resumes() {
    // Short mope duration; elf should be idle afterward.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000, // P ≈ 1.0
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000,
        mope_duration_ticks: 10, // Very short mope.
        ..Default::default()
    };
    let (mut sim, _elf_id) = mope_test_setup(
        cfg,
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    // Advance enough for mope to trigger and complete.
    sim.step(&[], sim.tick + interval * 5);

    // At least one completed Mope should exist (state == Complete).
    let completed_mope = sim
        .db
        .tasks
        .iter_all()
        .any(|t| t.kind_tag == TaskKindTag::Mope && t.state == TaskState::Complete);
    assert!(
        completed_mope,
        "Mope task should complete after mope_duration_ticks"
    );
}

#[test]
fn mope_does_not_interrupt_autonomous_sleep() {
    // A Devastated elf that is sleeping should NOT have sleep interrupted by mope.
    // We use a very long sleep and drain rest to 0 so the sleep won't complete
    // during the test window, proving mope didn't interrupt it.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000,
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000, // P ≈ 1.0 per heartbeat
        mope_can_interrupt_task: true,
        mope_duration_ticks: 100,
    };
    let mut config = test_config();
    config.mood_consequences = cfg;
    config.sleep_ticks_ground = 1_000_000; // Very long sleep.
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    elf_species.rest_per_sleep_tick = 1; // Tiny restore so rest_full won't trigger.
    let mut sim = SimState::with_config(fresh_test_seed(), config);
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
    sim.step(&[cmd], 1);

    let elf_id = *sim
        .db
        .creatures
        .iter_keys()
        .find(|id| sim.db.creatures.get(id).unwrap().species == Species::Elf)
        .unwrap();

    // Drain rest to 0 so the elf won't complete sleep via rest_full.
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.rest = 0;
        sim.db.update_creature(c).unwrap();
    }

    // Inject Devastated-level thoughts.
    for _ in 0..4 {
        sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
    }

    // Manually assign a Sleep task to the elf.
    let elf_node = creature_pos(&sim, elf_id);
    let sleep_task_id = TaskId::new(&mut sim.rng);
    let sleep_task = Task {
        id: sleep_task_id,
        kind: TaskKind::Sleep {
            bed_pos: None,
            location: crate::task::SleepLocation::Ground,
        },
        state: TaskState::InProgress,
        location: elf_node,
        progress: 0,
        total_cost: 1000000,
        required_species: None,
        origin: TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), sleep_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(sleep_task_id);
        sim.db.update_creature(c).unwrap();
    }

    // Run several heartbeats — mope rate is P≈1.0 but should not interrupt sleep.
    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 10);

    // The elf should still be sleeping — same task, never interrupted.
    let current_task = sim.db.creatures.get(&elf_id).and_then(|c| c.current_task);
    assert_eq!(
        current_task,
        Some(sleep_task_id),
        "Mope should not interrupt autonomous Sleep task"
    );
}

#[test]
fn mope_does_not_interrupt_existing_mope() {
    // A moping elf should not have its mope interrupted by another mope.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000,
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000, // P ≈ 1.0 per heartbeat
        mope_can_interrupt_task: true,
        mope_duration_ticks: 100_000, // Long mope — won't complete during test.
    };
    let (mut sim, elf_id) = mope_test_setup(
        cfg,
        &[
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
        ],
    );

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    // Run enough heartbeats to trigger the first mope.
    sim.step(&[], sim.tick + interval * 5);

    // Elf should have a mope task.
    let mope_task_id = sim
        .db
        .creatures
        .get(&elf_id)
        .and_then(|c| c.current_task)
        .filter(|tid| {
            sim.db
                .tasks
                .get(tid)
                .is_some_and(|t| t.kind_tag == TaskKindTag::Mope)
        });
    assert!(mope_task_id.is_some(), "Elf should be moping");
    let first_mope_id = mope_task_id.unwrap();

    // Run more heartbeats — mope rate is P≈1.0 but should not replace existing mope.
    sim.step(&[], sim.tick + interval * 10);

    let current_task = sim.db.creatures.get(&elf_id).and_then(|c| c.current_task);
    assert_eq!(
        current_task,
        Some(first_mope_id),
        "Moping elf should keep the same Mope task, not get a replacement"
    );
}

#[test]
fn mope_always_preempts_player_directed_build() {
    // With the preemption system, Mood(4) always preempts PlayerDirected(2)
    // regardless of the mope_can_interrupt_task config field (which is
    // now superseded). Verify that even with mope_can_interrupt_task=false,
    // a Devastated elf's Build task is still interrupted.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000,
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000, // P ≈ 1.0
        mope_can_interrupt_task: false,   // Superseded — should have no effect.
        mope_duration_ticks: 100,
    };
    let (mut sim, elf_id) = mope_test_setup(
        cfg,
        &[
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
        ],
    );

    // Assign a long-running Build task at the elf's current node.
    let elf_node = creature_pos(&sim, elf_id);
    let task_id = TaskId::new(&mut sim.rng);
    let project_id = crate::types::ProjectId::new(&mut sim.rng);
    let build_task = Task {
        id: task_id,
        kind: TaskKind::Build { project_id },
        state: TaskState::InProgress,
        location: elf_node,
        progress: 0,
        total_cost: 1000000,
        required_species: None,
        origin: TaskOrigin::PlayerDirected,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(sim.home_zone_id(), build_task);
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 10);

    // The elf should have been interrupted — its current task should differ
    // from the original Build task (either moping or idle after mope).
    let current_task = sim.db.creatures.get(&elf_id).and_then(|c| c.current_task);
    assert_ne!(
        current_task,
        Some(task_id),
        "Mope (Mood) should always preempt Build (PlayerDirected), \
             regardless of deprecated mope_can_interrupt_task config"
    );
}

#[test]
fn mope_task_location_is_home_when_assigned() {
    // An elf with an assigned home should mope at the home's nav node.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000,
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000, // P ≈ 1.0
        mope_duration_ticks: 50_000,      // Long enough to observe.
        ..Default::default()
    };
    let mut config = test_config();
    config.mood_consequences = cfg;
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Create a home.
    let anchor = VoxelCoord::new(tree_pos.x + 5, 0, tree_pos.z + 5);
    let home_id = insert_completed_home(&mut sim, anchor);

    // Get the home's bed nav node (this is the location mope should target).
    let bed_pos = sim
        .db
        .furniture
        .by_structure_id(&home_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|f| f.placed)
        .unwrap()
        .coord;
    let home_walkable = find_walkable(&sim, bed_pos, 10).unwrap();

    // Spawn an elf.
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

    let elf_id = *sim
        .db
        .creatures
        .iter_keys()
        .find(|id| sim.db.creatures.get(id).unwrap().species == Species::Elf)
        .unwrap();

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

    // Inject negative thoughts.
    for _ in 0..3 {
        sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
    }

    // Run heartbeats until mope triggers.
    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 10);

    // Find the mope task and verify its location is the home nav node.
    let mope_task = sim.db.tasks.iter_all().find(|t| {
        t.kind_tag == TaskKindTag::Mope
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(mope_task.is_some(), "Elf should have a Mope task");
    assert_eq!(
        mope_task.unwrap().location,
        home_walkable,
        "Mope task location should be the home's walkable position"
    );
}

// Old workshop tests (set_workshop_config_*, workshop_monitor_*, setup_workshop)
// have been removed — all crafting is now tested via the unified ActiveRecipe system.
// See the "Unified crafting commands" section below.

#[test]
fn moping_creates_notification() {
    // Set up with guaranteed moping (mean = 1 so it always fires).
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 1,
        mope_duration_ticks: 5000,
        ..Default::default()
    };
    let (mut sim, _elf_id) = mope_test_setup(
        cfg,
        // Two SleptOnGround thoughts push mood to Unhappy.
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );

    assert_eq!(sim.db.notifications.iter_all().count(), 0);

    // Run enough ticks for a heartbeat to fire and trigger moping.
    let elf_hb = sim.config.species[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + elf_hb + 1);

    // Should have a moping notification.
    assert!(
        sim.db.notifications.iter_all().count() > 0,
        "Expected a moping notification"
    );
    let notif = sim.db.notifications.iter_all().next().unwrap();
    assert!(
        notif.message.contains("moping"),
        "Notification should mention moping, got: {}",
        notif.message
    );
    assert!(
        notif.message.contains("Unhappy"),
        "Notification should mention mood tier, got: {}",
        notif.message
    );
}

#[test]
fn moping_notification_unnamed_elf_uses_generic_text() {
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 1,
        mope_duration_ticks: 5000,
        ..Default::default()
    };
    let (mut sim, elf_id) = mope_test_setup(
        cfg,
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );

    // Clear the elf's name to exercise the empty-name branch.
    let mut elf = sim.db.creatures.get(&elf_id).unwrap();
    elf.name = String::new();
    sim.db.update_creature(elf).unwrap();

    let elf_hb = sim.config.species[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + elf_hb + 1);

    assert!(
        sim.db.notifications.iter_all().count() > 0,
        "Expected a moping notification for unnamed elf"
    );
    let notif = sim.db.notifications.iter_all().next().unwrap();
    assert!(
        notif.message.starts_with("An elf is moping"),
        "Unnamed elf notification should use generic text, got: {}",
        notif.message
    );
}

/// High-priority #5: Mope progress increments by mope_action_ticks, not
/// by 1. A mope task with total_cost = 10000 and mope_action_ticks = 1000
/// completes after exactly 10 actions.
#[test]
fn mope_progress_increments_by_action_ticks() {
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000, // P ≈ 1.0
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000,
        mope_duration_ticks: 10_000,
        ..Default::default()
    };
    let (mut sim, _elf_id) = mope_test_setup(
        cfg,
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );
    sim.config.mope_action_ticks = 1000;

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    // Step enough to trigger mope.
    sim.step(&[], sim.tick + interval * 5);

    // Find the mope task.
    let mope_task = sim
        .db
        .tasks
        .iter_all()
        .find(|t| t.kind_tag == TaskKindTag::Mope);
    assert!(mope_task.is_some(), "Mope task should exist");
    let mope_task = mope_task.unwrap();

    // total_cost should be mope_duration_ticks (10000).
    assert_eq!(mope_task.total_cost, 10_000);

    // If still in progress, progress should be a multiple of mope_action_ticks.
    if mope_task.state == TaskState::InProgress {
        let remainder = mope_task.progress % 1000;
        assert_eq!(
            remainder, 0,
            "Mope progress should be a multiple of mope_action_ticks, got {}",
            mope_task.progress
        );
    }

    // If completed, verify progress >= total_cost.
    if mope_task.state == TaskState::Complete {
        assert!(mope_task.progress >= mope_task.total_cost);
    }
}

/// Medium-priority #9: Mope interrupts non-Move work action (Build).
/// Uses mope_mean_ticks = heartbeat interval (P ≈ 1.0 per heartbeat)
/// and runs 200 heartbeats to ensure at least one mope fires.
#[test]
fn mope_interrupts_build_action() {
    let mut config = test_config();
    config.build_work_ticks_per_voxel = 500_000; // Very long build.
    let heartbeat = config
        .species
        .get(&Species::Elf)
        .unwrap()
        .heartbeat_interval_ticks;
    config.mood_consequences = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: heartbeat,
        mope_mean_ticks_miserable: heartbeat,
        mope_mean_ticks_devastated: heartbeat,
        mope_can_interrupt_task: true,
        mope_duration_ticks: 100,
    };
    let elf_species = config.species.get_mut(&Species::Elf).unwrap();
    elf_species.food_decay_per_tick = 0;
    elf_species.rest_decay_per_tick = 0;
    let mut sim = SimState::with_config(fresh_test_seed(), config);
    let air_coord = find_air_adjacent_to_trunk(&sim);

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let cmd_spawn = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[cmd_spawn], 1);
    let elf_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap()
        .id;

    // Inject Devastated-tier thoughts.
    sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
    sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);
    sim.add_creature_thought(elf_id, ThoughtKind::SleptOnGround);

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

    // Run enough for elf to reach the build site and start building,
    // then for heartbeats to fire and trigger mope. P ≈ 1.0 per heartbeat
    // so 200 heartbeats should guarantee at least one mope fires.
    sim.step(&[], sim.tick + heartbeat * 200);

    let mope_ever_existed = sim
        .db
        .tasks
        .iter_all()
        .any(|t| t.kind_tag == TaskKindTag::Mope);
    assert!(
        mope_ever_existed,
        "Mope should have triggered at least once during 200 heartbeats with P≈1.0"
    );
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn unhappy_elf_eventually_mopes() {
    // Give elf SleptOnGround thoughts (weight -100 each → Unhappy/-200 → actually Miserable).
    // Use a high mope rate so it fires quickly.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000, // P ≈ 1.0 per heartbeat
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000,
        mope_duration_ticks: 100,
        ..Default::default()
    };
    let (mut sim, elf_id) = mope_test_setup(
        cfg,
        &[ThoughtKind::SleptOnGround, ThoughtKind::SleptOnGround],
    );

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 20);

    let has_mope = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::Mope
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(has_mope, "Unhappy elf should eventually get a Mope task");
}

#[test]
fn content_elf_never_mopes() {
    // Give elf positive thoughts → Content/Happy tier. Mean=0 → never mopes.
    let cfg = crate::config::MoodConsequencesConfig::default();
    let (mut sim, elf_id) = mope_test_setup(cfg, &[ThoughtKind::AteDining, ThoughtKind::AteDining]);

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 50);

    let has_mope = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::Mope
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(!has_mope, "Content elf should never mope");
}

#[test]
fn devastated_elf_interrupts_task_to_mope() {
    // Give elf Devastated-tier thoughts + a GoTo task + high mope rate.
    let cfg = crate::config::MoodConsequencesConfig {
        mope_mean_ticks_unhappy: 3000,
        mope_mean_ticks_miserable: 3000,
        mope_mean_ticks_devastated: 3000,
        mope_can_interrupt_task: true,
        mope_duration_ticks: 100,
    };
    let (mut sim, elf_id) = mope_test_setup(
        cfg,
        // SleptOnGround has weight -100, three of them → -300 → Devastated
        &[
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
            ThoughtKind::SleptOnGround,
        ],
    );

    // Assign a GoTo task to the elf so it's not idle.
    // Find a distant node for the GoTo task.
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    let far_pos = find_far_walkable(&sim, elf_pos, 10);
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
    {
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = Some(task_id);
        sim.db.update_creature(c).unwrap();
    }

    let interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + interval * 20);

    // Elf should have abandoned GoTo and started moping.
    let has_mope = sim.db.tasks.iter_all().any(|t| {
        t.kind_tag == TaskKindTag::Mope
            && sim
                .db
                .creatures
                .get(&elf_id)
                .is_some_and(|c| c.current_task == Some(t.id))
    });
    assert!(
        has_mope,
        "Miserable elf with mope_can_interrupt_task should interrupt GoTo and start moping"
    );
}
