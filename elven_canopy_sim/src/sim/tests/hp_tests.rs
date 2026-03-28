//! Tests for the HP/vital status system: damage, incapacitation, bleed-out,
//! death, healing, HP regeneration, death drops, and troll regen recovery.

use super::test_helpers::*;
use super::*;

// -----------------------------------------------------------------------
// HP / damage / heal / death tests
// -----------------------------------------------------------------------

#[test]
fn spawn_sets_hp_from_species_data() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    let hp_max = sim.species_table[&Species::Elf].hp_max;
    assert_eq!(creature.hp, hp_max);
    assert_eq!(creature.hp_max, hp_max);
    assert_eq!(creature.vital_status, VitalStatus::Alive);
}

#[test]
fn debug_kill_sets_dead_and_emits_event() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    let result = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Creature should still exist but be dead.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.vital_status, VitalStatus::Dead);
    assert_eq!(creature.hp, 0);

    // Should have emitted CreatureDied event.
    assert!(result.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureDied {
            creature_id: cid,
            cause: DeathCause::Debug,
            ..
        } if *cid == elf_id
    )));
}

#[test]
fn dead_creature_excluded_from_count() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    assert_eq!(sim.creature_count(Species::Elf), 1);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    assert_eq!(sim.creature_count(Species::Elf), 0);
    // But the row still exists in the DB.
    assert!(sim.db.creatures.get(&elf_id).is_some());
}

#[test]
fn damage_reduces_hp() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let hp_max = sim.species_table[&Species::Elf].hp_max;
    let tick = sim.tick;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 30,
            },
        }],
        tick + 1,
    );

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.hp, hp_max - 30);
    assert_eq!(creature.vital_status, VitalStatus::Alive);
}

#[test]
fn damage_incapacitates_at_zero_hp() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let creature_hp = sim.db.creatures.get(&elf_id).unwrap().hp;
    let tick = sim.tick;

    let result = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: creature_hp, // exactly reduces HP to 0 → incapacitated
            },
        }],
        tick + 1,
    );

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.vital_status, VitalStatus::Incapacitated);
    assert_eq!(creature.hp, 0);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(&e.kind, SimEventKind::CreatureIncapacitated { .. }))
    );
}

#[test]
fn overkill_damage_kills_outright() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 99999,
            },
        }],
        tick + 1,
    );

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    // Massive damage kills outright (hp <= -hp_max), death handler sets hp = 0.
    assert_eq!(creature.hp, 0);
    assert_eq!(creature.vital_status, VitalStatus::Dead);
}

#[test]
fn heal_restores_hp_clamped_to_max() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let hp_max = sim.species_table[&Species::Elf].hp_max;
    let tick = sim.tick;

    // Damage first.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 60,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_max - 60);

    // Heal more than needed.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::HealCreature {
                creature_id: elf_id,
                amount: 999,
            },
        }],
        tick2 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_max);
}

#[test]
fn heal_does_not_revive_dead() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Try to heal.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::HealCreature {
                creature_id: elf_id,
                amount: 100,
            },
        }],
        tick2 + 1,
    );

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.vital_status, VitalStatus::Dead);
    assert_eq!(creature.hp, 0);
}

// ---------------------------------------------------------------------------
// HP regeneration
// ---------------------------------------------------------------------------

#[test]
fn hp_regen_restores_hp_over_heartbeats() {
    let mut sim = test_sim(42);
    // ticks_per_hp_regen=100 means 1 HP per 100 ticks. With heartbeat=5000,
    // each heartbeat regens 5000/100 = 50 HP. Two heartbeats = 100 HP.
    let troll_data = sim.species_table.get_mut(&Species::Troll).unwrap();
    troll_data.ticks_per_hp_regen = 100;
    let ticks_per_regen = troll_data.ticks_per_hp_regen;
    let heartbeat = troll_data.heartbeat_interval_ticks;

    let troll_id = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;

    // Damage more than 2 heartbeats can heal (100 HP).
    let damage = 200;
    let regen_per_heartbeat = heartbeat as i64 / ticks_per_regen as i64;
    assert!(damage > regen_per_heartbeat * 2);
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: damage,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.creatures.get(&troll_id).unwrap().hp, hp_max - damage);

    // Advance through 2 heartbeats — should partially regen.
    let target_tick = sim.tick + heartbeat * 2 + 1;
    sim.step(&[], target_tick);

    let expected_regen = regen_per_heartbeat * 2;
    let creature = sim.db.creatures.get(&troll_id).unwrap();
    assert_eq!(creature.hp, hp_max - damage + expected_regen);
    // Verify still below max (regen didn't overshoot).
    assert!(creature.hp < hp_max);
}

#[test]
fn hp_regen_clamps_to_max() {
    let mut sim = test_sim(42);
    // Very fast regen (1 HP per tick) so it overshoots easily.
    sim.species_table
        .get_mut(&Species::Troll)
        .unwrap()
        .ticks_per_hp_regen = 1;
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;
    let heartbeat = sim.species_table[&Species::Troll].heartbeat_interval_ticks;

    // Small damage.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: 10,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.creatures.get(&troll_id).unwrap().hp, hp_max - 10);

    // Advance past a heartbeat — regen should clamp to hp_max.
    sim.step(&[], sim.tick + heartbeat + 1);
    assert_eq!(sim.db.creatures.get(&troll_id).unwrap().hp, hp_max);
}

#[test]
fn hp_regen_does_not_revive_dead() {
    let mut sim = test_sim(42);
    sim.species_table
        .get_mut(&Species::Troll)
        .unwrap()
        .ticks_per_hp_regen = 1;
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    // Use creature's actual hp_max (may differ from species due to Constitution).
    let actual_hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;
    let heartbeat = sim.species_table[&Species::Troll].heartbeat_interval_ticks;

    // Kill the troll outright (damage past -hp_max to bypass incapacitation).
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: actual_hp_max * 2,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&troll_id).unwrap().vital_status,
        VitalStatus::Dead
    );

    // Advance past heartbeats — should stay dead.
    sim.step(&[], sim.tick + heartbeat * 3 + 1);
    let creature = sim.db.creatures.get(&troll_id).unwrap();
    assert_eq!(creature.vital_status, VitalStatus::Dead);
    assert_eq!(creature.hp, 0);
}

#[test]
fn zero_hp_regen_does_not_heal() {
    let mut sim = test_sim(42);
    // Default regen is 0 (disabled).
    assert_eq!(sim.species_table[&Species::Elf].ticks_per_hp_regen, 0);
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let hp_max = sim.species_table[&Species::Elf].hp_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    // Damage.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 50,
            },
        }],
        tick + 1,
    );

    // Advance past heartbeats — HP should not change.
    sim.step(&[], sim.tick + heartbeat * 2 + 1);
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_max - 50);
}

#[test]
fn hp_regen_at_full_hp_is_noop() {
    let mut sim = test_sim(42);
    sim.species_table
        .get_mut(&Species::Troll)
        .unwrap()
        .ticks_per_hp_regen = 100;
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;
    let heartbeat = sim.species_table[&Species::Troll].heartbeat_interval_ticks;

    // No damage — already at full HP.
    sim.step(&[], sim.tick + heartbeat + 1);
    assert_eq!(sim.db.creatures.get(&troll_id).unwrap().hp, hp_max);
}

#[test]
fn hp_regen_integer_division_truncates() {
    let mut sim = test_sim(42);
    // 5000 / 3000 = 1 (truncated, not rounded to 2).
    let troll_data = sim.species_table.get_mut(&Species::Troll).unwrap();
    troll_data.ticks_per_hp_regen = 3000;
    let heartbeat = troll_data.heartbeat_interval_ticks;

    let troll_id = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;

    // Damage and advance 1 heartbeat.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: 100,
            },
        }],
        tick + 1,
    );
    sim.step(&[], sim.tick + heartbeat + 1);
    assert_eq!(
        sim.db.creatures.get(&troll_id).unwrap().hp,
        hp_max - 100 + 1
    );
}

#[test]
fn hp_regen_slower_than_heartbeat_yields_zero() {
    let mut sim = test_sim(42);
    // ticks_per_hp_regen > heartbeat → 5000 / 10000 = 0.
    let troll_data = sim.species_table.get_mut(&Species::Troll).unwrap();
    troll_data.ticks_per_hp_regen = 10000;
    let heartbeat = troll_data.heartbeat_interval_ticks;

    let troll_id = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: 50,
            },
        }],
        tick + 1,
    );

    // Multiple heartbeats — still no regen.
    sim.step(&[], sim.tick + heartbeat * 3 + 1);
    assert_eq!(sim.db.creatures.get(&troll_id).unwrap().hp, hp_max - 50);
}

#[test]
fn hp_regen_concurrent_with_combat_damage() {
    let mut sim = test_sim(42);
    let troll_data = sim.species_table.get_mut(&Species::Troll).unwrap();
    troll_data.ticks_per_hp_regen = 100;
    let heartbeat = troll_data.heartbeat_interval_ticks;
    let regen_per_heartbeat = heartbeat as i64 / 100;

    let troll_id = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;

    // Initial damage.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: 200,
            },
        }],
        tick + 1,
    );
    let hp_after_damage = hp_max - 200;

    // Advance past 1 heartbeat — regens.
    sim.step(&[], sim.tick + heartbeat + 1);
    let hp_after_regen1 = hp_after_damage + regen_per_heartbeat;
    assert_eq!(sim.db.creatures.get(&troll_id).unwrap().hp, hp_after_regen1);

    // Deal more damage between heartbeats.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: 30,
            },
        }],
        tick2 + 1,
    );
    let hp_after_damage2 = hp_after_regen1 - 30;

    // Advance past another heartbeat — regens again from new HP.
    sim.step(&[], sim.tick + heartbeat + 1);
    assert_eq!(
        sim.db.creatures.get(&troll_id).unwrap().hp,
        hp_after_damage2 + regen_per_heartbeat
    );
}

#[test]
fn hp_regen_huge_ticks_per_hp_regen_no_negative_heal() {
    // Regression: u64 values > i64::MAX must not wrap to negative in the
    // division, which would turn regen into damage.
    let mut sim = test_sim(42);
    sim.species_table
        .get_mut(&Species::Troll)
        .unwrap()
        .ticks_per_hp_regen = u64::MAX;
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;
    let heartbeat = sim.species_table[&Species::Troll].heartbeat_interval_ticks;

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: 50,
            },
        }],
        tick + 1,
    );
    let hp_before = sim.db.creatures.get(&troll_id).unwrap().hp;
    assert_eq!(hp_before, hp_max - 50);

    // Advance past heartbeats — HP must not decrease.
    sim.step(&[], sim.tick + heartbeat * 2 + 1);
    let hp_after = sim.db.creatures.get(&troll_id).unwrap().hp;
    assert!(
        hp_after >= hp_before,
        "regen must never reduce HP: {hp_after} < {hp_before}"
    );
}

#[test]
fn death_drops_inventory_as_ground_pile() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Give the elf some items.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::AddCreatureItem {
                creature_id: elf_id,
                item_kind: inventory::ItemKind::Bread,
                quantity: 5,
            },
        }],
        tick + 1,
    );

    let creature_pos = sim.db.creatures.get(&elf_id).unwrap().position;

    // Kill the elf.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick2 + 1,
    );

    // Creature's inventory should be empty (items transferred to ground pile).
    let inv_id = sim.db.creatures.get(&elf_id).unwrap().inventory_id;
    let remaining: Vec<_> = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC);
    assert!(remaining.is_empty(), "dead creature should have no items");

    // A ground pile should exist somewhere with the dropped bread.
    // (Position may be snapped by ensure_ground_pile, so we don't check exact pos.)
    let _ = creature_pos; // used only to confirm creature existed
    let total_bread: u32 = sim
        .db
        .ground_piles
        .iter_all()
        .flat_map(|p| {
            sim.db
                .item_stacks
                .by_inventory_id(&p.inventory_id, tabulosity::QueryOpts::ASC)
        })
        .filter(|s| s.kind == inventory::ItemKind::Bread)
        .map(|s| s.quantity)
        .sum();
    assert!(
        total_bread >= 5,
        "ground piles should have >= 5 bread, got {total_bread}"
    );
}

// ---------------------------------------------------------------------------
// Incapacitation (F-incapacitation)
// ---------------------------------------------------------------------------

#[test]
fn damage_to_zero_hp_incapacitates_not_kills() {
    // Test that damage bringing HP to exactly 0 results in Incapacitated, not Dead.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.hp = goblin_damage;
    });

    let tick = sim.tick;
    let events = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: goblin,
                target_id: elf,
            },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Incapacitated);
    assert_eq!(elf_c.hp, 0);

    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureIncapacitated { creature_id, .. } if *creature_id == elf
    )));
    assert!(!events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureDied { creature_id, .. } if *creature_id == elf
    )));
}

#[test]
fn incapacitated_creature_bleeds_out_to_death() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let hp_max = sim.db.creatures.get(&elf).unwrap().hp_max;
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = 0;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let start_tick = sim.tick;
    let target_tick = start_tick + heartbeat * (hp_max as u64) + 1;
    sim.step(&[], target_tick);

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Dead);
}

#[test]
fn further_damage_on_incapacitated_pushes_hp_negative() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    let hp_max = sim.db.creatures.get(&elf).unwrap().hp_max;
    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = 0;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: goblin,
                target_id: elf,
            },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.hp, -goblin_damage);
    assert!(
        elf_c.hp > -hp_max,
        "elf should still be incapacitated, not dead yet"
    );
    assert_eq!(elf_c.vital_status, VitalStatus::Incapacitated);
}

#[test]
fn massive_single_hit_kills_outright() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let hp_max = sim.db.creatures.get(&elf).unwrap().hp_max;

    let tick = sim.tick;
    let events = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf,
                amount: hp_max + hp_max,
            },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Dead);
    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureDied { creature_id, .. } if *creature_id == elf
    )));
}

#[test]
fn heal_revives_incapacitated_creature() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -10;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::HealCreature {
                creature_id: elf,
                amount: 20,
            },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Alive);
    assert_eq!(elf_c.hp, 10);
}

#[test]
fn heal_incapacitated_not_enough_stays_incapacitated() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -30;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::HealCreature {
                creature_id: elf,
                amount: 20,
            },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Incapacitated);
    assert_eq!(elf_c.hp, -10);
}

#[test]
fn incapacitated_creature_does_not_seek_food() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let food_threshold = sim.species_table[&Species::Elf].food_max
        * sim.species_table[&Species::Elf].food_hunger_threshold_pct
        / 100;
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -5;
        c.vital_status = VitalStatus::Incapacitated;
        c.food = food_threshold / 2;
        c.current_task = None;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let start_tick = sim.tick;
    sim.step(&[], start_tick + heartbeat + 1);

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert!(elf_c.current_task.is_none());
}

#[test]
fn debug_kill_bypasses_incapacitation() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let tick = sim.tick;
    let events = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature { creature_id: elf },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Dead);
    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureDied { creature_id, .. } if *creature_id == elf
    )));
}

#[test]
fn incapacitated_creature_is_targetable_by_melee() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = 0;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let tick = sim.tick;
    let events = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: goblin,
                target_id: elf,
            },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.hp, -goblin_damage);
    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureDamaged { target_id, .. } if *target_id == elf
    )));
}

#[test]
fn vital_status_incapacitated_serde_roundtrip() {
    let status = VitalStatus::Incapacitated;
    let json = serde_json::to_string(&status).unwrap();
    let restored: VitalStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(status, restored);
}

#[test]
fn incapacitation_notification_emitted() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.hp = 1;
    });

    let notif_count_before = sim.db.notifications.iter_all().count();

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf,
                amount: 1,
            },
        }],
        tick + 1,
    );

    let notif_count_after = sim.db.notifications.iter_all().count();
    assert!(
        notif_count_after > notif_count_before,
        "Should have emitted an incapacitation notification"
    );
    let last_notif = sim.db.notifications.iter_all().last().unwrap();
    assert!(
        last_notif.message.contains("incapacitated"),
        "Notification should mention incapacitation: {}",
        last_notif.message
    );
}

#[test]
fn heal_to_exactly_zero_stays_incapacitated() {
    // Healing an incapacitated creature to exactly 0 HP should leave it
    // incapacitated (not revive). Revival requires HP > 0.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -20;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::HealCreature {
                creature_id: elf,
                amount: 20,
            },
        }],
        tick + 1,
    );

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Incapacitated);
    assert_eq!(elf_c.hp, 0);
}

#[test]
fn incapacitated_creature_not_assigned_available_tasks() {
    // An incapacitated creature should NOT be assigned any tasks, even if
    // tasks are available and the creature is nominally idle.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -10;
        c.vital_status = VitalStatus::Incapacitated;
        c.current_task = None;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Create an available task.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let task_id = TaskId::new(&mut sim.rng);
    let go_task = task::Task {
        id: task_id,
        kind: task::TaskKind::GoTo,
        state: task::TaskState::Available,
        location: elf_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: task::TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(go_task);

    // Run a few heartbeats — the task should remain unassigned.
    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let tick = sim.tick;
    sim.step(&[], tick + heartbeat * 3);

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_c.current_task.is_none(),
        "Incapacitated creature should not be assigned tasks"
    );
}

#[test]
fn starvation_still_kills_directly() {
    // Starvation bypasses incapacitation — creature dies immediately when
    // food reaches 0 (no incapacitation step).
    let mut config = test_config();
    config
        .species
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 1_000_000_000_000_000;
    let mut sim = SimState::with_config(42, config);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

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

    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let target_tick = 1 + heartbeat_interval * 2 + 1;
    sim.step(&[], target_tick);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    // Should be Dead, NOT Incapacitated.
    assert_eq!(elf.vital_status, VitalStatus::Dead);
}

#[test]
fn bleed_out_precise_hp_per_heartbeat() {
    // Verify bleed-out loses exactly 1 HP per heartbeat, not more or less.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = 0;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Advance past exactly 3 heartbeats.
    let start_tick = sim.tick;
    sim.step(&[], start_tick + heartbeat * 3 + 1);

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.hp, -3, "Should lose exactly 1 HP per heartbeat");
    assert_eq!(elf_c.vital_status, VitalStatus::Incapacitated);
}

#[test]
fn save_load_roundtrip_preserves_incapacitation() {
    // A creature that is incapacitated mid-game should survive a
    // save->load roundtrip with correct vital_status and HP.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -42;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Roundtrip through JSON (from_json rebuilds transient state).
    let json = serde_json::to_string(&sim).unwrap();
    let restored = SimState::from_json(&json).unwrap();

    let elf_r = restored.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_r.vital_status, VitalStatus::Incapacitated);
    assert_eq!(elf_r.hp, -42);
}

#[test]
fn healed_incapacitated_creature_resumes_activation() {
    // A revived creature must have its activation chain restarted so it can
    // act again. Without this, the creature would be permanently frozen.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    // Incapacitate.
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -10;
        c.vital_status = VitalStatus::Incapacitated;
        c.current_task = None;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Heal to revive.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::HealCreature {
                creature_id: elf,
                amount: 50,
            },
        }],
        tick + 1,
    );

    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().vital_status,
        VitalStatus::Alive
    );

    // Advance several activation intervals — the creature should eventually
    // take an action (wander, etc.), proving the activation chain restarted.
    sim.step(&[], tick + 5000);

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    // The creature should have moved or taken some action (not frozen at NoAction).
    // A wander or any action_kind != NoAction, or a position change, proves it.
    let has_acted = elf_c.action_kind != ActionKind::NoAction || elf_c.current_task.is_some();
    assert!(
        has_acted,
        "Revived creature should resume acting. Action: {:?}, task: {:?}",
        elf_c.action_kind, elf_c.current_task
    );
}

#[test]
fn incapacitated_creature_in_spatial_index_after_save_load() {
    // After save/load, incapacitated creatures must remain in the spatial
    // index so they can be targeted by hostiles and hit by projectiles.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;

    // Incapacitate.
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -10;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Verify creature is in spatial index before save.
    assert!(
        sim.creatures_at_voxel(elf_pos).contains(&elf),
        "Incapacitated creature should be in spatial index before save"
    );

    // Save/load roundtrip (from_json calls rebuild_transient_state which
    // rebuilds the spatial index).
    let json = serde_json::to_string(&sim).unwrap();
    let restored = SimState::from_json(&json).unwrap();

    // Verify creature is still in spatial index after load.
    assert!(
        restored.creatures_at_voxel(elf_pos).contains(&elf),
        "Incapacitated creature should be in spatial index after save/load"
    );
}

#[test]
fn incapacitated_troll_recovers_via_regen() {
    // A troll with HP regen should recover from incapacitation: regen (6 HP)
    // outpaces bleed-out (1 HP), so HP climbs back above 0 and the troll
    // revives to Alive.
    let mut sim = test_sim(42);
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    zero_creature_stats(&mut sim, troll_id);

    let heartbeat = sim.species_table[&Species::Troll].heartbeat_interval_ticks;
    let ticks_per_hp_regen = sim.species_table[&Species::Troll].ticks_per_hp_regen;
    assert!(ticks_per_hp_regen > 0, "Troll should have HP regen");
    let regen_per_hb = (heartbeat / ticks_per_hp_regen) as i64;
    assert!(regen_per_hb > 1, "Regen should outpace 1 HP bleed-out");

    // Incapacitate at HP = -5 (well above -hp_max death threshold).
    if let Some(mut c) = sim.db.creatures.get(&troll_id) {
        c.hp = -5;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Advance enough heartbeats for regen to bring HP above 0.
    // Net gain per heartbeat = regen - 1. Need ceil(6 / (regen-1)) heartbeats.
    let net_gain = regen_per_hb - 1;
    let heartbeats_needed = (5 + net_gain - 1) / net_gain + 1; // extra margin
    let tick = sim.tick;
    sim.step(&[], tick + heartbeat * heartbeats_needed as u64 + 1);

    let troll = sim.db.creatures.get(&troll_id).unwrap();
    assert_eq!(
        troll.vital_status,
        VitalStatus::Alive,
        "Troll should recover from incapacitation via HP regen. HP: {}",
        troll.hp
    );
    assert!(troll.hp > 0);
}

#[test]
fn incapacitated_troll_dies_if_finished_off() {
    // Even though trolls regen, sustained damage can push HP past -hp_max
    // and kill them during incapacitation.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, troll_id);

    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;

    // Incapacitate troll.
    if let Some(mut c) = sim.db.creatures.get(&troll_id) {
        c.hp = 0;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Deal massive damage to push past -hp_max.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: troll_id,
                amount: hp_max + 1,
            },
        }],
        tick + 1,
    );

    let troll = sim.db.creatures.get(&troll_id).unwrap();
    assert_eq!(troll.vital_status, VitalStatus::Dead);
}

#[test]
fn non_regen_creature_does_not_recover() {
    // An elf (no HP regen) should NOT recover from incapacitation —
    // it just bleeds out. Sanity check against the troll recovery test.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    assert_eq!(
        sim.species_table[&Species::Elf].ticks_per_hp_regen,
        0,
        "Elves should not have HP regen"
    );

    // Incapacitate at HP = -5.
    if let Some(mut c) = sim.db.creatures.get(&elf) {
        c.hp = -5;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let heartbeat = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    let tick = sim.tick;
    sim.step(&[], tick + heartbeat * 3 + 1);

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_c.vital_status, VitalStatus::Incapacitated);
    // HP should have decreased (bleed-out only, no regen).
    assert_eq!(elf_c.hp, -8, "Should lose 1 HP per heartbeat with no regen");
}

#[test]
fn troll_regen_recovery_clamped_to_hp_max() {
    // When a troll recovers from incapacitation via regen, HP must not
    // exceed hp_max. With very fast regen the raw calculation could overshoot.
    let mut sim = test_sim(42);
    // Set regen extremely fast: 1 tick per HP → regen = interval HP/heartbeat.
    sim.species_table
        .get_mut(&Species::Troll)
        .unwrap()
        .ticks_per_hp_regen = 1;
    let troll_id = spawn_creature(&mut sim, Species::Troll);
    zero_creature_stats(&mut sim, troll_id);

    let hp_max = sim.db.creatures.get(&troll_id).unwrap().hp_max;
    let heartbeat = sim.species_table[&Species::Troll].heartbeat_interval_ticks;
    // With ticks_per_hp_regen=1, regen per heartbeat = interval (e.g., 3000).
    let regen = (heartbeat / 1) as i64;
    assert!(
        regen > hp_max,
        "Regen ({regen}) should exceed hp_max ({hp_max}) for this test"
    );

    // Incapacitate at HP = -1 (just below 0).
    if let Some(mut c) = sim.db.creatures.get(&troll_id) {
        c.hp = -1;
        c.vital_status = VitalStatus::Incapacitated;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // One heartbeat should revive with HP clamped to hp_max.
    let tick = sim.tick;
    sim.step(&[], tick + heartbeat + 1);

    let troll = sim.db.creatures.get(&troll_id).unwrap();
    assert_eq!(troll.vital_status, VitalStatus::Alive);
    assert_eq!(
        troll.hp, hp_max,
        "HP should be clamped to hp_max, not {}",
        troll.hp
    );
}

#[test]
fn attack_target_continues_through_incapacitation_to_death() {
    // An AttackTarget task should NOT be abandoned when the target becomes
    // incapacitated — the attacker keeps swinging until the target is Dead.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Disable food/rest so the goblin doesn't wander off to eat.
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .rest_decay_per_tick = 0;

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // Set elf HP low enough that melee will incapacitate quickly but not
    // kill outright (needs multiple hits to push past -hp_max).
    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    let hp_max = sim.db.creatures.get(&elf).unwrap().hp_max;
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.hp = goblin_damage * 2; // two hits to incapacitate
    });

    // Issue AttackCreature command (creates AttackTarget task).
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::AttackCreature {
                attacker_id: goblin,
                target_id: elf,
                queue: false,
            },
        }],
        tick + 1,
    );

    // Run long enough for multiple melee strikes to land, pushing through
    // incapacitation to death. Melee interval is ~1000 ticks; we need
    // enough hits to go from 0 to -hp_max (hp_max / goblin_damage strikes).
    let strikes_to_kill = (hp_max / goblin_damage) + 5; // margin
    let melee_interval = sim.species_table[&Species::Goblin].melee_interval_ticks;
    let run_ticks = melee_interval * (strikes_to_kill as u64 + 5);
    sim.step(&[], tick + 1 + run_ticks);

    let elf_c = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(
        elf_c.vital_status,
        VitalStatus::Dead,
        "Goblin should have finished off the incapacitated elf. Elf HP: {}, status: {:?}",
        elf_c.hp,
        elf_c.vital_status
    );
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

#[test]
fn dead_creature_heartbeat_does_not_reschedule() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill the elf.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Run sim forward past several heartbeat intervals. Any pending
    // heartbeat events for the dead elf should be no-ops (not reschedule).
    let heartbeat_interval = sim.species_table[&Species::Elf].heartbeat_interval_ticks;
    sim.step(&[], sim.tick + heartbeat_interval * 5);

    // Drain the event queue and check no heartbeat for this creature.
    let mut found_heartbeat = false;
    while let Some(evt) = sim.event_queue.pop_if_ready(u64::MAX) {
        if matches!(
            evt.kind,
            ScheduledEventKind::CreatureHeartbeat { creature_id } if creature_id == elf_id
        ) {
            found_heartbeat = true;
        }
    }
    assert!(
        !found_heartbeat,
        "dead creature should not have pending heartbeats"
    );
}

#[test]
fn dead_creature_not_assigned_tasks() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill the elf.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Create a GoTo task.
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::CreateTask {
                kind: TaskKind::GoTo,
                position: pos,
                required_species: Some(Species::Elf),
            },
        }],
        tick2 + 1,
    );

    // Run several activations.
    sim.step(&[], sim.tick + 10000);

    // Dead creature should NOT have picked up the task.
    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "dead creature should not claim tasks"
    );
}

#[test]
fn damage_dead_creature_is_noop() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Try to damage again.
    let tick2 = sim.tick;
    let result = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 50,
            },
        }],
        tick2 + 1,
    );

    // Should not emit a second death event.
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(&e.kind, SimEventKind::CreatureDied { .. })),
        "damaging dead creature should not emit another death event"
    );
}

#[test]
fn death_creates_notification() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let initial_notifications = sim.db.notifications.len();
    let tick = sim.tick;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    assert!(
        sim.db.notifications.len() > initial_notifications,
        "death should create a notification"
    );
    let last_notif = sim.db.notifications.iter_all().last().unwrap();
    assert!(
        last_notif.message.contains("died"),
        "notification should mention death: {}",
        last_notif.message
    );
}

#[test]
fn death_interrupts_current_task() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);

    // Create and claim a GoTo task.
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::CreateTask {
                kind: TaskKind::GoTo,
                position: pos,
                required_species: Some(Species::Elf),
            },
        }],
        tick + 1,
    );

    // Run until the elf picks up the task.
    sim.step(&[], sim.tick + 5000);
    // Elf should have a task now (either the GoTo or something from heartbeat).

    // Kill the elf.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick2 + 1,
    );

    let creature = sim.db.creatures.get(&elf_id).unwrap();
    assert!(
        creature.current_task.is_none(),
        "dead creature should have no task"
    );
    assert_eq!(creature.action_kind, ActionKind::NoAction);
}

#[test]
fn kill_nonexistent_creature_is_noop() {
    let mut sim = test_sim(42);
    let mut rng = GameRng::new(999);
    let fake_id = CreatureId::new(&mut rng);
    let tick = sim.tick;

    // Should not panic.
    let result = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: fake_id,
            },
        }],
        tick + 1,
    );

    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(&e.kind, SimEventKind::CreatureDied { .. })),
        "killing nonexistent creature should not emit event"
    );
}

#[test]
fn death_removes_from_spatial_index() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;

    // Elf should be in the spatial index before death.
    assert!(
        sim.spatial_index
            .get(&pos)
            .is_some_and(|v| v.contains(&elf_id)),
        "living elf should be in spatial index"
    );

    // Kill the elf.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Elf should no longer be in the spatial index.
    assert!(
        !sim.spatial_index
            .get(&pos)
            .is_some_and(|v| v.contains(&elf_id)),
        "dead elf should be removed from spatial index"
    );
}

#[test]
fn hp_death_serde_roundtrip() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf_id);
    let tick = sim.tick;

    // Damage elf to half HP.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 50,
            },
        }],
        tick + 1,
    );

    // Serialize and deserialize the DB.
    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: SimDb = serde_json::from_str(&json).unwrap();
    let creature = restored.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.hp, sim.db.creatures.get(&elf_id).unwrap().hp);
    assert_eq!(creature.hp_max, sim.species_table[&Species::Elf].hp_max);
    assert_eq!(creature.vital_status, VitalStatus::Alive);
}

#[test]
fn hp_death_serde_roundtrip_dead() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Kill elf.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: elf_id,
            },
        }],
        tick + 1,
    );

    // Serialize and deserialize.
    let json = serde_json::to_string(&sim.db).unwrap();
    let restored: SimDb = serde_json::from_str(&json).unwrap();
    let creature = restored.creatures.get(&elf_id).unwrap();
    assert_eq!(creature.vital_status, VitalStatus::Dead);
    assert_eq!(creature.hp, 0);
}

#[test]
fn zero_and_negative_damage_is_noop() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;
    let tick = sim.tick;

    // Zero damage — should not change HP.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 0,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_before);

    // Negative damage — should not change HP.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: -5,
            },
        }],
        tick2 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_before);
}

#[test]
fn zero_and_negative_heal_is_noop() {
    let mut sim = test_sim(42);
    let elf_id = spawn_elf(&mut sim);
    let tick = sim.tick;

    // Damage first so there's room to heal.
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DamageCreature {
                creature_id: elf_id,
                amount: 30,
            },
        }],
        tick + 1,
    );
    let hp_after_damage = sim.db.creatures.get(&elf_id).unwrap().hp;

    // Zero heal — should not change HP.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::HealCreature {
                creature_id: elf_id,
                amount: 0,
            },
        }],
        tick2 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_after_damage);

    // Negative heal — should not change HP.
    let tick3 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick3 + 1,
            action: SimAction::HealCreature {
                creature_id: elf_id,
                amount: -10,
            },
        }],
        tick3 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf_id).unwrap().hp, hp_after_damage);
}
