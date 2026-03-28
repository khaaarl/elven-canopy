//! Tests for elf paths (Outcast/Warrior/Scout): path assignment, skill caps,
//! double advancement rolls, backfill logic, and serialization.
//! Corresponds to `sim/paths.rs`.

use super::*;

#[test]
fn path_id_serde_roundtrip() {
    for path_id in PathId::ALL {
        let json = serde_json::to_string(path_id).unwrap();
        let restored: PathId = serde_json::from_str(&json).unwrap();
        assert_eq!(*path_id, restored);
    }
}

#[test]
fn path_config_has_all_paths() {
    let config = GameConfig::default();
    for path_id in PathId::ALL {
        assert!(
            config.paths.paths.contains_key(path_id),
            "PathConfig missing definition for {path_id:?}"
        );
    }
}

#[test]
fn assign_path_command_sets_path() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    assert_eq!(sim.creature_path(elf_id), Some(PathId::Warrior));
    assert!(
        events.iter().any(
            |e| matches!(&e.kind, SimEventKind::PathAssigned { creature_id, path_id }
                if *creature_id == elf_id && *path_id == PathId::Warrior)
        ),
        "should emit PathAssigned event"
    );
}

#[test]
fn assign_path_replaces_existing() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);
    assert_eq!(sim.creature_path(elf_id), Some(PathId::Warrior));

    sim.assign_path(elf_id, PathId::Scout, &mut events);
    assert_eq!(sim.creature_path(elf_id), Some(PathId::Scout));
}

#[test]
fn assign_path_ignores_nonexistent_creature() {
    let mut sim = test_sim(42);
    let mut rng = crate::prng::GameRng::new(999);
    let fake_id = CreatureId::new(&mut rng);
    let mut events = Vec::new();

    sim.assign_path(fake_id, PathId::Warrior, &mut events);
    assert!(events.is_empty());
    assert_eq!(sim.creature_path(fake_id), None);
}

#[test]
fn skill_cap_elevated_for_path_skills() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // With Outcast path (default), skill cap is default (100).
    assert_eq!(
        sim.skill_cap_for(elf_id, TraitKind::Striking),
        sim.config.skills.default_skill_cap
    );

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Warrior path: Striking/Archery/Evasion get cap 200.
    assert_eq!(sim.skill_cap_for(elf_id, TraitKind::Striking), 200);
    assert_eq!(sim.skill_cap_for(elf_id, TraitKind::Archery), 200);
    assert_eq!(sim.skill_cap_for(elf_id, TraitKind::Evasion), 200);

    // Non-warrior skills still use default cap.
    assert_eq!(
        sim.skill_cap_for(elf_id, TraitKind::Beastcraft),
        sim.config.skills.default_skill_cap
    );
}

#[test]
fn skill_cap_scout_path() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Scout, &mut events);

    assert_eq!(sim.skill_cap_for(elf_id, TraitKind::Beastcraft), 200);
    assert_eq!(sim.skill_cap_for(elf_id, TraitKind::Ranging), 200);

    // Combat skills NOT elevated for scout.
    assert_eq!(
        sim.skill_cap_for(elf_id, TraitKind::Striking),
        sim.config.skills.default_skill_cap
    );
}

#[test]
fn extra_advancement_rolls_for_path_skills() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Outcast path -> 0 extra rolls.
    assert_eq!(sim.extra_advancement_rolls(elf_id, TraitKind::Striking), 0);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Warrior: 1 extra roll for associated skills.
    assert_eq!(sim.extra_advancement_rolls(elf_id, TraitKind::Striking), 1);
    assert_eq!(sim.extra_advancement_rolls(elf_id, TraitKind::Archery), 1);
    assert_eq!(sim.extra_advancement_rolls(elf_id, TraitKind::Evasion), 1);

    // Non-associated skill -> 0 extra rolls.
    assert_eq!(
        sim.extra_advancement_rolls(elf_id, TraitKind::Beastcraft),
        0
    );
}

#[test]
fn outcast_path_gives_no_bonuses() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Outcast, &mut events);

    // Outcast: all skills use default cap.
    assert_eq!(
        sim.skill_cap_for(elf_id, TraitKind::Striking),
        sim.config.skills.default_skill_cap
    );
    assert_eq!(
        sim.skill_cap_for(elf_id, TraitKind::Beastcraft),
        sim.config.skills.default_skill_cap
    );

    // Outcast: 0 extra rolls for all skills.
    assert_eq!(sim.extra_advancement_rolls(elf_id, TraitKind::Striking), 0);
    assert_eq!(
        sim.extra_advancement_rolls(elf_id, TraitKind::Beastcraft),
        0
    );
}

#[test]
fn elf_spawn_auto_assigns_outcast() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Elf should have Outcast path immediately after spawn.
    assert_eq!(
        sim.creature_path(elf_id),
        Some(PathId::Outcast),
        "spawned elf should have Outcast path"
    );
}

#[test]
fn backfill_outcast_paths_assigns_unpathed_elves() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Manually remove the path assignment to simulate an old save.
    let _ = sim.db.path_assignments.remove_no_fk(&elf_id);
    assert_eq!(sim.creature_path(elf_id), None);

    sim.backfill_outcast_paths();

    assert_eq!(
        sim.creature_path(elf_id),
        Some(PathId::Outcast),
        "backfill should assign Outcast to unpathed elves"
    );
}

#[test]
fn backfill_outcast_paths_skips_already_pathed() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Manually assign Warrior.
    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    sim.backfill_outcast_paths();

    // Warrior assignment should not be overwritten.
    assert_eq!(sim.creature_path(elf_id), Some(PathId::Warrior));
}

#[test]
fn backfill_outcast_paths_skips_non_elves() {
    let mut sim = test_sim(42);

    // Spawn a non-elf creature.
    let mut events = Vec::new();
    sim.spawn_creature(Species::Deer, VoxelCoord::new(32, 1, 32), &mut events);
    let deer_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Deer)
        .unwrap()
        .id;

    sim.backfill_outcast_paths();

    assert_eq!(
        sim.creature_path(deer_id),
        None,
        "non-elves should not get a path assignment"
    );
}

#[test]
fn assign_path_via_sim_command() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let cmd = crate::command::SimCommand {
        player_name: "test".to_string(),
        tick: sim.tick,
        action: crate::command::SimAction::AssignPath {
            creature_id: elf_id,
            path_id: PathId::Scout,
        },
    };

    let result = sim.step(&[cmd], sim.tick + 1);
    assert_eq!(sim.creature_path(elf_id), Some(PathId::Scout));
    assert!(result.events.iter().any(
        |e| matches!(&e.kind, SimEventKind::PathAssigned { creature_id, path_id }
            if creature_id == &elf_id && path_id == &PathId::Scout)
    ));
}

#[test]
fn assign_path_command_serde_roundtrip() {
    let mut rng = crate::prng::GameRng::new(1);
    let creature_id = CreatureId::new(&mut rng);

    let cmd = crate::command::SimCommand {
        player_name: "test".to_string(),
        tick: 42,
        action: crate::command::SimAction::AssignPath {
            creature_id,
            path_id: PathId::Warrior,
        },
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let restored: crate::command::SimCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(json, serde_json::to_string(&restored).unwrap());
}

#[test]
fn path_skill_cap_blocks_default_but_allows_path_cap() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Set Striking skill to default cap (100).
    sim.insert_trait(elf_id, TraitKind::Striking, TraitValue::Int(100));

    // Without path: try_advance_skill should not advance (at cap).
    let before = sim.trait_int(elf_id, TraitKind::Striking, 0);
    for _ in 0..100 {
        sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);
    }
    let after_no_path = sim.trait_int(elf_id, TraitKind::Striking, 0);
    assert_eq!(
        before, after_no_path,
        "should not advance past default cap without path"
    );

    // Assign Warrior path: cap goes to 200 for Striking.
    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Now advancement should be possible.
    for _ in 0..200 {
        sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);
    }
    let after_path = sim.trait_int(elf_id, TraitKind::Striking, 0);
    assert!(
        after_path > 100,
        "Warrior path should allow Striking past default cap, got {after_path}"
    );
}

#[test]
fn path_double_roll_increases_advancement() {
    // Statistical test: with Warrior path (double rolls), Striking should
    // advance faster than without a combat path. We compare advancement
    // of an associated skill vs a non-associated skill using the same
    // number of attempts.
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Set both skills to 0.
    sim.insert_trait(elf_id, TraitKind::Striking, TraitValue::Int(0));
    sim.insert_trait(elf_id, TraitKind::Cuisine, TraitValue::Int(0));
    // Set Intelligence to 0 so it doesn't skew results.
    sim.insert_trait(elf_id, TraitKind::Intelligence, TraitValue::Int(0));

    // Run many advancement attempts on both skills.
    for _ in 0..500 {
        sim.try_advance_skill(elf_id, TraitKind::Striking, 500);
        sim.try_advance_skill(elf_id, TraitKind::Cuisine, 500);
    }

    let striking = sim.trait_int(elf_id, TraitKind::Striking, 0);
    let cuisine = sim.trait_int(elf_id, TraitKind::Cuisine, 0);

    // Striking gets double rolls, so it should advance significantly more.
    assert!(
        striking > cuisine,
        "Warrior Striking ({striking}) should advance faster than non-path Cuisine ({cuisine})"
    );
}

#[test]
fn path_non_associated_skill_uses_default_cap() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Assign Warrior path.
    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Set Cuisine (non-warrior skill) to 100 (default cap).
    sim.insert_trait(elf_id, TraitKind::Cuisine, TraitValue::Int(100));

    // Should NOT advance past default cap.
    for _ in 0..100 {
        sim.try_advance_skill(elf_id, TraitKind::Cuisine, 1000);
    }
    let val = sim.trait_int(elf_id, TraitKind::Cuisine, 0);
    assert_eq!(
        val, 100,
        "non-associated skill should not exceed default cap"
    );
}

#[test]
fn path_assignment_survives_save_load() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);
    assert_eq!(sim.creature_path(elf_id), Some(PathId::Warrior));

    // Roundtrip through JSON.
    let json = sim.to_json().unwrap();
    let sim2 = SimState::from_json(&json).unwrap();

    assert_eq!(
        sim2.creature_path(elf_id),
        Some(PathId::Warrior),
        "Warrior path should survive save/load"
    );
}

#[test]
fn backfill_outcast_paths_via_from_json() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);
    assert_eq!(sim.creature_path(elf_id), Some(PathId::Outcast));

    // Serialize, strip path_assignments from JSON to simulate old save.
    let mut json_val: serde_json::Value = serde_json::from_str(&sim.to_json().unwrap()).unwrap();
    if let Some(db) = json_val.get_mut("db") {
        db.as_object_mut()
            .unwrap()
            .insert("path_assignments".to_string(), serde_json::json!([]));
    }
    let json = serde_json::to_string(&json_val).unwrap();

    // from_json should call backfill_outcast_paths.
    let sim2 = SimState::from_json(&json).unwrap();
    assert_eq!(
        sim2.creature_path(elf_id),
        Some(PathId::Outcast),
        "backfill in from_json should assign Outcast"
    );
}

#[test]
fn non_elf_spawn_has_no_path() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick.max(1),
        action: SimAction::SpawnCreature {
            species: Species::Deer,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], sim.tick.max(1) + 1);

    let deer_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Deer)
        .expect("deer should exist")
        .id;

    assert_eq!(
        sim.creature_path(deer_id),
        None,
        "non-elf creatures should not get a path at spawn"
    );
}

#[test]
fn assign_path_rejects_non_elf() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    let cmd = SimCommand {
        player_name: String::new(),
        tick: sim.tick.max(1),
        action: SimAction::SpawnCreature {
            species: Species::Deer,
            position: tree_pos,
        },
    };
    sim.step(&[cmd], sim.tick.max(1) + 1);

    let deer_id = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Deer)
        .unwrap()
        .id;

    let mut events = Vec::new();
    sim.assign_path(deer_id, PathId::Warrior, &mut events);
    assert!(events.is_empty(), "should not emit event for non-elf");
    assert_eq!(
        sim.creature_path(deer_id),
        None,
        "non-elf should not get path assignment"
    );
}

#[test]
fn path_prng_consumption_with_extra_rolls() {
    // Verify PRNG contract: Warrior (1 extra roll) consumes exactly 2 PRNG
    // calls per try_advance_skill, regardless of whether at cap.
    // Uses rng clone to verify consumption count without twin-sim pattern.
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Set Striking to cap (200) so the roll won't actually advance.
    sim.insert_trait(elf_id, TraitKind::Striking, TraitValue::Int(200));

    // Clone rng and advance it 2 times (expected: 1 base + 1 extra roll).
    let mut expected_rng = sim.rng.clone();
    let _ = expected_rng.next_u64();
    let _ = expected_rng.next_u64();

    // The actual call should consume exactly 2 PRNG calls.
    sim.try_advance_skill(elf_id, TraitKind::Striking, 500);

    assert_eq!(
        sim.rng.next_u64(),
        expected_rng.next_u64(),
        "try_advance_skill with Warrior path should consume exactly 2 PRNG calls"
    );

    // Also verify non-associated skill consumes exactly 1 call.
    let mut expected_rng2 = sim.rng.clone();
    let _ = expected_rng2.next_u64();

    sim.try_advance_skill(elf_id, TraitKind::Cuisine, 500);

    assert_eq!(
        sim.rng.next_u64(),
        expected_rng2.next_u64(),
        "try_advance_skill for non-associated skill should consume exactly 1 PRNG call"
    );
}

#[test]
fn path_config_serde_roundtrip() {
    let config = crate::config::PathConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let restored: crate::config::PathConfig = serde_json::from_str(&json).unwrap();

    // Verify all paths survived.
    for path_id in PathId::ALL {
        let orig = config.paths.get(path_id).unwrap();
        let rest = restored.paths.get(path_id).unwrap();
        assert_eq!(orig.name, rest.name);
        assert_eq!(orig.category, rest.category);
        assert_eq!(orig.skill_caps, rest.skill_caps);
        assert_eq!(orig.extra_advancement_rolls, rest.extra_advancement_rolls);
    }
}

#[test]
fn backfill_outcast_paths_includes_incapacitated() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Damage elf to 0 HP to incapacitate via DamageCreature command.
    let hp = sim.db.creatures.get(&elf_id).unwrap().hp;
    let tick = sim.tick + 1;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: hp,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().vital_status,
        VitalStatus::Incapacitated
    );

    // Remove path to simulate old save.
    let _ = sim.db.path_assignments.remove_no_fk(&elf_id);

    sim.backfill_outcast_paths();

    assert_eq!(
        sim.creature_path(elf_id),
        Some(PathId::Outcast),
        "incapacitated elves should also get backfilled"
    );
}

#[test]
fn backfill_outcast_paths_skips_dead_elves() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    // Kill the elf fully (damage past -hp_max).
    let mut events = Vec::new();
    sim.handle_creature_death(elf_id, DeathCause::Debug, &mut events);
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().vital_status,
        VitalStatus::Dead
    );

    // Remove path to simulate old save.
    let _ = sim.db.path_assignments.remove_no_fk(&elf_id);

    sim.backfill_outcast_paths();

    assert_eq!(
        sim.creature_path(elf_id),
        None,
        "dead elves should NOT get backfilled"
    );
}

#[test]
fn extra_rolls_can_increment_skill_twice_per_call() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Start at 0, use max probability so both rolls succeed.
    sim.insert_trait(elf_id, TraitKind::Striking, TraitValue::Int(0));
    sim.insert_trait(elf_id, TraitKind::Intelligence, TraitValue::Int(0));

    sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);

    let val = sim.trait_int(elf_id, TraitKind::Striking, 0);
    assert_eq!(
        val, 2,
        "both rolls should succeed at 1000 permille, advancing by 2"
    );
}

#[test]
fn extra_rolls_never_exceed_cap() {
    let mut sim = test_sim(42);
    let elf_id = spawn_test_elf(&mut sim);

    let mut events = Vec::new();
    sim.assign_path(elf_id, PathId::Warrior, &mut events);

    // Set to cap (200). Even with many attempts at max probability,
    // skill should never exceed 200.
    sim.insert_trait(elf_id, TraitKind::Striking, TraitValue::Int(200));

    for _ in 0..100 {
        // Clone rng to verify 2 calls consumed each time.
        let mut expected_rng = sim.rng.clone();
        let _ = expected_rng.next_u64();
        let _ = expected_rng.next_u64();

        sim.try_advance_skill(elf_id, TraitKind::Striking, 1000);

        assert_eq!(
            sim.rng.next_u64(),
            expected_rng.next_u64(),
            "should consume exactly 2 PRNG calls even at cap"
        );
    }

    assert_eq!(
        sim.trait_int(elf_id, TraitKind::Striking, 0),
        200,
        "skill should never exceed path cap"
    );
}

#[test]
fn path_id_short_name() {
    assert_eq!(PathId::Outcast.short_name(), "Outcast");
    assert_eq!(PathId::Warrior.short_name(), "Warrior");
    assert_eq!(PathId::Scout.short_name(), "Scout");
    // short_name is a substring of display_name for all variants.
    for &path_id in PathId::ALL {
        assert!(
            path_id.display_name().contains(path_id.short_name()),
            "{:?}: short_name '{}' not in display_name '{}'",
            path_id,
            path_id.short_name(),
            path_id.display_name()
        );
    }
}
