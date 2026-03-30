//! Tests for creature spawning, biology traits, species data, creature stats,
//! and flying creature (hornet/wyvern) spawning. Corresponds to `sim/creature.rs`.

use super::*;

// -----------------------------------------------------------------------
// Spawn / name tests
// -----------------------------------------------------------------------

#[test]
fn spawn_elf_command() {
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

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Elf), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Elf,
            ..
        }
    )));
}

#[test]
fn spawned_elf_has_vaelith_name() {
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
    assert_eq!(sim.creature_count(Species::Elf), 1);

    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .expect("elf should exist");

    // Elf should have a non-empty Vaelith name with given + surname.
    assert!(!elf.name.is_empty(), "Elf should have a name");
    assert!(
        elf.name.contains(' '),
        "Name '{}' should contain a space (given + surname)",
        elf.name
    );
    assert!(
        !elf.name_meaning.is_empty(),
        "Elf should have a name meaning"
    );
}

#[test]
fn spawned_elf_name_is_deterministic() {
    // Same seed should produce the same elf name.
    let mut sim1 = test_sim(42);
    let mut sim2 = test_sim(42);
    let tree_pos = sim1.db.trees.get(&sim1.player_tree_id).unwrap().position;

    let cmd1 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };
    let cmd2 = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            species: Species::Elf,
            position: tree_pos,
        },
    };

    sim1.step(&[cmd1], 2);
    sim2.step(&[cmd2], 2);

    let elf1 = sim1.db.creatures.iter_all().next().unwrap();
    let elf2 = sim2.db.creatures.iter_all().next().unwrap();
    assert_eq!(elf1.name, elf2.name);
    assert_eq!(elf1.name_meaning, elf2.name_meaning);
}

#[test]
fn spawned_non_elf_has_no_name() {
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
    let capy = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .expect("capybara should exist");

    // Non-elf creatures should not have Vaelith names.
    assert!(capy.name.is_empty(), "Capybara should not have a name");
}

// ---------------------------------------------------------------------------
// Creature biology traits
// ---------------------------------------------------------------------------

#[test]
fn spawned_elf_has_biology_traits() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Elf should have all elf-specific traits plus BioSeed.
    assert_ne!(sim.trait_int(elf_id, TraitKind::BioSeed, 0), 0);
    // Hair color index should be in range 0–6.
    let hair = sim.trait_int(elf_id, TraitKind::HairColor, -1);
    assert!((0..7).contains(&hair), "hair_color {hair} out of range");
    let eye = sim.trait_int(elf_id, TraitKind::EyeColor, -1);
    assert!((0..5).contains(&eye), "eye_color {eye} out of range");
    let skin = sim.trait_int(elf_id, TraitKind::SkinTone, -1);
    assert!((0..4).contains(&skin), "skin_tone {skin} out of range");
    let style = sim.trait_int(elf_id, TraitKind::HairStyle, -1);
    assert!((0..3).contains(&style), "hair_style {style} out of range");

    // Elf should NOT have non-elf traits.
    assert_eq!(sim.trait_int(elf_id, TraitKind::BodyColor, -1), -1);
    assert_eq!(sim.trait_int(elf_id, TraitKind::AntlerStyle, -1), -1);
}

#[test]
fn spawned_capybara_has_biology_traits() {
    let mut sim = test_sim(42);
    let capy_id = spawn_creature(&mut sim, Species::Capybara);

    assert_ne!(sim.trait_int(capy_id, TraitKind::BioSeed, 0), 0);
    let body = sim.trait_int(capy_id, TraitKind::BodyColor, -1);
    assert!((0..4).contains(&body), "body_color {body} out of range");
    let acc = sim.trait_int(capy_id, TraitKind::Accessory, -1);
    assert!((0..4).contains(&acc), "accessory {acc} out of range");

    // Should NOT have elf traits.
    assert_eq!(sim.trait_int(capy_id, TraitKind::HairColor, -1), -1);
}

#[test]
fn spawned_deer_has_biology_traits() {
    let mut sim = test_sim(42);
    let deer_id = spawn_creature(&mut sim, Species::Deer);

    assert_ne!(sim.trait_int(deer_id, TraitKind::BioSeed, 0), 0);
    let body = sim.trait_int(deer_id, TraitKind::BodyColor, -1);
    assert!((0..4).contains(&body), "body_color {body} out of range");
    let antler = sim.trait_int(deer_id, TraitKind::AntlerStyle, -1);
    assert!(
        (0..3).contains(&antler),
        "antler_style {antler} out of range"
    );
    let spots = sim.trait_int(deer_id, TraitKind::SpotPattern, -1);
    assert!((0..2).contains(&spots), "spot_pattern {spots} out of range");
}

#[test]
fn biology_traits_deterministic() {
    let mut sim1 = test_sim(42);
    let mut sim2 = test_sim(42);
    let elf1 = spawn_creature(&mut sim1, Species::Elf);
    let elf2 = spawn_creature(&mut sim2, Species::Elf);

    assert_eq!(
        sim1.trait_int(elf1, TraitKind::HairColor, -1),
        sim2.trait_int(elf2, TraitKind::HairColor, -1),
    );
    assert_eq!(
        sim1.trait_int(elf1, TraitKind::EyeColor, -1),
        sim2.trait_int(elf2, TraitKind::EyeColor, -1),
    );
    assert_eq!(
        sim1.trait_int(elf1, TraitKind::BioSeed, 0),
        sim2.trait_int(elf2, TraitKind::BioSeed, 0),
    );
}

#[test]
fn biology_traits_cascade_on_creature_removal() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Traits should exist.
    assert_ne!(sim.trait_int(elf_id, TraitKind::BioSeed, 0), 0);
    let trait_count_before = sim
        .db
        .creature_traits
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
        .len();
    assert!(trait_count_before > 0, "should have traits after spawn");

    // Removing the creature via the DB should cascade-delete all traits.
    // First remove FKs that would block creature removal (tasks, inventory, etc.).
    let creature = sim.db.creatures.get(&elf_id).unwrap().clone();
    if let Some(task_id) = creature.current_task {
        // Clear creature's FK before removing the task.
        let mut c = sim.db.creatures.get(&elf_id).unwrap();
        c.current_task = None;
        sim.db.update_creature(c).unwrap();
        sim.db.remove_task(&task_id).unwrap();
    }
    let inv_id = creature.inventory_id;
    // Remove all logistics_want_rows referencing the inventory (FK blocker).
    let want_keys: Vec<_> = sim
        .db
        .logistics_want_rows
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|r| (r.inventory_id, r.seq))
        .collect();
    for key in want_keys {
        sim.db.remove_logistics_want_row(&key).unwrap();
    }
    // Remove all item stacks in the creature's inventory.
    let stack_ids: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .iter()
        .map(|s| s.id)
        .collect();
    for sid in stack_ids {
        sim.db.remove_item_stack(&sid).unwrap();
    }
    sim.db.remove_inventory(&inv_id).unwrap();
    sim.db
        .remove_creature(&elf_id)
        .expect("creature removal should succeed");
    assert_eq!(sim.trait_int(elf_id, TraitKind::BioSeed, 0), 0);
    assert_eq!(
        sim.db
            .creature_traits
            .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC)
            .len(),
        0
    );
}

#[test]
fn trait_int_returns_default_for_missing_trait() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    // TraitKind::WarPaint is not set for elves.
    assert_eq!(sim.trait_int(elf_id, TraitKind::WarPaint, 42), 42);
}

#[test]
fn trait_int_returns_default_for_text_value() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Manually insert a text-valued trait to verify fallback.
    sim.db
        .insert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf_id,
            trait_kind: TraitKind::WarPaint,
            value: TraitValue::Text("blue".into()),
        })
        .unwrap();

    // trait_int should return the default since the value is Text.
    assert_eq!(sim.trait_int(elf_id, TraitKind::WarPaint, 99), 99);
    // trait_text should return the text.
    assert_eq!(sim.trait_text(elf_id, TraitKind::WarPaint, ""), "blue");
}

#[test]
fn compound_unique_prevents_duplicate_traits() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Trying to insert a second HairColor should fail (duplicate PK).
    let result = sim.db.insert_creature_trait(crate::db::CreatureTrait {
        creature_id: elf_id,
        trait_kind: TraitKind::HairColor,
        value: TraitValue::Int(99),
    });
    assert!(result.is_err(), "duplicate trait should be rejected");
}

#[test]
fn all_species_get_bio_seed_on_spawn() {
    // Ground species only — Hornet (flying) can't spawn at tree_pos (Trunk).
    // Hornet BioSeed is verified by `hornet_spawn_has_traits`.
    let all_species = [
        Species::Elf,
        Species::Capybara,
        Species::Boar,
        Species::Deer,
        Species::Elephant,
        Species::Goblin,
        Species::Monkey,
        Species::Orc,
        Species::Squirrel,
        Species::Troll,
    ];
    for species in all_species {
        let mut sim = test_sim(42);
        let id = spawn_creature(&mut sim, species);
        assert_ne!(
            sim.trait_int(id, TraitKind::BioSeed, 0),
            0,
            "{species:?} should have a BioSeed"
        );
    }
}

#[test]
fn biology_traits_serde_roundtrip() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    let hair_before = sim.trait_int(elf_id, TraitKind::HairColor, -1);
    let eye_before = sim.trait_int(elf_id, TraitKind::EyeColor, -1);
    let seed_before = sim.trait_int(elf_id, TraitKind::BioSeed, 0);

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    assert_eq!(
        restored.trait_int(elf_id, TraitKind::HairColor, -1),
        hair_before
    );
    assert_eq!(
        restored.trait_int(elf_id, TraitKind::EyeColor, -1),
        eye_before
    );
    assert_eq!(
        restored.trait_int(elf_id, TraitKind::BioSeed, 0),
        seed_before
    );
}

#[test]
fn trait_kind_serde_roundtrip() {
    let all_kinds = [
        TraitKind::BioSeed,
        TraitKind::HairColor,
        TraitKind::EyeColor,
        TraitKind::SkinTone,
        TraitKind::HairStyle,
        TraitKind::BodyColor,
        TraitKind::AntlerStyle,
        TraitKind::SpotPattern,
        TraitKind::TuskSize,
        TraitKind::HornStyle,
        TraitKind::FurColor,
        TraitKind::TailType,
        TraitKind::TuskType,
        TraitKind::FaceMarking,
        TraitKind::WarPaint,
        TraitKind::EarStyle,
        TraitKind::SkinColor,
        TraitKind::Accessory,
        TraitKind::StripePattern,
        TraitKind::WingStyle,
        TraitKind::ScalePattern,
        TraitKind::Striking,
        TraitKind::Archery,
        TraitKind::Evasion,
        TraitKind::Ranging,
        TraitKind::Herbalism,
        TraitKind::Beastcraft,
        TraitKind::Cuisine,
        TraitKind::Tailoring,
        TraitKind::Woodcraft,
        TraitKind::Alchemy,
        TraitKind::Singing,
        TraitKind::Channeling,
        TraitKind::Literature,
        TraitKind::Art,
        TraitKind::Influence,
        TraitKind::Culture,
        TraitKind::Counsel,
        TraitKind::Strength,
        TraitKind::Agility,
        TraitKind::Dexterity,
        TraitKind::Constitution,
        TraitKind::Willpower,
        TraitKind::Intelligence,
        TraitKind::Perception,
        TraitKind::Charisma,
    ];
    for kind in all_kinds {
        let json = serde_json::to_string(&kind).unwrap();
        let restored: TraitKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, restored, "serde roundtrip failed for {kind:?}");
    }
}

#[test]
fn trait_value_serde_roundtrip() {
    let values = [
        TraitValue::Int(42),
        TraitValue::Int(-1),
        TraitValue::Text("blue".into()),
    ];
    for val in values {
        let json = serde_json::to_string(&val).unwrap();
        let restored: TraitValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, restored);
    }
}

#[test]
fn spawned_boar_has_biology_traits() {
    let mut sim = test_sim(42);
    let id = spawn_creature(&mut sim, Species::Boar);
    let body = sim.trait_int(id, TraitKind::BodyColor, -1);
    assert!((0..4).contains(&body), "body_color {body} out of range");
    let tusk = sim.trait_int(id, TraitKind::TuskSize, -1);
    assert!((0..3).contains(&tusk), "tusk_size {tusk} out of range");
    assert_eq!(sim.trait_int(id, TraitKind::HairColor, -1), -1);
}

#[test]
fn spawned_elephant_has_biology_traits() {
    let mut sim = test_sim(42);
    let id = spawn_creature(&mut sim, Species::Elephant);
    let body = sim.trait_int(id, TraitKind::BodyColor, -1);
    assert!((0..4).contains(&body), "body_color {body} out of range");
    let tusk = sim.trait_int(id, TraitKind::TuskType, -1);
    assert!((0..3).contains(&tusk), "tusk_type {tusk} out of range");
}

#[test]
fn spawned_goblin_has_biology_traits() {
    let mut sim = test_sim(42);
    let id = spawn_creature(&mut sim, Species::Goblin);
    let skin = sim.trait_int(id, TraitKind::SkinColor, -1);
    assert!((0..4).contains(&skin), "skin_color {skin} out of range");
    let ear = sim.trait_int(id, TraitKind::EarStyle, -1);
    assert!((0..3).contains(&ear), "ear_style {ear} out of range");
    assert_eq!(sim.trait_int(id, TraitKind::HairColor, -1), -1);
}

#[test]
fn spawned_monkey_has_biology_traits() {
    let mut sim = test_sim(42);
    let id = spawn_creature(&mut sim, Species::Monkey);
    let fur = sim.trait_int(id, TraitKind::FurColor, -1);
    assert!((0..4).contains(&fur), "fur_color {fur} out of range");
    let face = sim.trait_int(id, TraitKind::FaceMarking, -1);
    assert!((0..3).contains(&face), "face_marking {face} out of range");
}

#[test]
fn spawned_orc_has_biology_traits() {
    let mut sim = test_sim(42);
    let id = spawn_creature(&mut sim, Species::Orc);
    let skin = sim.trait_int(id, TraitKind::SkinColor, -1);
    assert!((0..4).contains(&skin), "skin_color {skin} out of range");
    let paint = sim.trait_int(id, TraitKind::WarPaint, -1);
    assert!((0..3).contains(&paint), "war_paint {paint} out of range");
}

#[test]
fn spawned_squirrel_has_biology_traits() {
    let mut sim = test_sim(42);
    let id = spawn_creature(&mut sim, Species::Squirrel);
    let fur = sim.trait_int(id, TraitKind::FurColor, -1);
    assert!((0..4).contains(&fur), "fur_color {fur} out of range");
    let tail = sim.trait_int(id, TraitKind::TailType, -1);
    assert!((0..3).contains(&tail), "tail_type {tail} out of range");
}

#[test]
fn spawned_troll_has_biology_traits() {
    let mut sim = test_sim(42);
    let id = spawn_creature(&mut sim, Species::Troll);
    let skin = sim.trait_int(id, TraitKind::SkinColor, -1);
    assert!((0..4).contains(&skin), "skin_color {skin} out of range");
    let horn = sim.trait_int(id, TraitKind::HornStyle, -1);
    assert!((0..3).contains(&horn), "horn_style {horn} out of range");
}

#[test]
fn trait_text_returns_default_for_missing_trait() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    // TraitKind::WarPaint is not set for elves — trait_text should return default.
    assert_eq!(sim.trait_text(elf_id, TraitKind::WarPaint, "none"), "none");
}

#[test]
fn trait_text_returns_default_for_int_value() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    // HairColor is stored as Int — trait_text should return the default string.
    assert_eq!(
        sim.trait_text(elf_id, TraitKind::HairColor, "fallback"),
        "fallback"
    );
}

// ---------------------------------------------------------------------------
// Old save backward compatibility
// ---------------------------------------------------------------------------

#[test]
fn old_save_without_creature_traits_deserializes() {
    // Simulate loading a save that predates creature biology.
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let json = serde_json::to_string(&sim).unwrap();

    // Strip the creature_traits key from the JSON to simulate an old save.
    let mut parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    if let Some(db) = parsed.get_mut("db") {
        db.as_object_mut().unwrap().remove("creature_traits");
    }
    let stripped = serde_json::to_string(&parsed).unwrap();

    let restored: SimState = serde_json::from_str(&stripped).unwrap();
    // Creatures should exist but have no traits — defaults returned.
    assert!(restored.db.creatures.get(&elf_id).is_some());
    assert_eq!(restored.trait_int(elf_id, TraitKind::HairColor, -1), -1);
    assert_eq!(restored.trait_int(elf_id, TraitKind::BioSeed, 0), 0);
}

/// Verify that old-format saves (auto-PK tables serialized as
/// `{"next_id": N, "rows": [...]}`) load correctly after the
/// F-child-table-pks migration. Tables that changed from auto-PK
/// to plain/compound/parent PK now serialize as `[...]`, but the
/// backward-compat deserializer must still accept the old format.
#[test]
fn old_save_format_backward_compat_for_converted_tables() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    // Insert a thought to populate the thoughts table.
    sim.tick = 1000;
    sim.add_creature_thought(elf_id, ThoughtKind::AteDining);

    let json = serde_json::to_string(&sim).unwrap();
    let mut parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Convert plain tables (creature_traits, civ_relationships) from new
    // array format back to old auto-PK format: {"next_id": N, "rows": [...]}.
    let db = parsed.get_mut("db").unwrap().as_object_mut().unwrap();
    for table_name in &[
        "creature_traits",
        "civ_relationships",
        "task_haul_data",
        "task_sleep_data",
        "task_acquire_data",
        "task_craft_data",
        "task_attack_target_data",
        "task_attack_move_data",
    ] {
        if let Some(val) = db.get(*table_name) {
            if val.is_array() {
                let rows = val.clone();
                let wrapper = serde_json::json!({
                    "next_id": rows.as_array().map_or(0, |a| a.len()),
                    "rows": rows,
                });
                db.insert(table_name.to_string(), wrapper);
            }
        }
    }

    // Also simulate old format for nonpk_auto tables: rename "next_seq" to
    // "next_id" (old auto-PK counter name). The deserializer should ignore
    // the unrecognized "next_id" and recompute from max(seq) + 1.
    for table_name in &[
        "thoughts",
        "task_blueprint_refs",
        "task_structure_refs",
        "task_voxel_refs",
        "logistics_want_rows",
        "item_subcomponents",
        "enchantment_effects",
    ] {
        if let Some(val) = db.get(*table_name) {
            if let Some(obj) = val.as_object() {
                let mut new_obj = obj.clone();
                if let Some(counter) = new_obj.remove("next_seq") {
                    new_obj.insert("next_id".to_string(), counter);
                }
                db.insert(table_name.to_string(), serde_json::Value::Object(new_obj));
            }
        }
    }

    let old_format_json = serde_json::to_string(&parsed).unwrap();
    let restored: SimState = serde_json::from_str(&old_format_json).unwrap();

    // Verify creatures and traits survived.
    assert!(restored.db.creatures.get(&elf_id).is_some());
    let traits = restored
        .db
        .creature_traits
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC);
    assert!(!traits.is_empty(), "traits should survive old-format load");

    // Verify thoughts survived (nonpk_auto table with renamed counter).
    let thoughts = restored
        .db
        .thoughts
        .by_creature_id(&elf_id, tabulosity::QueryOpts::ASC);
    assert_eq!(thoughts.len(), 1);
    assert_eq!(thoughts[0].kind, ThoughtKind::AteDining);
}

// ---------------------------------------------------------------------------
// Creature stats (F-creature-stats)
// ---------------------------------------------------------------------------

#[test]
fn creature_stats_rolled_at_spawn() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // All 8 stats should be present as Int traits.
    for &kind in &crate::stats::STAT_TRAIT_KINDS {
        let val = sim.trait_int(elf_id, kind, i64::MIN);
        assert_ne!(val, i64::MIN, "{kind:?} should be set at spawn");
    }
}

#[test]
fn creature_stats_deterministic() {
    let mut sim1 = test_sim(42);
    let mut sim2 = test_sim(42);
    let elf1 = spawn_creature(&mut sim1, Species::Elf);
    let elf2 = spawn_creature(&mut sim2, Species::Elf);

    for &kind in &crate::stats::STAT_TRAIT_KINDS {
        assert_eq!(
            sim1.trait_int(elf1, kind, 0),
            sim2.trait_int(elf2, kind, 0),
            "{kind:?} should be deterministic"
        );
    }
}

#[test]
fn constitution_modifies_hp_max() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let con = sim.trait_int(elf_id, TraitKind::Constitution, 0);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let base_hp = sim.species_table[&Species::Elf].hp_max;
    let expected_hp = crate::stats::apply_stat_multiplier(base_hp, con);
    assert_eq!(elf.hp_max, expected_hp);
    assert_eq!(elf.hp, expected_hp); // spawns at full HP
}

#[test]
fn stat_zero_preserves_baseline() {
    // With all stats at 0, behavior matches species base values.
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    zero_creature_stats(&mut sim, elf_id);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let base_hp = sim.species_table[&Species::Elf].hp_max;
    assert_eq!(elf.hp_max, base_hp);
    assert_eq!(elf.hp, base_hp);
}

#[test]
fn all_species_get_stats_on_spawn() {
    // Ground species only — Hornet (flying) can't spawn at tree_pos (Trunk).
    // Hornet stats are verified by hornet_spawn_has_traits + hornet_species_in_config.
    let all_species = [
        Species::Elf,
        Species::Capybara,
        Species::Boar,
        Species::Deer,
        Species::Elephant,
        Species::Goblin,
        Species::Monkey,
        Species::Orc,
        Species::Squirrel,
        Species::Troll,
    ];
    for species in all_species {
        let mut sim = test_sim(42);
        let id = spawn_creature(&mut sim, species);
        for &kind in &crate::stats::STAT_TRAIT_KINDS {
            let val = sim.trait_int(id, kind, i64::MIN);
            assert_ne!(val, i64::MIN, "{species:?} should have {kind:?} at spawn");
        }
    }
}

#[test]
fn strength_modifies_melee_damage() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    let base_damage = sim.species_table[&Species::Goblin].melee_damage;
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    // Set goblin STR to +10 (doubles damage).
    assert!(
        sim.db
            .creature_traits
            .contains(&(goblin, TraitKind::Strength))
    );
    let mut t = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Strength))
        .unwrap();
    t.value = TraitValue::Int(100);
    sim.db.update_creature_trait(t).unwrap();

    // Position them adjacent and make the goblin strike.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    let mut events = Vec::new();
    sim.try_melee_strike(goblin, elf, &mut events);

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    let expected_damage = crate::stats::apply_stat_multiplier(base_damage, 100);
    assert_eq!(
        elf_hp_before - elf_hp_after,
        expected_damage,
        "STR +100 should double damage: base={base_damage}, expected={expected_damage}"
    );
}

#[test]
fn troll_spawn_hp_approximately_400() {
    // Troll has CON mean +200 (4x multiplier on base 100), so effective HP
    // should be approximately 400. Allow variance from stochastic stat roll.
    let mut sim = test_sim(42);
    let troll = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll).unwrap().hp_max;
    assert!(
        hp_max >= 200 && hp_max <= 800,
        "Troll hp_max should be approximately 400 (got {hp_max})"
    );
}

#[test]
fn uniform_base_stats_all_species() {
    // All species should share the same base hp_max and walk_ticks_per_voxel.
    let config = GameConfig::default();
    for (&species, data) in &config.species {
        assert_eq!(
            data.hp_max, 100,
            "{species:?} should have uniform base hp_max=100"
        );
        assert_eq!(
            data.walk_ticks_per_voxel, 500,
            "{species:?} should have uniform base walk_ticks_per_voxel=500"
        );
    }
}

#[test]
fn effective_detection_range_sq_passive_returns_zero() {
    // Passive species (base=0) should always return 0, regardless of Perception.
    let mut sim = test_sim(42);
    let capy = spawn_creature(&mut sim, Species::Capybara);
    let range = sim.effective_detection_range_sq(capy, Species::Capybara);
    assert_eq!(range, 0, "Passive species should have 0 detection range");
}

#[test]
fn effective_detection_range_sq_perception_zero_returns_base() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    let base = sim.species_table[&Species::Goblin].hostile_detection_range_sq;
    let range = sim.effective_detection_range_sq(goblin, Species::Goblin);
    assert_eq!(
        range, base,
        "Perception 0 should return base detection range"
    );
}

#[test]
fn effective_detection_range_sq_high_perception_extends() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    let mut t = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Perception))
        .unwrap();
    t.value = TraitValue::Int(200);
    sim.db.update_creature_trait(t).unwrap();
    let base = sim.species_table[&Species::Goblin].hostile_detection_range_sq;
    let range = sim.effective_detection_range_sq(goblin, Species::Goblin);
    // PER +200 = 4x linear range → 16x squared range (applied twice).
    let once = crate::stats::apply_stat_multiplier(base, 200);
    let expected = crate::stats::apply_stat_multiplier(once, 200);
    assert_eq!(range, expected);
    assert!(range > base, "PER +200 should extend detection range");
}

#[test]
fn effective_detection_range_sq_low_perception_shrinks() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    let mut t = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Perception))
        .unwrap();
    t.value = TraitValue::Int(-200);
    sim.db.update_creature_trait(t).unwrap();
    let base = sim.species_table[&Species::Goblin].hostile_detection_range_sq;
    let range = sim.effective_detection_range_sq(goblin, Species::Goblin);
    assert!(range > 0, "Detection range should be clamped to >= 1");
    assert!(range < base, "PER -200 should shrink detection range");
}

#[test]
fn effective_detection_range_sq_per_100_quadruples_squared() {
    // PER +100 should double the linear radius, which means quadrupling the
    // squared range. This verifies the double-application invariant.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    let mut t = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Perception))
        .unwrap();
    t.value = TraitValue::Int(100);
    sim.db.update_creature_trait(t).unwrap();
    let base = sim.species_table[&Species::Goblin].hostile_detection_range_sq; // 225
    let range = sim.effective_detection_range_sq(goblin, Species::Goblin);
    // PER +100 doubles linear radius → 4x squared range.
    assert_eq!(range, base * 4, "PER +100 should quadruple squared range");
}

#[test]
fn stat_modified_hp_max_survives_serde_roundtrip() {
    // Verify that a creature with CON-modified hp_max survives serialization.
    let mut sim = test_sim(42);
    let troll = spawn_creature(&mut sim, Species::Troll);
    let hp_max_before = sim.db.creatures.get(&troll).unwrap().hp_max;
    assert!(hp_max_before > 100, "Troll should have CON-boosted HP");

    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: crate::db::SimDb = serde_json::from_str(&json).unwrap();
    let hp_max_after = restored.creatures.get(&troll).unwrap().hp_max;
    assert_eq!(
        hp_max_before, hp_max_after,
        "hp_max should survive serde roundtrip"
    );
}

#[test]
fn disengage_uses_creature_hp_max_not_species_base() {
    // A creature with high CON has stat-modified hp_max >> species base (100).
    // The disengage check must use creature.hp_max, not the uniform base.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);

    // Give this goblin CON +200 → hp_max ≈ 400 (4x base 100).
    let mut t = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Constitution))
        .unwrap();
    t.value = TraitValue::Int(200);
    sim.db.update_creature_trait(t).unwrap();
    let effective_hp = crate::stats::apply_stat_multiplier(100, 200);
    let mut c = sim.db.creatures.get(&goblin).unwrap();
    c.hp_max = effective_hp;
    c.hp = effective_hp;
    sim.db.update_creature(c).unwrap();

    // Set disengage threshold to 50%. With hp_max=400, hp=150 is 37.5% → flee.
    // With species base 100, hp=150 would be 150% → would NOT flee (bug).
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .engagement_style
        .disengage_threshold_pct = 50;
    let mut c = sim.db.creatures.get(&goblin).unwrap();
    c.hp = 150;
    sim.db.update_creature(c).unwrap();

    assert!(
        sim.should_flee(goblin, Species::Goblin),
        "Goblin at 150/400 HP (37.5%) should flee with 50% threshold"
    );
}

#[test]
fn stat_serde_roundtrip() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    let str_before = sim.trait_int(elf_id, TraitKind::Strength, 0);
    let agi_before = sim.trait_int(elf_id, TraitKind::Agility, 0);

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    assert_eq!(
        restored.trait_int(elf_id, TraitKind::Strength, 0),
        str_before
    );
    assert_eq!(
        restored.trait_int(elf_id, TraitKind::Agility, 0),
        agi_before
    );
}

// ---------------------------------------------------------------------------
// spawn_initial_creatures
// ---------------------------------------------------------------------------

/// Build a test config with known initial creatures for a 64x64x64 world.
fn initial_spawn_test_config() -> GameConfig {
    use crate::config::{InitialCreatureSpec, InitialGroundPileSpec};
    let mut config = test_config();
    config.elf_starting_bread = 0; // Isolate from starting bread feature.
    config.initial_creatures = vec![
        InitialCreatureSpec {
            species: Species::Elf,
            count: 2,
            spawn_position: VoxelCoord::new(32, 1, 32),
            food_pcts: vec![100, 50],
            rest_pcts: vec![80, 40],
            bread_counts: vec![0, 3],
            initial_equipment: vec![],
        },
        InitialCreatureSpec {
            species: Species::Capybara,
            count: 1,
            spawn_position: VoxelCoord::new(32, 1, 32),
            food_pcts: vec![],
            rest_pcts: vec![],
            bread_counts: vec![],
            initial_equipment: vec![],
        },
    ];
    config.initial_ground_piles = vec![InitialGroundPileSpec {
        position: VoxelCoord::new(32, 1, 34),
        item_kind: crate::inventory::ItemKind::Bread,
        quantity: 5,
        material: None,
        dye_color: None,
    }];
    config
}

#[test]
fn spawn_initial_creatures_populates() {
    let config = initial_spawn_test_config();
    let mut sim = SimState::with_config(42, config);
    let mut events = Vec::new();
    sim.spawn_initial_creatures(&mut events);

    let elf_count = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .count();
    let capy_count = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Capybara)
        .count();
    assert_eq!(elf_count, 2);
    assert_eq!(capy_count, 1);
    assert_eq!(sim.db.creatures.len(), 3);

    // Should have emitted CreatureArrived events for all 3.
    let arrived: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, SimEventKind::CreatureArrived { .. }))
        .collect();
    assert_eq!(arrived.len(), 3);
}

#[test]
fn spawn_initial_creatures_sets_food_rest() {
    let config = initial_spawn_test_config();
    let mut sim = SimState::with_config(42, config);
    let mut events = Vec::new();
    sim.spawn_initial_creatures(&mut events);

    let elf_food_max = sim.species_table[&Species::Elf].food_max;
    let elf_rest_max = sim.species_table[&Species::Elf].rest_max;

    let mut elves: Vec<_> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elf)
        .collect();
    // Sort by food descending to identify first (100%) vs second (50%).
    elves.sort_by(|a, b| b.food.cmp(&a.food));

    assert_eq!(elves[0].food, elf_food_max * 100 / 100);
    assert_eq!(elves[0].rest, elf_rest_max * 80 / 100);
    assert_eq!(elves[1].food, elf_food_max * 50 / 100);
    assert_eq!(elves[1].rest, elf_rest_max * 40 / 100);

    // Second elf should have 3 bread.
    let bread_count = sim.inv_item_count(
        sim.creature_inv(elves[1].id),
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(bread_count, 3);
}

#[test]
fn spawn_initial_creatures_ground_piles() {
    let config = initial_spawn_test_config();
    let mut sim = SimState::with_config(42, config);
    let mut events = Vec::new();
    sim.spawn_initial_creatures(&mut events);

    // Ground pile should exist. Position may be snapped to surface via
    // find_surface_position, so look up by the expected surface position.
    let surface_pos = sim.find_surface_position(32, 34);
    let pile = sim
        .db
        .ground_piles
        .by_position(&surface_pos, tabulosity::QueryOpts::ASC)
        .into_iter()
        .next()
        .expect("ground pile should exist");
    let bread_count = sim.inv_item_count(
        pile.inventory_id,
        crate::inventory::ItemKind::Bread,
        crate::inventory::MaterialFilter::Any,
    );
    assert_eq!(bread_count, 5);
}

// -----------------------------------------------------------------------
// F-flying-nav + F-giant-hornet tests
// -----------------------------------------------------------------------

#[test]
fn hornet_species_in_config() {
    let config = GameConfig::default();
    assert!(config.species.contains_key(&Species::Hornet));
    let data = &config.species[&Species::Hornet];
    assert_eq!(data.flight_ticks_per_voxel, Some(250));
    assert_eq!(data.footprint, [1, 1, 1]);
    assert!(data.melee_damage > 0);
}

#[test]
fn hornet_spawn_has_traits() {
    let mut sim = test_sim(42);
    // Spawn hornet in the air above the tree.
    let pos = VoxelCoord::new(32, 40, 32);
    let id = spawn_hornet_at(&mut sim, pos);

    // Hornet should have BodyColor, StripePattern, WingStyle traits.
    let body_color = sim.trait_int(id, TraitKind::BodyColor, -1);
    assert!(
        (0..4).contains(&body_color),
        "body_color {body_color} out of range"
    );
    let stripe = sim.trait_int(id, TraitKind::StripePattern, -1);
    assert!(
        (0..3).contains(&stripe),
        "stripe_pattern {stripe} out of range"
    );
    let wing = sim.trait_int(id, TraitKind::WingStyle, -1);
    assert!((0..3).contains(&wing), "wing_style {wing} out of range");
}

#[test]
fn hornet_spawns_at_air_position_not_nav_node() {
    let mut sim = test_sim(42);
    // Air position with no nav node nearby.
    let air_pos = VoxelCoord::new(32, 45, 32);
    assert!(sim.world.get(air_pos).is_flyable());

    // Spawn directly (not through step()) to avoid immediate activation/wander.
    let mut events = Vec::new();
    let id = sim
        .spawn_creature(Species::Hornet, air_pos, &mut events)
        .expect("hornet should spawn in air");

    let creature = sim.db.creatures.get(&id).unwrap();
    // Hornet should be at the exact air position, not snapped to a nav node.
    assert_eq!(creature.position, air_pos);
    // Should be in the air — verify there's no nav node here.
    assert!(sim.nav_graph.node_at(air_pos).is_none());
}

#[test]
fn hornet_is_hostile_to_elves() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);

    // Spawn the hornet close to the elf (within detection range of 14 voxels).
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let hornet_pos = VoxelCoord::new(elf_pos.x, elf_pos.y + 3, elf_pos.z);
    let mut events = Vec::new();
    let hornet_id = sim
        .spawn_creature(Species::Hornet, hornet_pos, &mut events)
        .expect("hornet should spawn near elf");

    // Hornet is aggressive and has no civ.
    let hornet = sim.db.creatures.get(&hornet_id).unwrap();
    assert!(hornet.civ_id.is_none());
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert!(elf.civ_id.is_some());

    // Verify the hornet detects the elf as hostile.
    let hornet_data = &sim.species_table[&Species::Hornet];
    let targets = sim.detect_hostile_targets(
        hornet_id,
        Species::Hornet,
        hornet.position,
        hornet.civ_id,
        hornet_data.hostile_detection_range_sq,
    );
    assert!(
        !targets.is_empty(),
        "hornet should detect the elf as hostile"
    );
    // The detected target should be the elf.
    assert_eq!(targets[0].0, elf_id);
}

#[test]
fn hornet_is_flyer() {
    let config = GameConfig::default();
    let data = &config.species[&Species::Hornet];
    assert!(data.flight_ticks_per_voxel.is_some());
    // Non-flyers should have None.
    assert!(
        config.species[&Species::Elf]
            .flight_ticks_per_voxel
            .is_none()
    );
    assert!(
        config.species[&Species::Goblin]
            .flight_ticks_per_voxel
            .is_none()
    );
}

#[test]
fn spawn_hornet_via_spawn_creature_command() {
    let mut sim = test_sim(42);
    let hornet_count_before = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Hornet)
        .count();

    // Spawn hornet in the air via the standard SpawnCreature command.
    let air_pos = VoxelCoord::new(32, 40, 32);
    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: String::new(),
        tick,
        action: SimAction::SpawnCreature {
            species: Species::Hornet,
            position: air_pos,
        },
    };
    sim.step(&[cmd], tick + 1);

    let hornet_count_after = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Hornet)
        .count();
    assert_eq!(hornet_count_after, hornet_count_before + 1);
}

#[test]
fn hornet_serde_roundtrip() {
    // Species::Hornet should serialize/deserialize correctly.
    let species = Species::Hornet;
    let json = serde_json::to_string(&species).unwrap();
    let restored: Species = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, species);
}

#[test]
fn voxel_type_is_flyable() {
    use crate::types::VoxelType;
    // Air is flyable.
    assert!(VoxelType::Air.is_flyable());
    // Leaf and Fruit are solid but flyable (sparse canopy doesn't block flight).
    assert!(VoxelType::Leaf.is_flyable());
    assert!(VoxelType::Fruit.is_flyable());
    // BuildingInterior is flyable.
    assert!(VoxelType::BuildingInterior.is_flyable());
    // Ladders are flyable.
    assert!(VoxelType::WoodLadder.is_flyable());
    assert!(VoxelType::RopeLadder.is_flyable());
    // Solid types are not flyable.
    assert!(!VoxelType::Trunk.is_flyable());
    assert!(!VoxelType::Branch.is_flyable());
    assert!(!VoxelType::GrownWall.is_flyable());
    assert!(!VoxelType::Dirt.is_flyable());
}

#[test]
fn hornet_spawn_in_solid_returns_none() {
    let mut sim = test_sim(42);
    // Find a trunk voxel (guaranteed to exist — the tree is there).
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    assert!(!sim.world.get(tree_pos).is_flyable());
    let mut events = Vec::new();
    assert!(
        sim.spawn_creature(Species::Hornet, tree_pos, &mut events)
            .is_none()
    );
}

#[test]
fn hornet_spawn_in_leaf_succeeds() {
    let mut sim = test_sim(42);
    let leaf_pos = VoxelCoord::new(32, 45, 32);
    sim.world.set(leaf_pos, crate::types::VoxelType::Leaf);
    let mut events = Vec::new();
    let id = sim
        .spawn_creature(Species::Hornet, leaf_pos, &mut events)
        .expect("hornet should spawn in leaf voxel");
    assert_eq!(sim.db.creatures.get(&id).unwrap().position, leaf_pos);
}
#[test]
fn flight_pathfinding_corner_cost_matches_scaled_distance() {
    use crate::nav::scaled_distance;
    // Corner diagonal: (1,1,1) should match NEIGHBOR_OFFSETS.
    let corner_cost = scaled_distance(1, 1, 1);
    assert_eq!(corner_cost, 1773);
    // Edge diagonal: (1,1,0) should match.
    let edge_cost = scaled_distance(1, 1, 0);
    assert_eq!(edge_cost, 1448);
    // Face-adjacent: (1,0,0) should match.
    let face_cost = scaled_distance(1, 0, 0);
    assert_eq!(face_cost, 1024);
}

// -----------------------------------------------------------------------
// B-hostile-detect-nav: elf vs flying hornet at various heights
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// F-flying-nav-big + F-wyvern tests
// -----------------------------------------------------------------------

#[test]
fn wyvern_species_in_config() {
    let config = GameConfig::default();
    assert!(config.species.contains_key(&Species::Wyvern));
    let data = &config.species[&Species::Wyvern];
    assert_eq!(data.flight_ticks_per_voxel, Some(200));
    assert_eq!(data.footprint, [2, 2, 2]);
    assert!(data.melee_damage > 0);
    assert_eq!(data.hp_max, 100); // uniform base; CON stat provides toughness
}

#[test]
fn wyvern_spawn_has_traits() {
    let mut sim = test_sim(42);
    let pos = VoxelCoord::new(20, 40, 20);
    let mut events = Vec::new();
    let id = sim
        .spawn_creature(Species::Wyvern, pos, &mut events)
        .expect("wyvern should spawn in open air");

    let body_color = sim.trait_int(id, TraitKind::BodyColor, -1);
    assert!((0..4).contains(&body_color));
    let scale = sim.trait_int(id, TraitKind::ScalePattern, -1);
    assert!((0..3).contains(&scale));
    let horn = sim.trait_int(id, TraitKind::HornStyle, -1);
    assert!((0..3).contains(&horn));
}

#[test]
fn wyvern_spawn_checks_full_footprint() {
    let mut sim = test_sim(42);
    // Place a trunk voxel at one corner of where the 2x2x2 footprint would be.
    let anchor = VoxelCoord::new(20, 40, 20);
    sim.world
        .set(VoxelCoord::new(21, 41, 21), crate::types::VoxelType::Trunk);
    let mut events = Vec::new();
    assert!(
        sim.spawn_creature(Species::Wyvern, anchor, &mut events)
            .is_none(),
        "wyvern should not spawn when one footprint voxel is solid"
    );
}

#[test]
fn wyvern_serde_roundtrip() {
    let species = Species::Wyvern;
    let json = serde_json::to_string(&species).unwrap();
    let restored: Species = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, species);
}

#[test]
fn wyvern_is_hostile_to_elves() {
    let mut sim = test_sim(42);
    let elf_id = spawn_creature(&mut sim, Species::Elf);
    let elf_pos = sim.db.creatures.get(&elf_id).unwrap().position;

    // Spawn wyvern above the elf (needs 2x2x2 clear space in the air).
    let wyvern_pos = VoxelCoord::new(elf_pos.x - 1, elf_pos.y + 5, elf_pos.z - 1);
    let mut events = Vec::new();
    let wyvern_id = sim
        .spawn_creature(Species::Wyvern, wyvern_pos, &mut events)
        .expect("wyvern should spawn");

    let wyvern = sim.db.creatures.get(&wyvern_id).unwrap();
    assert!(wyvern.civ_id.is_none());

    let wyvern_data = &sim.species_table[&Species::Wyvern];
    let targets = sim.detect_hostile_targets(
        wyvern_id,
        Species::Wyvern,
        wyvern.position,
        wyvern.civ_id,
        wyvern_data.hostile_detection_range_sq,
    );
    assert!(!targets.is_empty(), "wyvern should detect the elf");
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn spawn_capybara_command() {
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

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Capybara), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Capybara,
            ..
        }
    )));

    // Capybara should be at a ground-level node (y=1, air above terrain).
    let capybara = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap();
    assert_eq!(capybara.position.y, 1);
    assert!(sim.nav_graph.node_at(capybara.position).is_some());
}

#[test]
fn species_data_loaded_from_config() {
    let sim = test_sim(42);
    assert_eq!(sim.species_table.len(), 12);
    assert!(sim.species_table.contains_key(&Species::Elf));
    assert!(sim.species_table.contains_key(&Species::Capybara));
    assert!(sim.species_table.contains_key(&Species::Boar));
    assert!(sim.species_table.contains_key(&Species::Deer));
    assert!(sim.species_table.contains_key(&Species::Elephant));
    assert!(sim.species_table.contains_key(&Species::Wyvern));
    assert!(sim.species_table.contains_key(&Species::Hornet));
    assert!(sim.species_table.contains_key(&Species::Goblin));
    assert!(sim.species_table.contains_key(&Species::Monkey));
    assert!(sim.species_table.contains_key(&Species::Orc));
    assert!(sim.species_table.contains_key(&Species::Squirrel));
    assert!(sim.species_table.contains_key(&Species::Troll));

    let elf_data = &sim.species_table[&Species::Elf];
    assert!(!elf_data.ground_only);
    assert!(elf_data.allowed_edge_types.is_none());

    let capy_data = &sim.species_table[&Species::Capybara];
    assert!(capy_data.ground_only);
    assert!(capy_data.allowed_edge_types.is_some());

    let boar_data = &sim.species_table[&Species::Boar];
    assert!(boar_data.ground_only);
    assert_eq!(boar_data.walk_ticks_per_voxel, 500); // uniform base

    let deer_data = &sim.species_table[&Species::Deer];
    assert!(deer_data.ground_only);
    assert_eq!(deer_data.walk_ticks_per_voxel, 500); // uniform base

    let monkey_data = &sim.species_table[&Species::Monkey];
    assert!(!monkey_data.ground_only);
    assert_eq!(monkey_data.climb_ticks_per_voxel, Some(800));

    let squirrel_data = &sim.species_table[&Species::Squirrel];
    assert!(!squirrel_data.ground_only);
    assert_eq!(squirrel_data.climb_ticks_per_voxel, Some(600));

    // Troll has HP regeneration; most species default to 0.
    let troll_data = &sim.species_table[&Species::Troll];
    assert_eq!(troll_data.ticks_per_hp_regen, 500);
    assert_eq!(elf_data.ticks_per_hp_regen, 0);
}

#[test]
fn graph_for_species_dispatch() {
    let sim = test_sim(42);

    // Elf (1x1x1) → standard graph.
    let elf_graph = sim.graph_for_species(Species::Elf) as *const _;
    let standard = &sim.nav_graph as *const _;
    assert_eq!(elf_graph, standard, "Elf should use standard nav graph");

    // Elephant (2x2x2) → large graph.
    let elephant_graph = sim.graph_for_species(Species::Elephant) as *const _;
    let large = &sim.large_nav_graph as *const _;
    assert_eq!(elephant_graph, large, "Elephant should use large nav graph");
}

#[test]
fn new_sim_has_large_nav_graph() {
    let sim = test_sim(42);
    assert!(
        sim.large_nav_graph.live_nodes().count() > 0,
        "Large nav graph should have nodes after construction",
    );
}

#[test]
fn elephant_spawns_on_large_graph() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();
    let spawn_pos = VoxelCoord::new(10, 1, 10);
    sim.spawn_creature(Species::Elephant, spawn_pos, &mut events);

    // There should be exactly one elephant.
    let elephants: Vec<&crate::db::Creature> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Elephant)
        .collect();
    assert_eq!(elephants.len(), 1, "Should have spawned one elephant");

    // Its position should map to a node in the large nav graph.
    let elephant = elephants[0];
    let node_id = sim
        .large_nav_graph
        .node_at(elephant.position)
        .expect("Elephant should have a nav node in the large graph");
    let node = sim.large_nav_graph.node(node_id);
    assert_eq!(
        node.position, elephant.position,
        "Elephant position should match its large graph node",
    );
}

#[test]
fn troll_spawns_on_large_graph() {
    let mut sim = test_sim(42);
    let mut events = Vec::new();
    let spawn_pos = VoxelCoord::new(10, 1, 10);
    sim.spawn_creature(Species::Troll, spawn_pos, &mut events);

    let trolls: Vec<&crate::db::Creature> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Troll)
        .collect();
    assert_eq!(trolls.len(), 1, "Should have spawned one troll");

    let troll = trolls[0];
    let node_id = sim
        .large_nav_graph
        .node_at(troll.position)
        .expect("Troll should have a nav node in the large graph");
    let node = sim.large_nav_graph.node(node_id);
    assert_eq!(
        node.position, troll.position,
        "Troll position should match its large graph node",
    );
}

#[test]
fn creature_species_preserved() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn one elf and one capybara.
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
                species: Species::Capybara,
                position: tree_pos,
            },
        },
    ];
    sim.step(&cmds, 2);

    assert_eq!(sim.creature_count(Species::Elf), 1);
    assert_eq!(sim.creature_count(Species::Capybara), 1);
    assert_eq!(sim.db.creatures.len(), 2);

    // Verify species are correctly stored.
    let elf = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .unwrap();
    assert_eq!(elf.species, Species::Elf);

    let capy = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Capybara)
        .unwrap();
    assert_eq!(capy.species, Species::Capybara);
}

// -----------------------------------------------------------------------
// New species tests (Boar, Deer, Monkey, Squirrel)
// -----------------------------------------------------------------------

#[test]
fn spawn_boar_command() {
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

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Boar), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Boar,
            ..
        }
    )));

    // Boar is ground-only — should be at y=1.
    let boar = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Boar)
        .unwrap();
    assert_eq!(boar.position.y, 1);
}

#[test]
fn spawn_deer_command() {
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

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Deer), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Deer,
            ..
        }
    )));

    // Deer is ground-only — should be at y=1.
    let deer = sim
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Deer)
        .unwrap();
    assert_eq!(deer.position.y, 1);
}

#[test]
fn spawn_monkey_command() {
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

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Monkey), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Monkey,
            ..
        }
    )));
}

#[test]
fn spawn_squirrel_command() {
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

    let result = sim.step(&[cmd], 2);
    assert_eq!(sim.creature_count(Species::Squirrel), 1);
    assert!(result.events.iter().any(|e| matches!(
        e.kind,
        SimEventKind::CreatureArrived {
            species: Species::Squirrel,
            ..
        }
    )));
}

#[test]
fn all_small_species_spawn_and_coexist() {
    let mut sim = test_sim(300);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Only non-hostile species — hostile species (Goblin, Orc) would fight
    // and kill friendlies during the 50k-tick coexistence window, especially
    // with expanded melee ranges (range_sq=3 covers 3D diagonals).
    let species_list = [
        Species::Elf,
        Species::Capybara,
        Species::Boar,
        Species::Deer,
        Species::Monkey,
        Species::Squirrel,
    ];
    let mut tick = 1;
    for &species in &species_list {
        let cmd = SimCommand {
            player_name: String::new(),
            tick,
            action: SimAction::SpawnCreature {
                species,
                position: tree_pos,
            },
        };
        sim.step(&[cmd], tick + 1);
        tick = sim.tick + 1;
    }

    assert_eq!(sim.db.creatures.len(), 6);
    for &species in &species_list {
        assert_eq!(sim.creature_count(species), 1, "Expected 1 {:?}", species);
    }

    // Run for a while — all should remain alive with valid nodes.
    sim.step(&[], 50000);
    assert_eq!(sim.db.creatures.len(), 6);
    for creature in sim.db.creatures.iter_all() {
        assert!(
            sim.graph_for_species(creature.species)
                .node_at(creature.position)
                .is_some(),
            "{:?} has no nav node at its position",
            creature.species
        );
    }
}

#[test]
fn spawn_creature_with_civ_sets_civ_id() {
    let mut sim = test_sim(42);
    let hostile_civ = ensure_hostile_civ(&mut sim);
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let creature_id = sim
        .spawn_creature_with_civ(Species::Goblin, tree_pos, Some(hostile_civ), &mut events)
        .expect("should spawn goblin");

    let creature = sim.db.creatures.get(&creature_id).unwrap();
    assert_eq!(creature.civ_id, Some(hostile_civ));
    assert_eq!(creature.species, Species::Goblin);
}

#[test]
fn species_config_backward_compat_ticks_per_hp_regen() {
    // Old save files won't have ticks_per_hp_regen. Verify it defaults to 0.
    let config = GameConfig::default();
    let troll_data = &config.species[&Species::Troll];
    let json = serde_json::to_string(troll_data).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let obj = value.as_object_mut().unwrap();
    obj.remove("ticks_per_hp_regen");
    let stripped = serde_json::to_string(&value).unwrap();

    let restored: crate::species::SpeciesData = serde_json::from_str(&stripped).unwrap();
    assert_eq!(
        restored.ticks_per_hp_regen, 0,
        "ticks_per_hp_regen should default to 0"
    );
}

#[test]
fn all_civ_species_with_to_species_have_species_table_entry() {
    // Every CivSpecies that maps to a Species via to_species() must have
    // a corresponding entry in the default species table.
    let config = GameConfig::default();
    for civ_species in CivSpecies::ALL {
        if let Some(species) = civ_species.to_species() {
            assert!(
                config.species.contains_key(&species),
                "{civ_species:?} maps to {species:?} but species table has no entry"
            );
        }
    }
}

#[test]
fn spawn_creature_helper_returns_newly_spawned_id() {
    let mut sim = test_sim(42);
    // Spawn two capybaras and verify each call returns the newly spawned
    // one, not the first capybara in the table.
    let first = spawn_creature(&mut sim, Species::Capybara);
    let second = spawn_creature(&mut sim, Species::Capybara);
    assert_ne!(
        first, second,
        "spawn_creature should return distinct IDs for each spawn"
    );
    // Both creatures should exist.
    assert!(sim.db.creatures.get(&first).is_some());
    assert!(sim.db.creatures.get(&second).is_some());
}

#[test]
fn spawn_test_elf_helper_returns_newly_spawned_id() {
    let mut sim = test_sim(42);
    let first = spawn_test_elf(&mut sim);
    let second = spawn_test_elf(&mut sim);
    assert_ne!(
        first, second,
        "spawn_test_elf should return distinct IDs for each spawn"
    );
}

#[test]
fn spawn_test_elves_helper_returns_only_new_ids() {
    let mut sim = test_sim(42);
    let pre_existing = spawn_test_elf(&mut sim);
    let new_elves = spawn_test_elves(&mut sim, 3);
    assert_eq!(new_elves.len(), 3, "should return exactly 3 new elves");
    assert!(
        !new_elves.contains(&pre_existing),
        "should not include pre-existing elf"
    );
}

// -----------------------------------------------------------------------
// Creature sex (F-creature-sex)
// -----------------------------------------------------------------------

#[test]
fn spawned_creature_has_sex_field() {
    let mut sim = test_sim(42);
    let id = spawn_test_elf(&mut sim);
    let creature = sim.db.creatures.get(&id).unwrap();
    // With default weights [0,1,1], sex must be Male or Female (never None).
    assert!(
        creature.sex == CreatureSex::Male || creature.sex == CreatureSex::Female,
        "elf should have Male or Female sex, got {:?}",
        creature.sex,
    );
}

#[test]
fn spawned_creatures_have_both_sexes() {
    // Spawn enough creatures that both sexes must appear (structural property).
    let mut sim = test_sim(99);
    let ids = spawn_test_elves(&mut sim, 30);
    let mut has_male = false;
    let mut has_female = false;
    for id in &ids {
        let creature = sim.db.creatures.get(id).unwrap();
        match creature.sex {
            CreatureSex::Male => has_male = true,
            CreatureSex::Female => has_female = true,
            CreatureSex::None => panic!("elf should not have CreatureSex::None"),
        }
    }
    assert!(has_male, "expected at least one male in 30 elves");
    assert!(has_female, "expected at least one female in 30 elves");
}

#[test]
fn roll_creature_sex_respects_all_none_weights() {
    use crate::species::roll_creature_sex;
    let mut rng = elven_canopy_prng::GameRng::new(123);
    for _ in 0..20 {
        let sex = roll_creature_sex(&[1, 0, 0], &mut rng);
        assert_eq!(
            sex,
            CreatureSex::None,
            "weights [1,0,0] should always produce None"
        );
    }
}

#[test]
fn roll_creature_sex_respects_all_male_weights() {
    use crate::species::roll_creature_sex;
    let mut rng = elven_canopy_prng::GameRng::new(456);
    for _ in 0..20 {
        let sex = roll_creature_sex(&[0, 1, 0], &mut rng);
        assert_eq!(
            sex,
            CreatureSex::Male,
            "weights [0,1,0] should always produce Male"
        );
    }
}

#[test]
fn roll_creature_sex_respects_all_female_weights() {
    use crate::species::roll_creature_sex;
    let mut rng = elven_canopy_prng::GameRng::new(789);
    for _ in 0..20 {
        let sex = roll_creature_sex(&[0, 0, 1], &mut rng);
        assert_eq!(
            sex,
            CreatureSex::Female,
            "weights [0,0,1] should always produce Female"
        );
    }
}

#[test]
fn roll_creature_sex_equal_weights_produces_both() {
    use crate::species::roll_creature_sex;
    let mut rng = elven_canopy_prng::GameRng::new(42);
    let mut male_count = 0;
    let mut female_count = 0;
    for _ in 0..100 {
        match roll_creature_sex(&[0, 1, 1], &mut rng) {
            CreatureSex::Male => male_count += 1,
            CreatureSex::Female => female_count += 1,
            CreatureSex::None => panic!("weights [0,1,1] should never produce None"),
        }
    }
    assert!(male_count > 0, "expected at least one male in 100 rolls");
    assert!(
        female_count > 0,
        "expected at least one female in 100 rolls"
    );
}

#[test]
fn creature_sex_symbol() {
    assert_eq!(CreatureSex::None.symbol(), "");
    assert_eq!(CreatureSex::Male.symbol(), "♂");
    assert_eq!(CreatureSex::Female.symbol(), "♀");
}

#[test]
fn creature_sex_default_is_none() {
    assert_eq!(CreatureSex::default(), CreatureSex::None);
}

#[test]
fn creature_sex_serde_roundtrip() {
    for sex in &[CreatureSex::None, CreatureSex::Male, CreatureSex::Female] {
        let json = serde_json::to_string(sex).unwrap();
        let deserialized: CreatureSex = serde_json::from_str(&json).unwrap();
        assert_eq!(*sex, deserialized);
    }
}

#[test]
fn creature_sex_default_on_missing_field() {
    // Verify that a struct with #[serde(default)] on a CreatureSex field
    // deserializes to CreatureSex::None when the key is absent in JSON.
    // This is the actual old-save backward-compat guarantee.
    #[derive(serde::Deserialize)]
    struct FakeCreature {
        #[serde(default)]
        sex: CreatureSex,
    }
    let val: FakeCreature = serde_json::from_str(r#"{}"#).unwrap();
    assert_eq!(val.sex, CreatureSex::None);
}

#[test]
fn roll_creature_sex_zero_weight_excluded() {
    // Weights [0, 5, 1]: None has weight 0, should never appear.
    use crate::species::roll_creature_sex;
    let mut rng = elven_canopy_prng::GameRng::new(42);
    for _ in 0..100 {
        let sex = roll_creature_sex(&[0, 5, 1], &mut rng);
        assert_ne!(
            sex,
            CreatureSex::None,
            "weight-0 variant should never be produced"
        );
    }
    // Weights [1, 0, 5]: Male has weight 0, should never appear.
    for _ in 0..100 {
        let sex = roll_creature_sex(&[1, 0, 5], &mut rng);
        assert_ne!(
            sex,
            CreatureSex::Male,
            "weight-0 variant should never be produced"
        );
    }
}

#[test]
#[should_panic(expected = "sex_weights sum must be >= 1")]
fn roll_creature_sex_panics_on_zero_sum() {
    use crate::species::roll_creature_sex;
    let mut rng = elven_canopy_prng::GameRng::new(42);
    roll_creature_sex(&[0, 0, 0], &mut rng);
}

#[test]
fn sex_weights_in_default_config() {
    // All species in the default config should have valid sex_weights (sum >= 1).
    let config = GameConfig::default();
    for (species, data) in &config.species {
        let sum: u32 = data.sex_weights.iter().sum();
        assert!(
            sum >= 1,
            "species {:?} has sex_weights sum {} (must be >= 1)",
            species,
            sum,
        );
    }
}
