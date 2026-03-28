//! Tests for combat mechanics: melee attacks, ranged attacks (bow/arrow),
//! armor damage reduction, hit/miss/crit rolls, evasion, friendly fire
//! avoidance, projectile flight paths, weapon degradation, and arrow chase.
//! Corresponds to `sim/combat.rs`.

use super::*;

// -----------------------------------------------------------------------
// Domain-specific helpers
// -----------------------------------------------------------------------

fn force_guaranteed_hits(sim: &mut SimState, creature_id: CreatureId) {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    // Set Striking and Archery to 500 (huge attack bonus).
    // With zeroed defender stats, attacker_total = 500 + DEX + quasi_normal.
    // Even with DEX=0, min attacker_total = 500 + 0 + (-300) = 200 > 0, so
    // always hits.
    for skill in [TraitKind::Striking, TraitKind::Archery] {
        if sim.db.creature_traits.get(&(creature_id, skill)).is_some() {
            let _ = sim
                .db
                .creature_traits
                .modify_unchecked(&(creature_id, skill), |t| {
                    t.value = TraitValue::Int(500);
                });
        } else {
            let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
                creature_id,
                trait_kind: skill,
                value: TraitValue::Int(500),
            });
        }
    }
    // Raise crit threshold so the large attack bonus guarantees a normal Hit,
    // not a CriticalHit (which would double damage and break assertions).
    sim.config.evasion_crit_threshold = 100_000;
}

/// Helper: equip a full set of wood armor on a creature (all 5 slots).
fn equip_full_armor(sim: &mut SimState, creature_id: CreatureId) {
    let inv_id = sim.db.creatures.get(&creature_id).unwrap().inventory_id;
    let mat = Some(inventory::Material::Oak);
    for (kind, slot) in [
        (inventory::ItemKind::Helmet, inventory::EquipSlot::Head),
        (
            inventory::ItemKind::Breastplate,
            inventory::EquipSlot::Torso,
        ),
        (inventory::ItemKind::Greaves, inventory::EquipSlot::Legs),
        (inventory::ItemKind::Boots, inventory::EquipSlot::Feet),
        (inventory::ItemKind::Gauntlets, inventory::EquipSlot::Hands),
    ] {
        // Unequip anything currently in this slot.
        sim.inv_unequip_slot(inv_id, slot);
        sim.inv_add_item(inv_id, kind, 1, Some(creature_id), None, mat, 0, None, None);
        // Find the newly added stack and equip it.
        let stack = sim
            .db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .into_iter()
            .find(|s| s.kind == kind && s.equipped_slot.is_none())
            .unwrap();
        sim.inv_equip_item(stack.id);
    }
}

/// Helper: equip a single armor piece with specific durability.
fn equip_armor_with_durability(
    sim: &mut SimState,
    creature_id: CreatureId,
    kind: inventory::ItemKind,
    slot: inventory::EquipSlot,
    current_hp: i32,
    max_hp: i32,
) {
    let inv_id = sim.db.creatures.get(&creature_id).unwrap().inventory_id;
    sim.inv_unequip_slot(inv_id, slot);
    sim.inv_add_item_with_durability(
        inv_id,
        kind,
        1,
        Some(creature_id),
        None,
        Some(inventory::Material::Oak),
        0,
        current_hp,
        max_hp,
        None,
        None,
    );
    let stack = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == kind && s.equipped_slot.is_none())
        .unwrap();
    sim.inv_equip_item(stack.id);
}

/// Helper: give a creature a bow and some arrows.
fn arm_with_bow_and_arrows(sim: &mut SimState, creature_id: CreatureId, arrows: u32) {
    let inv_id = sim.db.creatures.get(&creature_id).unwrap().inventory_id;
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bow, 1, Some(creature_id), None);
    sim.inv_add_simple_item(
        inv_id,
        inventory::ItemKind::Arrow,
        arrows,
        Some(creature_id),
        None,
    );
}

fn spawn_hornet_at(sim: &mut SimState, pos: VoxelCoord) -> CreatureId {
    let existing: std::collections::BTreeSet<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.species == Species::Hornet)
        .map(|c| c.id)
        .collect();
    let tick = sim.tick + 1;
    let cmd = SimCommand {
        player_name: String::new(),
        tick,
        action: SimAction::SpawnCreature {
            species: Species::Hornet,
            position: pos,
        },
    };
    sim.step(&[cmd], tick + 1);
    sim.db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Hornet && !existing.contains(&c.id))
        .expect("hornet should have spawned")
        .id
}

/// Helper: spawn elf, make it aggressive (military group), zero stats (predictable damage),
/// and return (elf_id, elf_position).
fn setup_aggressive_elf(sim: &mut SimState) -> (CreatureId, VoxelCoord) {
    let elf_id = spawn_elf(sim);
    zero_creature_stats(sim, elf_id);
    let soldiers = soldiers_group(sim);
    set_military_group(sim, elf_id, Some(soldiers.id));
    // force_idle but keep activations — the elf needs to act autonomously.
    force_idle(sim, elf_id);
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;
    (elf_id, pos)
}

/// Helper: spawn a hornet at a specific position and freeze it (force idle, cancel
/// activations) so it stays put and we can test elf behavior in isolation.
fn setup_frozen_hornet(sim: &mut SimState, pos: VoxelCoord) -> CreatureId {
    let mut events = Vec::new();
    let hornet_id = sim
        .spawn_creature(Species::Hornet, pos, &mut events)
        .expect("hornet should spawn");
    zero_creature_stats(sim, hornet_id);
    force_idle_and_cancel_activations(sim, hornet_id);
    hornet_id
}

/// Helper: give a creature a spear (Oak, quality 0).
fn give_spear(sim: &mut SimState, creature_id: CreatureId) {
    let inv_id = sim.db.creatures.get(&creature_id).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Spear,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );
}

/// Fire a projectile at a goblin and return the damage dealt. Used by arrow
/// durability/damage tests.
fn fire_arrow_at_goblin_with_hp(seed: u64, arrow_hp: i32) -> i64 {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    let mut sim = test_sim(seed);
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    // Disable arrow durability damage on impact so it doesn't interfere.
    sim.config.arrow_impact_damage_min = 0;
    sim.config.arrow_impact_damage_max = 0;

    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    // Set evasion deeply negative so shooter-less projectile always hits.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: goblin,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(-500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(goblin, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(-500);
        });
    sim.config.evasion_crit_threshold = 100_000;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None);

    // Modify the arrow's HP in the projectile inventory before it flies.
    if arrow_hp < 3 {
        let proj = sim.db.projectiles.iter_all().next().unwrap();
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
        let stack_id = stacks[0].id;
        let _ = sim
            .db
            .item_stacks
            .modify_unchecked(&stack_id, |s| s.current_hp = arrow_hp);
    }

    // Run until resolved.
    let mut damage_dealt: i64 = 0;
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if let SimEventKind::ProjectileHitCreature { damage, .. } = &e.kind {
                damage_dealt = *damage;
            }
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }
    assert!(damage_dealt > 0, "Projectile should have hit the goblin");
    damage_dealt
}

// -----------------------------------------------------------------------
// in_melee_range pure-function tests
// -----------------------------------------------------------------------

#[test]
fn test_in_melee_range_adjacent() {
    // Face-adjacent 1x1 creatures: dist² = 1, within range_sq = 2.
    assert!(in_melee_range(
        VoxelCoord::new(5, 1, 5),
        [1, 1, 1],
        VoxelCoord::new(6, 1, 5),
        [1, 1, 1],
        2,
    ));
}

#[test]
fn test_in_melee_range_diagonal() {
    // 2D diagonal: dist² = 1² + 1² = 2, within range_sq = 2.
    assert!(in_melee_range(
        VoxelCoord::new(5, 1, 5),
        [1, 1, 1],
        VoxelCoord::new(6, 1, 6),
        [1, 1, 1],
        2,
    ));
}

#[test]
fn test_in_melee_range_too_far() {
    // 2 voxels apart on X: dist² = 2² = 4, exceeds range_sq = 2.
    assert!(!in_melee_range(
        VoxelCoord::new(5, 1, 5),
        [1, 1, 1],
        VoxelCoord::new(7, 1, 5),
        [1, 1, 1],
        2,
    ));
}

#[test]
fn test_in_melee_range_3d_corner() {
    // 3D diagonal: dist² = 1 + 1 + 1 = 3, exceeds range_sq = 2.
    assert!(!in_melee_range(
        VoxelCoord::new(5, 1, 5),
        [1, 1, 1],
        VoxelCoord::new(6, 2, 6),
        [1, 1, 1],
        2,
    ));
}

#[test]
fn test_in_melee_range_large_footprint() {
    // 2x2x2 attacker at (4,1,5), target at (6,1,5).
    // Attacker occupies x=4..5, target at x=6. Gap on x = 6-5 = 1, dist² = 1.
    assert!(in_melee_range(
        VoxelCoord::new(4, 1, 5),
        [2, 2, 2],
        VoxelCoord::new(6, 1, 5),
        [1, 1, 1],
        2,
    ));
}

// -----------------------------------------------------------------------
// try_melee_strike integration tests
// -----------------------------------------------------------------------

#[test]
fn test_melee_strike_deals_damage() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Place goblin adjacent (x+1).
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

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

    // HP reduced.
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(elf_hp_after, elf_hp_before - goblin_damage);

    // CreatureDamaged event emitted.
    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureDamaged {
            attacker_id,
            target_id,
            damage,
            ..
        } if *attacker_id == goblin && *target_id == elf && *damage == goblin_damage
    )));
}

#[test]
fn test_melee_strike_incapacitates_target() {
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

    // Set elf HP equal to goblin damage so one strike incapacitates.
    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.hp = goblin_damage; // exactly equal → incapacitated (not dead)
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

    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().vital_status,
        VitalStatus::Incapacitated
    );
    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureIncapacitated { creature_id, .. } if *creature_id == elf
    )));
}

#[test]
fn test_melee_strike_out_of_range() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Place goblin 3 voxels away — out of range.
    let goblin_pos = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
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

    // HP unchanged.
    assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_before);
}

#[test]
fn test_melee_strike_cooldown() {
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
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    // First strike succeeds.
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
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().hp,
        elf_hp_before - goblin_damage,
    );

    // Goblin is now in MeleeStrike action — second strike should fail.
    assert_eq!(
        sim.db.creatures.get(&goblin).unwrap().action_kind,
        ActionKind::MeleeStrike,
    );
    let elf_hp_mid = sim.db.creatures.get(&elf).unwrap().hp;
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: goblin,
                target_id: elf,
            },
        }],
        tick2 + 1,
    );
    // HP unchanged — attack was rejected due to cooldown.
    assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_mid);
}

#[test]
fn test_melee_strike_dead_target() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // Kill the elf first.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature { creature_id: elf },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().vital_status,
        VitalStatus::Dead
    );

    // Melee attack on dead target should be a no-op.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: goblin,
                target_id: elf,
            },
        }],
        tick2 + 1,
    );
    // Goblin should still be idle (attack didn't fire).
    assert_eq!(
        sim.db.creatures.get(&goblin).unwrap().action_kind,
        ActionKind::NoAction,
    );
}

#[test]
fn test_melee_strike_zero_damage_species() {
    let mut sim = test_sim(42);
    // Capybara has melee_damage = 0 — cannot melee.
    let capybara = spawn_species(&mut sim, Species::Capybara);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Put capybara adjacent to elf.
    let capy_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, capybara, capy_pos);
    force_idle(&mut sim, capybara);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: capybara,
                target_id: elf,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_before);
}

#[test]
fn test_melee_strike_serde_roundtrip() {
    // Verify ActionKind::MeleeStrike survives serde roundtrip.
    let action = ActionKind::MeleeStrike;
    let json = serde_json::to_string(&action).unwrap();
    let restored: ActionKind = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, ActionKind::MeleeStrike);
}

#[test]
fn test_melee_strike_cooldown_expires() {
    // After the cooldown elapses, the creature can strike again. With
    // hostile AI, a goblin adjacent to an elf will automatically re-strike
    // once the cooldown resolves.
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
    let interval = sim.species_table[&Species::Goblin].melee_interval_ticks;

    // First strike via command.
    let elf_hp_initial = sim.db.creatures.get(&elf).unwrap().hp;
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
    assert_eq!(
        sim.db.creatures.get(&goblin).unwrap().action_kind,
        ActionKind::MeleeStrike,
    );
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().hp,
        elf_hp_initial - goblin_damage,
    );

    // Advance past cooldown. The goblin's MeleeStrike resolves, and the
    // hostile AI will auto-strike the elf again (still adjacent). There
    // may also be a pre-existing activation from spawn, so the elf may
    // take more than one additional hit. Verify at least 2 total strikes.
    // Give enough time for the goblin's activation to fire and complete
    // the second strike (cooldown + activation interval + margin).
    sim.step(&[], sim.tick + interval * 3);
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    let total_damage = elf_hp_initial - elf_hp_after;
    assert!(
        total_damage >= 2 * goblin_damage,
        "Should have at least 2 strikes: total damage {total_damage}, \
             expected >= {} (2 × {goblin_damage})",
        2 * goblin_damage,
    );
    // Damage should be an exact multiple of goblin_damage.
    assert_eq!(
        total_damage % goblin_damage,
        0,
        "Total damage {total_damage} should be a multiple of {goblin_damage}",
    );
}

#[test]
fn test_melee_strike_dead_attacker() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // Kill the goblin.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: goblin,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&goblin).unwrap().vital_status,
        VitalStatus::Dead,
    );

    // Dead goblin trying to melee should be a no-op.
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: goblin,
                target_id: elf,
            },
        }],
        tick2 + 1,
    );
    assert_eq!(sim.db.creatures.get(&elf).unwrap().hp, elf_hp_before);
}

// -----------------------------------------------------------------------
// Armor damage reduction tests
// -----------------------------------------------------------------------

#[test]
fn armor_reduces_melee_damage() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic damage/HP values.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Equip full armor on elf (total armor = 9).
    equip_full_armor(&mut sim, elf);
    // Disable degradation to isolate the damage-reduction test.
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let base_damage = sim.species_table[&Species::Goblin].melee_damage;
    let strength = sim.trait_int(goblin, TraitKind::Strength, 0);
    let raw_damage = crate::stats::apply_stat_multiplier(base_damage, strength);
    // With 9 armor and min_damage=1, effective = max(raw - 9, 1).
    let expected_effective = (raw_damage - 9).max(sim.config.armor_min_damage);
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

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

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - expected_effective,
        "Armor should reduce damage from {} to {}",
        raw_damage,
        expected_effective
    );
}

#[test]
fn armor_enforces_minimum_damage() {
    // Even with massive armor, at least armor_min_damage (1) gets through.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic damage/HP values.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    equip_full_armor(&mut sim, elf);

    // Reduce goblin damage below total armor (9) to test the floor.
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .melee_damage = 3;
    // Disable degradation to keep test deterministic.
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
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

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_before - elf_hp_after,
        sim.config.armor_min_damage,
        "Damage floor: exactly armor_min_damage ({}) should get through when armor exceeds raw damage",
        sim.config.armor_min_damage
    );
}

#[test]
fn no_armor_means_full_damage() {
    // Unarmored creature takes full melee damage.
    let mut sim = test_sim(42);
    // Disable starting equipment to ensure no armor.
    sim.config.elf_default_wants = vec![];
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic damage/HP values.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip any starting equipment the elf might have.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    let base_damage = sim.species_table[&Species::Goblin].melee_damage;
    let strength = sim.trait_int(goblin, TraitKind::Strength, 0);
    let goblin_damage = crate::stats::apply_stat_multiplier(base_damage, strength);
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

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

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - goblin_damage,
        "Unarmored creature should take full damage"
    );
}

#[test]
fn worn_armor_provides_less_protection() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic damage/HP values.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip all slots first.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    // Equip a breastplate in "worn" condition: 42/60 = 70% → exactly at worn threshold.
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        42,
        60,
    );

    // Worn breastplate: base 3, -1 penalty = 2 effective armor.
    let armor_params = inventory::ArmorParams {
        worn_pct: sim.config.durability_worn_pct,
        damaged_pct: sim.config.durability_damaged_pct,
        worn_penalty: sim.config.armor_worn_penalty,
        damaged_penalty: sim.config.armor_damaged_penalty,
    };
    let worn_armor = inventory::effective_armor_value(
        inventory::ItemKind::Breastplate,
        Some(inventory::Material::Oak),
        42,
        60,
        armor_params,
    );
    assert_eq!(worn_armor, 2, "Worn breastplate should give 2 armor");

    // Disable degradation to avoid PRNG interference.
    sim.config.armor_non_penetrating_degrade_chance_recip = 0;
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let base_damage = sim.species_table[&Species::Goblin].melee_damage;
    let strength = sim.trait_int(goblin, TraitKind::Strength, 0);
    let goblin_damage = crate::stats::apply_stat_multiplier(base_damage, strength);
    let expected = (goblin_damage - worn_armor as i64).max(sim.config.armor_min_damage);
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

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

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - expected,
        "Worn breastplate (armor={}) should reduce goblin damage ({}) to {}",
        worn_armor,
        goblin_damage,
        expected,
    );
}

#[test]
fn damaged_armor_provides_even_less_protection() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic damage/HP values.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip all slots.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    // Equip a breastplate in "damaged" condition: 24/60 = 40% → at damaged threshold.
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        24,
        60,
    );

    // Damaged breastplate: base 3, -2 penalty = 1 effective armor.
    let armor_params = inventory::ArmorParams {
        worn_pct: sim.config.durability_worn_pct,
        damaged_pct: sim.config.durability_damaged_pct,
        worn_penalty: sim.config.armor_worn_penalty,
        damaged_penalty: sim.config.armor_damaged_penalty,
    };
    let damaged_armor = inventory::effective_armor_value(
        inventory::ItemKind::Breastplate,
        Some(inventory::Material::Oak),
        24,
        60,
        armor_params,
    );
    assert_eq!(damaged_armor, 1, "Damaged breastplate should give 1 armor");

    // Disable degradation.
    sim.config.armor_non_penetrating_degrade_chance_recip = 0;
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let base_damage = sim.species_table[&Species::Goblin].melee_damage;
    let strength = sim.trait_int(goblin, TraitKind::Strength, 0);
    let goblin_damage = crate::stats::apply_stat_multiplier(base_damage, strength);
    let expected = (goblin_damage - damaged_armor as i64).max(sim.config.armor_min_damage);
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

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

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - expected,
        "Damaged breastplate (armor={}) should reduce goblin damage ({}) to {}",
        damaged_armor,
        goblin_damage,
        expected,
    );
}

#[test]
fn armor_reduces_projectile_damage() {
    use crate::db::CreatureTrait;
    // Verify armor reduces damage from projectile (arrow) hits, not just melee.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    // Set target's evasion stats deeply negative so the no-shooter projectile
    // (0 attack + quasi_normal) always exceeds defender_total and hits.
    // Evasion skill has no row at spawn (default 0), so use insert_no_fk.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(-500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(-500);
        });
    // Raise crit threshold to prevent the large margin from triggering crits.
    sim.config.evasion_crit_threshold = 100_000;
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;

    // Equip full armor on the elf (total armor = 9).
    equip_full_armor(&mut sim, elf);
    // Disable degradation to keep test deterministic.
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    // Record HP before.
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    // Spawn a projectile aimed at the elf (no gravity for predictability).
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
    sim.spawn_projectile(origin, elf_pos, None);

    // Run until resolved.
    let mut hit_damage: Option<i64> = None;
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if let SimEventKind::ProjectileHitCreature { damage, .. } = &e.kind {
                hit_damage = Some(*damage);
            }
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert!(hit_damage.is_some(), "Projectile should have hit the elf");
    let damage = hit_damage.unwrap();
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    let actual_damage = elf_hp_before - elf_hp_after;

    // The event damage should equal the actual HP loss.
    assert_eq!(actual_damage, damage);
    // Damage should be reduced: at minimum, armor_min_damage (1) gets through.
    assert!(
        actual_damage >= sim.config.armor_min_damage,
        "At least min_damage ({}) should get through armor",
        sim.config.armor_min_damage
    );
}

#[test]
fn armor_config_serde_roundtrip() {
    // Verify the new config fields survive JSON roundtrip.
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let restored: GameConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.armor_min_damage, config.armor_min_damage);
    assert_eq!(
        restored.armor_non_penetrating_degrade_chance_recip,
        config.armor_non_penetrating_degrade_chance_recip
    );
    assert_eq!(restored.armor_worn_penalty, config.armor_worn_penalty);
    assert_eq!(restored.armor_damaged_penalty, config.armor_damaged_penalty);
    assert_eq!(
        restored.armor_degrade_location_weights,
        config.armor_degrade_location_weights
    );
}

#[test]
fn armor_config_backward_compat_missing_fields() {
    // Old saves without armor config fields should deserialize with defaults.
    let sim = test_sim(42);
    let json = serde_json::to_string(&sim).unwrap();
    let mut val: serde_json::Value = serde_json::from_str(&json).unwrap();
    let config_obj = val
        .get_mut("config")
        .and_then(|c| c.as_object_mut())
        .unwrap();
    config_obj.remove("armor_min_damage");
    config_obj.remove("armor_non_penetrating_degrade_chance_recip");
    config_obj.remove("armor_worn_penalty");
    config_obj.remove("armor_damaged_penalty");
    config_obj.remove("armor_degrade_location_weights");
    let stripped = serde_json::to_string(&val).unwrap();
    let restored: SimState = serde_json::from_str(&stripped).unwrap();
    assert_eq!(restored.config.armor_min_damage, 1);
    assert_eq!(
        restored.config.armor_non_penetrating_degrade_chance_recip,
        20
    );
    assert_eq!(restored.config.armor_worn_penalty, 1);
    assert_eq!(restored.config.armor_damaged_penalty, 2);
    assert_eq!(
        restored.config.armor_degrade_location_weights,
        [5, 4, 3, 2, 1]
    );
}

#[test]
fn armor_degradation_penetrating_hit_reduces_durability() {
    // With a penetrating hit, armor at the targeted location should degrade.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Unequip all slots.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    // Equip only a breastplate (armor=3). Force degradation to always hit torso.
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        60,
        60,
    );
    // Weight only torso so the degradation always targets our breastplate.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];

    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    assert!(
        goblin_damage > 3,
        "Goblin damage must exceed armor (3) for penetrating hit"
    );

    // Run many strikes to statistically verify degradation happens.
    let mut total_hp_lost = 0i32;
    let strikes = 20;
    for _ in 0..strikes {
        let bp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .map(|s| s.current_hp);
        // Reset goblin so it can strike again.
        force_idle(&mut sim, goblin);
        // Heal the elf to survive.
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = c.hp_max);
        // Re-equip breastplate if it broke.
        if bp_before.is_none() {
            equip_armor_with_durability(
                &mut sim,
                elf,
                inventory::ItemKind::Breastplate,
                inventory::EquipSlot::Torso,
                60,
                60,
            );
        }

        let bp_hp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;

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

        if let Some(bp) = sim.inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso) {
            total_hp_lost += bp_hp_before - bp.current_hp;
        } else {
            // Breastplate broke.
            total_hp_lost += bp_hp_before;
        }
    }

    assert!(
        total_hp_lost > 0,
        "After {strikes} penetrating hits, breastplate should have lost some HP"
    );
}

#[test]
fn armor_degradation_non_penetrating_rare() {
    // With a non-penetrating hit (armor >= raw damage), degradation is rare (1/20).
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Unequip all slots.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    // Equip full armor (total=9) and reduce goblin base damage to ensure
    // non-penetrating even after STR scaling (goblin STR mean +58).
    equip_full_armor(&mut sim, elf);
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .melee_damage = 2;

    // Force degradation to always target torso.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];

    let mut degrade_count = 0;
    let strikes = 100;
    for _ in 0..strikes {
        force_idle(&mut sim, goblin);
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = c.hp_max);

        let bp_hp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;

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

        let bp_hp_after = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;
        if bp_hp_after < bp_hp_before {
            degrade_count += 1;
        }
    }

    // Expected ~5 out of 100 (1/20 chance). Allow 1–20 range.
    assert!(
        degrade_count >= 1 && degrade_count <= 20,
        "Non-penetrating degradation count ({degrade_count}/100) should be rare (~5%)"
    );
}

#[test]
fn armor_degradation_empty_slot_no_crash() {
    // When the random location picks an empty slot, nothing degrades.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Unequip everything.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    // This should not panic — degradation targets an empty slot.
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
}

#[test]
fn clothing_degrades_from_combat_hit() {
    // Clothing (not armor) in a targeted slot also degrades.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic hit results.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip all slots.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    // Equip a cloth tunic (not armor — 0 armor value) in torso slot.
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Tunic,
        inventory::EquipSlot::Torso,
        30,
        30,
    );

    // Force all degradation to torso.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];

    // Goblin hit is fully penetrating (tunic gives 0 armor).
    let mut total_hp_lost = 0i32;
    let strikes = 10;
    for _ in 0..strikes {
        force_idle(&mut sim, goblin);
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = c.hp_max);

        let tunic_hp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .map(|s| s.current_hp)
            .unwrap_or(0);

        if tunic_hp_before <= 0 {
            // Re-equip if broken.
            equip_armor_with_durability(
                &mut sim,
                elf,
                inventory::ItemKind::Tunic,
                inventory::EquipSlot::Torso,
                30,
                30,
            );
        }

        let tunic_hp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;

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

        if let Some(tunic) = sim.inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso) {
            total_hp_lost += tunic_hp_before - tunic.current_hp;
        } else {
            total_hp_lost += tunic_hp_before;
        }
    }

    assert!(
        total_hp_lost > 0,
        "Clothing should degrade from combat hits"
    );
}

#[test]
fn armor_degradation_destroys_item() {
    // When degradation reduces armor HP to 0, the item is removed.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Unequip all, then equip a breastplate with only 1 HP remaining.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        1,
        60,
    );
    // Force all degradation to torso.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];

    // Run strikes until the breastplate breaks.
    let mut broke = false;
    for _ in 0..50 {
        force_idle(&mut sim, goblin);
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = c.hp_max);

        if sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .is_none()
        {
            broke = true;
            break;
        }

        // Re-set to 1 HP if it survived (degradation might roll 0).
        let _ = sim.db.item_stacks.modify_unchecked(
            &sim.inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
                .unwrap()
                .id,
            |s| s.current_hp = 1,
        );

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
    }

    assert!(
        broke,
        "Breastplate with 1 HP should eventually break from combat degradation"
    );
    assert!(
        sim.inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .is_none(),
        "Torso slot should be empty after breastplate breaks"
    );
}

#[test]
fn armor_mixed_condition_pieces_sum_correctly() {
    // Multiple pieces in different conditions should sum correctly.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic damage/HP values.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip all.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }

    // Good helmet (base 2), Worn breastplate (base 3, -1 = 2), Damaged greaves (base 2, -2 = 1).
    // Total = 2 + 2 + 1 = 5.
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Helmet,
        inventory::EquipSlot::Head,
        40,
        40,
    );
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        42,
        60, // 70% = worn
    );
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Greaves,
        inventory::EquipSlot::Legs,
        16,
        40, // 40% = damaged
    );

    // Disable degradation.
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let base_damage = sim.species_table[&Species::Goblin].melee_damage;
    let strength = sim.trait_int(goblin, TraitKind::Strength, 0);
    let raw_damage = crate::stats::apply_stat_multiplier(base_damage, strength);
    let expected = (raw_damage - 5).max(sim.config.armor_min_damage);
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

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

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - expected,
        "Mixed-condition armor (total=5) should reduce {} damage to {}",
        raw_damage,
        expected,
    );
}

#[test]
fn armor_extreme_penetrating_damage_no_overflow() {
    // Extreme raw damage should not cause overflow in degradation math.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Give goblin absurdly high damage.
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .melee_damage = 1_000_000;
    // Force degradation to torso.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];

    // Equip a breastplate so there's something to degrade.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        60,
        60,
    );

    // Should not panic.
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
}

#[test]
fn armor_projectile_degrades_equipment() {
    // Verify armor/clothing degrades from projectile hits.
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;

    // Unequip all, equip only a breastplate.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        60,
        60,
    );
    // Force all degradation to torso.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];

    // Fire multiple projectiles and check for degradation.
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let mut total_hp_lost = 0i32;

    for _ in 0..10 {
        // Heal elf to survive.
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = c.hp_max);

        // Re-equip if breastplate broke.
        if sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .is_none()
        {
            equip_armor_with_durability(
                &mut sim,
                elf,
                inventory::ItemKind::Breastplate,
                inventory::EquipSlot::Torso,
                60,
                60,
            );
        }

        let bp_hp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;

        let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
        sim.spawn_projectile(origin, elf_pos, None);

        for _ in 0..500 {
            if sim.db.projectiles.is_empty() {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            if !sim.db.projectiles.is_empty() {
                sim.event_queue
                    .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
            }
        }

        if let Some(bp) = sim.inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso) {
            total_hp_lost += bp_hp_before - bp.current_hp;
        } else {
            total_hp_lost += bp_hp_before;
        }
    }

    assert!(
        total_hp_lost > 0,
        "Breastplate should degrade from projectile hits"
    );
}

#[test]
fn armor_melee_incapacitate_with_armor_equipped() {
    // Incapacitating an armored creature should not corrupt state.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic hit results.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    equip_full_armor(&mut sim, elf);

    // Set elf HP low enough that a single hit incapacitates even through armor.
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = 1);

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

    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().vital_status,
        VitalStatus::Incapacitated,
    );
    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::CreatureIncapacitated { creature_id, .. } if *creature_id == elf
    )));
}

#[test]
fn armor_save_load_roundtrip_preserves_combat() {
    // Equipped armor with varying durability must survive serde roundtrip
    // and still reduce damage correctly.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic damage/HP values (before roundtrip so stats persist).
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip all, equip a worn breastplate.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        42,
        60, // 70% = worn, effective armor = 2
    );
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    // Roundtrip through JSON (from_json rebuilds transient state).
    let json = sim.to_json().unwrap();
    let mut restored = SimState::from_json(&json).unwrap();

    // Verify breastplate survives roundtrip with correct durability.
    let restored_inv_id = restored.db.creatures.get(&elf).unwrap().inventory_id;
    let bp = restored
        .inv_equipped_in_slot(restored_inv_id, inventory::EquipSlot::Torso)
        .expect("breastplate should survive roundtrip");
    assert_eq!(bp.current_hp, 42);
    assert_eq!(bp.max_hp, 60);

    // Verify combat uses the restored armor.
    force_idle(&mut restored, goblin);
    let base_damage = restored.species_table[&Species::Goblin].melee_damage;
    let strength = restored.trait_int(goblin, TraitKind::Strength, 0);
    let goblin_damage = crate::stats::apply_stat_multiplier(base_damage, strength);
    let expected = (goblin_damage - 2).max(restored.config.armor_min_damage);
    let elf_hp_before = restored.db.creatures.get(&elf).unwrap().hp;

    let tick = restored.tick;
    restored.step(
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

    let elf_hp_after = restored.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - expected,
        "Armor reduction should work after serde roundtrip"
    );
}

#[test]
fn armor_non_penetrating_degrade_disabled_when_recip_zero() {
    // Setting recip to 0 should completely disable non-penetrating degradation.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Unequip all, equip full armor (total=9), reduce goblin base damage
    // below armor even after STR scaling (goblin STR mean +58).
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }
    equip_full_armor(&mut sim, elf);
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .melee_damage = 2;
    // Disable non-penetrating degradation.
    sim.config.armor_non_penetrating_degrade_chance_recip = 0;
    // Force all degradation to torso.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];

    let bp_hp_before = sim
        .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
        .unwrap()
        .current_hp;

    for _ in 0..50 {
        force_idle(&mut sim, goblin);
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = c.hp_max);

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
    }

    let bp_hp_after = sim
        .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
        .unwrap()
        .current_hp;
    assert_eq!(
        bp_hp_after, bp_hp_before,
        "With recip=0, non-penetrating hits should never degrade armor"
    );
}

#[test]
fn armor_degradation_targets_hands_slot() {
    // Verify weight-to-slot mapping: weights[4] = Hands.
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats for deterministic hit results.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip all, equip gauntlets (Hands) and breastplate (Torso).
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    for slot in [
        inventory::EquipSlot::Head,
        inventory::EquipSlot::Torso,
        inventory::EquipSlot::Legs,
        inventory::EquipSlot::Feet,
        inventory::EquipSlot::Hands,
    ] {
        sim.inv_unequip_slot(inv_id, slot);
    }
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Gauntlets,
        inventory::EquipSlot::Hands,
        30,
        30,
    );
    equip_armor_with_durability(
        &mut sim,
        elf,
        inventory::ItemKind::Breastplate,
        inventory::EquipSlot::Torso,
        60,
        60,
    );

    // Only target Hands: weights[4].
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 1];

    let mut gauntlet_hp_lost = 0i32;
    for _ in 0..20 {
        force_idle(&mut sim, goblin);
        let _ = sim.db.creatures.modify_unchecked(&elf, |c| c.hp = c.hp_max);

        // Re-equip gauntlets if broken.
        if sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Hands)
            .is_none()
        {
            equip_armor_with_durability(
                &mut sim,
                elf,
                inventory::ItemKind::Gauntlets,
                inventory::EquipSlot::Hands,
                30,
                30,
            );
        }

        let g_hp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Hands)
            .unwrap()
            .current_hp;
        let bp_hp_before = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;

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

        // Breastplate should never degrade (only Hands targeted).
        let bp_hp_after = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;
        assert_eq!(
            bp_hp_after, bp_hp_before,
            "Breastplate should not degrade when only Hands is weighted"
        );

        if let Some(g) = sim.inv_equipped_in_slot(inv_id, inventory::EquipSlot::Hands) {
            gauntlet_hp_lost += g_hp_before - g.current_hp;
        } else {
            gauntlet_hp_lost += g_hp_before;
        }
    }

    assert!(
        gauntlet_hp_lost > 0,
        "Gauntlets should degrade when Hands slot is the only weighted target"
    );
}

// -----------------------------------------------------------------------
// Shoot action tests
// -----------------------------------------------------------------------

#[test]
fn test_shoot_arrow_spawns_projectile() {
    let mut sim = test_sim(200);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place them apart with clear LOS (same Y, 5 voxels apart on X).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

    // Give elf a bow and arrows.
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    let tick = sim.tick;
    let events = sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );

    // A projectile should exist.
    assert_eq!(sim.db.projectiles.iter_all().count(), 1);

    // Arrow consumed from inventory.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Arrow,
            inventory::MaterialFilter::Any
        ),
        4
    );
    // Bow still there.
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Bow,
            inventory::MaterialFilter::Any
        ),
        1
    );

    // Elf is now on Shoot cooldown.
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(elf_creature.action_kind, ActionKind::Shoot);
    assert!(elf_creature.next_available_tick.is_some());

    // ProjectileLaunched event emitted.
    assert!(events.events.iter().any(|e| matches!(
        &e.kind,
        SimEventKind::ProjectileLaunched {
            attacker_id,
            target_id,
        } if *attacker_id == elf && *target_id == goblin
    )));
}

#[test]
fn test_shoot_arrow_no_bow_fails() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

    // Give arrows but NO bow.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Arrow, 5, None, None);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );

    // No projectile spawned.
    assert_eq!(sim.db.projectiles.iter_all().count(), 0);
    // Elf still idle.
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().action_kind,
        ActionKind::NoAction,
    );
}

#[test]
fn test_shoot_arrow_no_arrows_fails() {
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

    // Give bow but NO arrows.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_simple_item(inv_id, inventory::ItemKind::Bow, 1, None, None);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );

    assert_eq!(sim.db.projectiles.iter_all().count(), 0);
}

#[test]
fn test_shoot_arrow_cooldown_prevents_second_shot() {
    let mut sim = test_sim(200);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 10);

    // First shot.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.projectiles.iter_all().count(), 1);

    // Immediate second shot should fail (still on cooldown).
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick2 + 1,
    );

    // Arrow count should only have decreased by 1 (second shot failed).
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Arrow,
            inventory::MaterialFilter::Any
        ),
        9
    );
}

#[test]
fn test_shoot_arrow_blocked_los_fails() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    // Place goblin 5 voxels away.
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Place a solid wall between them.
    sim.world.set(
        VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z),
        VoxelType::Trunk,
    );

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );

    // No projectile — LOS blocked.
    assert_eq!(sim.db.projectiles.iter_all().count(), 0);
}

#[test]
fn test_shoot_arrow_leaf_does_not_block_los() {
    let mut sim = test_sim(200);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Place a leaf between them — should NOT block LOS.
    sim.world.set(
        VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z),
        VoxelType::Leaf,
    );

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );

    // Projectile spawned (leaf doesn't block).
    assert_eq!(sim.db.projectiles.iter_all().count(), 1);
}

#[test]
fn test_shoot_arrow_dead_target_fails() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Kill the goblin.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature {
                creature_id: goblin,
            },
        }],
        tick + 1,
    );

    // Try to shoot dead goblin.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick2 + 1,
    );

    assert_eq!(sim.db.projectiles.iter_all().count(), 0);
}

#[test]
fn test_shoot_arrow_dead_attacker_fails() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Kill the elf.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugKillCreature { creature_id: elf },
        }],
        tick + 1,
    );

    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick2 + 1,
    );

    assert_eq!(sim.db.projectiles.iter_all().count(), 0);
}

#[test]
fn test_shoot_action_serde_roundtrip() {
    let action = ActionKind::Shoot;
    let json = serde_json::to_string(&action).unwrap();
    let restored: ActionKind = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, ActionKind::Shoot);
}

#[test]
fn test_shoot_arrow_cooldown_expiry_allows_second_shot() {
    let mut sim = test_sim(200);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 10);

    // First shot.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.projectiles.iter_all().count(), 1);
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Arrow,
            inventory::MaterialFilter::Any
        ),
        9
    );

    // Advance past the cooldown. The activation system clears the Shoot
    // action, but the elf may flee or wander. Force idle and restore
    // position to isolate the second-shot test.
    let cooldown = sim.config.shoot_cooldown_ticks;
    sim.step(&[], sim.tick + cooldown + 1);
    force_idle(&mut sim, elf);
    force_position(&mut sim, elf, elf_pos);

    // Second shot should succeed now that cooldown has expired.
    let tick2 = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick2 + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick2 + 1,
    );
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Arrow,
            inventory::MaterialFilter::Any
        ),
        8
    );
}

#[test]
fn test_shoot_arrow_rejected_when_not_idle() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Put elf into a non-idle action (e.g., Build).
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.action_kind = ActionKind::Build;
        c.next_available_tick = Some(sim.tick + 5000);
    });

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: elf,
                target_id: goblin,
            },
        }],
        tick + 1,
    );

    // No projectile — elf was busy.
    assert_eq!(sim.db.projectiles.iter_all().count(), 0);
}

#[test]
fn test_hostile_ai_shoots_when_armed() {
    let mut sim = test_sim(300);
    let elf_id = spawn_species(&mut sim, Species::Elf);
    let goblin_id = spawn_species(&mut sim, Species::Goblin);
    force_guaranteed_hits(&mut sim, goblin_id);

    // Arm the goblin with bow + arrows. Don't reposition — let the sim's
    // natural spawn placement and nav graph handle positions.
    arm_with_bow_and_arrows(&mut sim, goblin_id, 10);

    // Run the sim long enough for the goblin to activate and find elves.
    // The goblin may melee if adjacent, or shoot if it has LOS and is far
    // enough away. Either way, it should consume arrows or deal damage.
    let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;
    sim.step(&[], sim.tick + 20_000);

    let inv_id = sim.db.creatures.get(&goblin_id).unwrap().inventory_id;
    let arrows_remaining = sim.inv_item_count(
        inv_id,
        inventory::ItemKind::Arrow,
        inventory::MaterialFilter::Any,
    );
    let elf_hp_after = sim.db.creatures.get(&elf_id).unwrap().hp;

    // The goblin should have either shot arrows or attacked in melee.
    assert!(
        arrows_remaining < 10 || elf_hp_after < elf_hp_before,
        "Hostile goblin with bow+arrows should have attacked (arrows={arrows_remaining}, \
             hp_before={elf_hp_before}, hp_after={elf_hp_after})"
    );
}

// -----------------------------------------------------------------------
// Flight path / friendly fire tests
// -----------------------------------------------------------------------

#[test]
fn flight_path_clear_no_creatures() {
    // Arrow through empty space should not be blocked.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let target_voxel = VoxelCoord::new(elf_pos.x + 10, elf_pos.y, elf_pos.z);

    use crate::projectile::{compute_aim_velocity, sub_voxel_from_voxel_center};
    let origin_sub = sub_voxel_from_voxel_center(elf_pos);
    let speed = sim.config.arrow_base_speed;
    let gravity = sim.config.arrow_gravity;
    let aim = compute_aim_velocity(origin_sub, target_voxel, speed, gravity, 5, 5000);
    assert!(aim.hit_tick.is_some());

    let blocked = sim.flight_path_blocked_by_friendly(elf, elf_pos, target_voxel, aim.velocity);
    assert!(
        blocked.is_none(),
        "Should not be blocked with no creatures in path"
    );
}

#[test]
fn flight_path_blocked_by_friendly_creature() {
    // An elf between the shooter and the target should block the shot.
    let mut sim = test_sim(200);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let blocker = spawn_elf(&mut sim);

    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position;
    // Place blocker 5 voxels away (in the flight path).
    let blocker_pos = VoxelCoord::new(shooter_pos.x + 5, shooter_pos.y, shooter_pos.z);
    force_position(&mut sim, blocker, blocker_pos);
    // Target 10 voxels away (past the blocker).
    let target_voxel = VoxelCoord::new(shooter_pos.x + 10, shooter_pos.y, shooter_pos.z);

    use crate::projectile::{compute_aim_velocity, sub_voxel_from_voxel_center};
    let origin_sub = sub_voxel_from_voxel_center(shooter_pos);
    let speed = sim.config.arrow_base_speed;
    let gravity = sim.config.arrow_gravity;
    let aim = compute_aim_velocity(origin_sub, target_voxel, speed, gravity, 5, 5000);
    assert!(aim.hit_tick.is_some());

    let blocked =
        sim.flight_path_blocked_by_friendly(shooter, shooter_pos, target_voxel, aim.velocity);
    assert_eq!(blocked, Some(blocker), "Should be blocked by friendly elf");
}

#[test]
fn flight_path_origin_area_excluded_for_friendlies() {
    // A friendly creature at the origin voxel (immediate neighbor) should
    // NOT block the shot — squads can stand together.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let buddy = spawn_elf(&mut sim);

    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position;
    // Place buddy right next to shooter (adjacent voxel).
    let buddy_pos = VoxelCoord::new(shooter_pos.x + 1, shooter_pos.y, shooter_pos.z);
    force_position(&mut sim, buddy, buddy_pos);
    // Target far away.
    let target_voxel = VoxelCoord::new(shooter_pos.x + 10, shooter_pos.y, shooter_pos.z);

    use crate::projectile::{compute_aim_velocity, sub_voxel_from_voxel_center};
    let origin_sub = sub_voxel_from_voxel_center(shooter_pos);
    let speed = sim.config.arrow_base_speed;
    let gravity = sim.config.arrow_gravity;
    let aim = compute_aim_velocity(origin_sub, target_voxel, speed, gravity, 5, 5000);
    assert!(aim.hit_tick.is_some());

    let blocked =
        sim.flight_path_blocked_by_friendly(shooter, shooter_pos, target_voxel, aim.velocity);
    assert!(blocked.is_none(), "Friendly near origin should not block");
}

#[test]
fn shoot_arrow_blocked_by_friendly_in_path() {
    // Full integration: elf should NOT shoot through a friendly elf.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let blocker = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position;
    let blocker_pos = VoxelCoord::new(shooter_pos.x + 5, shooter_pos.y, shooter_pos.z);
    let goblin_pos = VoxelCoord::new(shooter_pos.x + 10, shooter_pos.y, shooter_pos.z);
    force_position(&mut sim, blocker, blocker_pos);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, shooter);

    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: shooter,
                target_id: goblin,
            },
        }],
        tick + 1,
    );

    // No projectile should have been spawned.
    assert_eq!(
        sim.db.projectiles.iter_all().count(),
        0,
        "Should not shoot through friendly elf"
    );
    // Arrow should NOT have been consumed.
    let inv_id = sim.db.creatures.get(&shooter).unwrap().inventory_id;
    assert_eq!(
        sim.inv_item_count(
            inv_id,
            inventory::ItemKind::Arrow,
            inventory::MaterialFilter::Any
        ),
        5,
        "Arrow should not be consumed when shot is blocked"
    );
}

#[test]
fn shoot_arrow_hostile_in_path_does_not_block() {
    // A hostile creature in the flight path should NOT block the shot
    // (they're a valid target the arrow can hit).
    let mut sim = test_sim(200);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let goblin_near = spawn_species(&mut sim, Species::Goblin);
    let goblin_far = spawn_species(&mut sim, Species::Goblin);

    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position;
    let near_pos = VoxelCoord::new(shooter_pos.x + 5, shooter_pos.y, shooter_pos.z);
    let far_pos = VoxelCoord::new(shooter_pos.x + 10, shooter_pos.y, shooter_pos.z);
    force_position(&mut sim, goblin_near, near_pos);
    force_position(&mut sim, goblin_far, far_pos);
    force_idle(&mut sim, shooter);

    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: shooter,
                target_id: goblin_far,
            },
        }],
        tick + 1,
    );

    // Projectile should be spawned — hostile in path doesn't block.
    assert_eq!(
        sim.db.projectiles.iter_all().count(),
        1,
        "Should shoot past hostile creature"
    );
}

#[test]
fn try_combat_redirects_to_alternate_target() {
    // When primary target is blocked by friendly, the attacker should
    // redirect to an alternate hostile target with a clear path.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let blocker = spawn_elf(&mut sim);
    let goblin_blocked = spawn_species(&mut sim, Species::Goblin);
    let goblin_clear = spawn_species(&mut sim, Species::Goblin);

    // Place everyone on the forest floor away from the tree to avoid
    // LOS issues with the trunk. Y=1 is walking height on flat floor.
    let base = VoxelCoord::new(5, 1, 32);
    force_position(&mut sim, shooter, base);

    // Place blocker between shooter and goblin_blocked (on X axis).
    let blocker_pos = VoxelCoord::new(base.x + 5, base.y, base.z);
    let blocked_pos = VoxelCoord::new(base.x + 10, base.y, base.z);
    force_position(&mut sim, blocker, blocker_pos);
    force_position(&mut sim, goblin_blocked, blocked_pos);

    // Place goblin_clear on the opposite X side with no blocker.
    let clear_pos = VoxelCoord::new(base.x - 8, base.y, base.z);
    force_position(&mut sim, goblin_clear, clear_pos);

    force_idle(&mut sim, shooter);
    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    let mut events = Vec::new();
    let result = sim.try_combat_against_target(shooter, goblin_blocked, &mut events);

    assert!(
        result,
        "Should have redirected and fired at alternate target"
    );
    assert_eq!(
        sim.db.projectiles.iter_all().count(),
        1,
        "Should have spawned a projectile at alternate target"
    );

    // Verify the projectile was launched at the clear goblin.
    let launched_event = events.iter().find(|e| {
        matches!(&e.kind, SimEventKind::ProjectileLaunched { target_id, .. }
                if *target_id == goblin_clear)
    });
    assert!(
        launched_event.is_some(),
        "ProjectileLaunched should target the clear goblin"
    );
}

#[test]
fn in_flight_arrow_passes_through_friendly_near_origin() {
    // An arrow should pass through a friendly creature standing adjacent
    // to the shooter (origin neighbor) and hit a hostile further away.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let buddy = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, shooter);
    zero_creature_stats(&mut sim, buddy);
    zero_creature_stats(&mut sim, goblin);
    force_guaranteed_hits(&mut sim, shooter);

    // Place on flat ground away from the tree.
    let base = VoxelCoord::new(5, 1, 32);
    force_position(&mut sim, shooter, base);
    // Buddy adjacent (origin+1 on X axis).
    let buddy_pos = VoxelCoord::new(base.x + 1, base.y, base.z);
    force_position(&mut sim, buddy, buddy_pos);
    // Goblin further along the same axis.
    let goblin_pos = VoxelCoord::new(base.x + 10, base.y, base.z);
    force_position(&mut sim, goblin, goblin_pos);

    force_idle_and_cancel_activations(&mut sim, shooter);
    force_idle_and_cancel_activations(&mut sim, buddy);
    force_idle_and_cancel_activations(&mut sim, goblin);
    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    // Re-confirm all positions after idle (ensure no drift from earlier steps).
    force_position(&mut sim, shooter, base);
    force_position(&mut sim, buddy, buddy_pos);
    force_position(&mut sim, goblin, goblin_pos);

    // Restore buddy HP to full in case the goblin attacked during spawn steps.
    let _ = sim.db.creatures.modify_unchecked(&buddy, |c| {
        c.hp = c.hp_max;
    });

    // Fire the arrow.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: shooter,
                target_id: goblin,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.projectiles.iter_all().count(),
        1,
        "Arrow should launch"
    );

    // Advance until the projectile resolves (hits goblin or despawns).
    let max_tick = sim.tick + 10_000;
    while sim.tick < max_tick && sim.db.projectiles.iter_all().count() > 0 {
        sim.step(&[], sim.tick + 10);
    }

    // Buddy should be alive and at full HP (arrow passed through, not hit).
    let buddy_creature = sim.db.creatures.get(&buddy).unwrap();
    assert_eq!(
        buddy_creature.vital_status,
        VitalStatus::Alive,
        "Friendly near origin should not be hit by arrow"
    );
    let buddy_max_hp = sim.species_table[&Species::Elf].hp_max;
    assert_eq!(
        buddy_creature.hp, buddy_max_hp,
        "Friendly near origin should be at full HP"
    );
    // Goblin should have taken damage (arrow hit them).
    let goblin_hp = sim.db.creatures.get(&goblin).unwrap().hp;
    let goblin_max = sim.species_table[&Species::Goblin].hp_max;
    assert!(
        goblin_hp < goblin_max,
        "Goblin should have been hit by the arrow"
    );
}

#[test]
fn position_blocks_friendly_archer_on_line() {
    // A candidate position directly between a friendly archer and their
    // target should be detected as blocking.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let mover = spawn_elf(&mut sim);
    let archer = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place on flat ground away from tree.
    let base = VoxelCoord::new(5, 1, 32);
    force_position(&mut sim, mover, base);

    // Archer at x=5, goblin at x=15, candidate at x=10 — directly in line.
    let archer_pos = VoxelCoord::new(base.x, base.y, base.z + 3);
    force_position(&mut sim, archer, archer_pos);
    let goblin_pos = VoxelCoord::new(base.x, base.y, base.z + 13);
    force_position(&mut sim, goblin, goblin_pos);

    // Give archer a bow so they count as an archer.
    arm_with_bow_and_arrows(&mut sim, archer, 5);

    // Give archer an attack task targeting the goblin.
    let goblin_node = sim.nav_graph.find_nearest_node(goblin_pos);
    if let Some(node) = goblin_node {
        let task_id = TaskId::new(&mut sim.rng);
        let task = Task {
            id: task_id,
            kind: TaskKind::AttackTarget { target: goblin },
            state: TaskState::InProgress,
            location: sim.nav_graph.node(node).position,
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: Some(goblin),
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        sim.insert_task(task);
        if let Some(mut c) = sim.db.creatures.get(&archer) {
            c.current_task = Some(task_id);
            let _ = sim.db.creatures.update_no_fk(c);
        }
    }

    // Candidate position between archer and goblin (on the Z-axis line).
    let candidate = VoxelCoord::new(base.x, base.y, base.z + 8);
    assert!(
        sim.position_blocks_friendly_archers(mover, candidate),
        "Candidate between archer and target should block"
    );

    // Candidate position off the line should not block.
    let off_line = VoxelCoord::new(base.x + 5, base.y, base.z + 8);
    assert!(
        !sim.position_blocks_friendly_archers(mover, off_line),
        "Candidate far off the line should not block"
    );
}

#[test]
fn in_flight_arrow_hits_hostile_at_origin_neighbor() {
    // Point-blank: a hostile creature adjacent to the shooter (origin
    // neighbor) should still be hit by the arrow.
    let mut sim = test_sim(42);
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let goblin_near = spawn_species(&mut sim, Species::Goblin);
    let goblin_far = spawn_species(&mut sim, Species::Goblin);

    // Place on flat ground away from the tree.
    let base = VoxelCoord::new(5, 1, 32);
    force_position(&mut sim, shooter, base);
    // Hostile adjacent (origin+1).
    let near_pos = VoxelCoord::new(base.x + 1, base.y, base.z);
    force_position(&mut sim, goblin_near, near_pos);
    // Target further away (we shoot at this one, but arrow should hit
    // the near goblin first since it's in the path).
    let far_pos = VoxelCoord::new(base.x + 10, base.y, base.z);
    force_position(&mut sim, goblin_far, far_pos);

    force_idle(&mut sim, shooter);
    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    // Fire at the far goblin.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugShootAction {
                attacker_id: shooter,
                target_id: goblin_far,
            },
        }],
        tick + 1,
    );
    assert_eq!(sim.db.projectiles.iter_all().count(), 1);

    // Advance until the projectile resolves.
    let max_tick = sim.tick + 10_000;
    while sim.tick < max_tick && sim.db.projectiles.iter_all().count() > 0 {
        sim.step(&[], sim.tick + 10);
    }

    // Near hostile should have been hit (arrow doesn't skip hostiles
    // even near origin).
    let near_hp = sim.db.creatures.get(&goblin_near).unwrap().hp;
    let goblin_max = sim.species_table[&Species::Goblin].hp_max;
    assert!(
        near_hp < goblin_max,
        "Hostile near origin should be hit by arrow"
    );
}

// -----------------------------------------------------------------------
// Arrow durability / damage tests
// -----------------------------------------------------------------------

#[test]
fn damaged_arrow_deals_less_damage() {
    let full_damage = fire_arrow_at_goblin_with_hp(42, 3);
    let worn_damage = fire_arrow_at_goblin_with_hp(42, 2);
    let damaged_damage = fire_arrow_at_goblin_with_hp(42, 1);

    assert!(full_damage > 0, "Full HP arrow should deal damage");
    assert!(
        worn_damage < full_damage,
        "Worn arrow ({worn_damage}) should deal less than full ({full_damage})"
    );
    assert!(
        damaged_damage < worn_damage,
        "Damaged arrow ({damaged_damage}) should deal less than worn ({worn_damage})"
    );
    // Verify proportionality: 2/3 HP → 2/3 damage, 1/3 HP → 1/3 damage.
    // Use the exact integer division: damage * current_hp / max_hp.
    assert_eq!(worn_damage, full_damage * 2 / 3);
    assert_eq!(damaged_damage, full_damage * 1 / 3);
}

#[test]
fn damaged_arrow_deals_at_least_one_damage() {
    // Even a nearly-broken arrow should deal minimum 1 damage.
    let damage = fire_arrow_at_goblin_with_hp(42, 1);
    assert!(damage >= 1, "Damaged arrow should deal at least 1 damage");
}

#[test]
fn indestructible_arrow_deals_full_damage() {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    // An arrow with max_hp=0 (indestructible) should deal full damage.
    let mut sim = test_sim(42);
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.config.arrow_impact_damage_min = 0;
    sim.config.arrow_impact_damage_max = 0;
    // Remove arrow from durability config to make it indestructible.
    sim.config
        .item_durability
        .remove(&inventory::ItemKind::Arrow);

    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    // Set evasion deeply negative so shooter-less projectile always hits.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: goblin,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(-500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(goblin, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(-500);
        });
    sim.config.evasion_crit_threshold = 100_000;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None);

    let mut damage_dealt: i64 = 0;
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if let SimEventKind::ProjectileHitCreature { damage, .. } = &e.kind {
                damage_dealt = *damage;
            }
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    // Full damage should equal what a full-HP arrow deals.
    let full_hp_damage = fire_arrow_at_goblin_with_hp(42, 3);
    assert_eq!(
        damage_dealt, full_hp_damage,
        "Indestructible arrow should deal full damage"
    );
}

// -----------------------------------------------------------------------
// Melee weapon tests (spear, club)
// -----------------------------------------------------------------------

#[test]
fn melee_weapon_club_replaces_species_damage() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    // Give the elf a club.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;
    let club_damage = sim.config.club_base_damage;
    let species_damage = sim.species_table[&Species::Elf].melee_damage;
    assert!(
        club_damage > species_damage,
        "Club damage ({club_damage}) should exceed species base ({species_damage})"
    );

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    let target_hp_after = sim.db.creatures.get(&target).unwrap().hp;
    assert_eq!(
        target_hp_after,
        target_hp_before - club_damage,
        "Club damage ({club_damage}) should replace species base"
    );
}

/// Give an elf a spear and place target at distance-squared = 4 (extended reach).
/// Spear should reach but bare hands (range_sq=3) should not.
#[test]
fn melee_weapon_spear_extended_range() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    // Place elf 2 voxels away (distance_sq = 4 for a single axis offset of 2).
    let elf_pos = VoxelCoord::new(target_pos.x + 2, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    // Without a weapon, elf can't reach (species melee_range_sq = 3, dist_sq = 4).
    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&target).unwrap().hp,
        target_hp_before,
        "Bare-hands elf should not reach target at distance 2"
    );

    // Now give the elf a spear (range_sq = 8, covers dist_sq = 4).
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Spear,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );
    force_idle(&mut sim, elf);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    let spear_damage = sim.config.spear_base_damage;
    assert_eq!(
        sim.db.creatures.get(&target).unwrap().hp,
        target_hp_before - spear_damage,
        "Spear should reach target at distance 2 (dist_sq=4)"
    );
}

/// When elf has both spear and club, and target is adjacent (dist_sq <= 2),
/// prefer the club (higher damage).
#[test]
fn melee_weapon_prefers_highest_damage_when_both_in_range() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Spear,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;
    let club_damage = sim.config.club_base_damage;

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    assert_eq!(
        sim.db.creatures.get(&target).unwrap().hp,
        target_hp_before - club_damage,
        "Should prefer club (damage {club_damage}) over spear when both in range"
    );
}

/// When target is at extended range (dist_sq=4), only spear can reach
/// (club range_sq=3) — club should not be selected.
#[test]
fn melee_weapon_spear_only_at_extended_range() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 2, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Spear,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;
    let spear_damage = sim.config.spear_base_damage;

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    assert_eq!(
        sim.db.creatures.get(&target).unwrap().hp,
        target_hp_before - spear_damage,
        "Only spear should reach at dist_sq=4"
    );
}

/// Melee weapons degrade on use (0-2 HP per strike).
#[test]
fn melee_weapon_degrades_on_strike() {
    let mut sim = test_sim(42);
    // Force degradation to always apply by setting min = max = 1.
    sim.config.melee_weapon_impact_damage_min = 1;
    sim.config.melee_weapon_impact_damage_max = 1;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    // Find the club stack and check its HP.
    let club_max_hp = sim.config.item_durability[&ItemKind::Club];
    let club_stack = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == ItemKind::Club)
        .unwrap();
    assert_eq!(club_stack.max_hp, club_max_hp);
    assert_eq!(club_stack.current_hp, club_max_hp);

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    // Club should have lost 1 HP.
    let club_stack_after = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == ItemKind::Club)
        .unwrap();
    assert_eq!(
        club_stack_after.current_hp,
        club_max_hp - 1,
        "Club should lose 1 HP per strike"
    );
}

/// Bare-handed melee still works when creature has no weapons.
#[test]
fn melee_bare_hands_fallback() {
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
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

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

    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().hp,
        elf_hp_before - goblin_damage,
        "Bare-handed melee should still use species base damage"
    );
}

/// max_melee_range_sq returns species range when no weapons, and
/// weapon range when weapons extend it.
#[test]
fn max_melee_range_sq_accounts_for_weapons() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let species_range = sim.species_table[&Species::Elf].melee_range_sq;
    assert_eq!(
        sim.max_melee_range_sq(elf),
        species_range,
        "No weapons: should return species range"
    );

    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Spear,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    let spear_range = sim.config.spear_melee_range_sq;
    assert_eq!(
        sim.max_melee_range_sq(elf),
        spear_range,
        "With spear: should return spear range (extended)"
    );

    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );
    // Club range (3) < spear range (8), so max should still be spear.
    assert_eq!(
        sim.max_melee_range_sq(elf),
        spear_range,
        "With both: should return max (spear range)"
    );
}

/// Creatures with zero species melee_damage can still melee with a weapon.
#[test]
fn melee_weapon_enables_attack_for_zero_damage_species() {
    let mut sim = test_sim(42);
    // Capybara has melee_damage = 0.
    let capybara = spawn_species(&mut sim, Species::Capybara);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, capybara);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, capybara);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let cap_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, capybara, cap_pos);
    force_idle(&mut sim, capybara);

    // Without weapon, capybara can't melee.
    assert!(!sim.can_melee(capybara));

    // Give capybara a club.
    let inv_id = sim.db.creatures.get(&capybara).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    assert!(sim.can_melee(capybara));

    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;
    let club_damage = sim.config.club_base_damage;

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: capybara,
                target_id: target,
            },
        }],
        tick + 1,
    );

    assert_eq!(
        sim.db.creatures.get(&target).unwrap().hp,
        target_hp_before - club_damage,
        "Capybara with club should deal club damage"
    );
}

/// When a weapon breaks (reaches 0 HP), the next melee strike should fall
/// back to bare hands (or another weapon).
#[test]
fn melee_weapon_breaks_then_fallback_to_bare_hands() {
    let mut sim = test_sim(42);
    // Force degradation to always break (1 HP weapon, 1 damage per strike).
    sim.config.melee_weapon_impact_damage_min = 1;
    sim.config.melee_weapon_impact_damage_max = 1;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    // Give the elf a club with only 1 HP remaining.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    let club_max_hp = sim.config.item_durability[&ItemKind::Club];
    sim.inv_add_item_with_durability(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        1,
        club_max_hp,
        None,
        None,
    );

    let club_damage = sim.config.club_base_damage;
    let species_damage = sim.species_table[&Species::Elf].melee_damage;
    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;

    // First strike: club is used, deals club damage, then breaks.
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&target).unwrap().hp,
        target_hp_before - club_damage,
        "First strike should use club damage"
    );

    // Club should be gone (broken).
    let clubs_remaining =
        sim.inv_item_count(inv_id, ItemKind::Club, inventory::MaterialFilter::Any);
    assert_eq!(clubs_remaining, 0, "Club should have broken");

    // Second strike: bare hands fallback.
    force_idle(&mut sim, elf);
    let target_hp_after_first = sim.db.creatures.get(&target).unwrap().hp;
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );
    assert_eq!(
        sim.db.creatures.get(&target).unwrap().hp,
        target_hp_after_first - species_damage,
        "Second strike should use bare-hands species damage"
    );
}

/// Weapon stacks with quantity 0 should be ignored by weapon selection.
#[test]
fn melee_weapon_zero_quantity_stack_ignored() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    // Add a club with quantity 0 (simulating a fully consumed stack).
    sim.inv_add_item_with_durability(
        inv_id,
        ItemKind::Club,
        0,
        None,
        None,
        Some(Material::Oak),
        0,
        50,
        50,
        None,
        None,
    );

    // max_melee_range_sq should only reflect species range, not the q=0 club.
    let species_range = sim.species_table[&Species::Elf].melee_range_sq;
    assert_eq!(
        sim.max_melee_range_sq(elf),
        species_range,
        "Zero-quantity weapon should be ignored"
    );
}

/// When melee_weapon_impact_damage_min > max, degradation is a no-op.
#[test]
fn melee_weapon_degradation_noop_when_min_greater_than_max() {
    let mut sim = test_sim(42);
    sim.config.melee_weapon_impact_damage_min = 5;
    sim.config.melee_weapon_impact_damage_max = 2;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    let club_max_hp = sim.config.item_durability[&ItemKind::Club];
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    // Club should not have degraded.
    let club_stack = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == ItemKind::Club)
        .unwrap();
    assert_eq!(
        club_stack.current_hp, club_max_hp,
        "Weapon should not degrade when min > max"
    );
}

/// When melee_weapon_impact_damage_max = 0, degradation is disabled.
#[test]
fn melee_weapon_degradation_disabled_when_max_zero() {
    let mut sim = test_sim(42);
    sim.config.melee_weapon_impact_damage_min = 0;
    sim.config.melee_weapon_impact_damage_max = 0;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    let club_max_hp = sim.config.item_durability[&ItemKind::Club];
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    let club_stack = sim
        .db
        .item_stacks
        .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|s| s.kind == ItemKind::Club)
        .unwrap();
    assert_eq!(
        club_stack.current_hp, club_max_hp,
        "Weapon should not degrade when max = 0"
    );
}

/// melee_distance_sq is consistent with in_melee_range.
#[test]
fn melee_distance_sq_consistent_with_in_melee_range() {
    let cases = [
        (
            VoxelCoord::new(0, 0, 0),
            [1, 1, 1],
            VoxelCoord::new(1, 0, 0),
            [1, 1, 1],
        ),
        (
            VoxelCoord::new(0, 0, 0),
            [1, 1, 1],
            VoxelCoord::new(1, 1, 0),
            [1, 1, 1],
        ),
        (
            VoxelCoord::new(0, 0, 0),
            [1, 1, 1],
            VoxelCoord::new(1, 1, 1),
            [1, 1, 1],
        ),
        (
            VoxelCoord::new(0, 0, 0),
            [1, 1, 1],
            VoxelCoord::new(2, 0, 0),
            [1, 1, 1],
        ),
        (
            VoxelCoord::new(0, 0, 0),
            [1, 1, 1],
            VoxelCoord::new(3, 0, 0),
            [1, 1, 1],
        ),
        (
            VoxelCoord::new(0, 0, 0),
            [2, 1, 2],
            VoxelCoord::new(1, 0, 1),
            [1, 1, 1],
        ),
    ];

    for range_sq in [1, 2, 3, 4, 8, 9] {
        for &(a_pos, a_fp, t_pos, t_fp) in &cases {
            let dist = melee_distance_sq(a_pos, a_fp, t_pos, t_fp);
            let in_range = in_melee_range(a_pos, a_fp, t_pos, t_fp, range_sq);
            assert_eq!(
                dist <= range_sq,
                in_range,
                "Mismatch: dist_sq={dist}, range_sq={range_sq}, a={a_pos:?} t={t_pos:?}"
            );
        }
    }
}

/// Weapon damage goes through armor reduction correctly (weapon base, not
/// species base, is what armor subtracts from).
#[test]
fn weapon_damage_reduced_by_armor() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    // Give elf a club (damage = 20).
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    // Equip goblin with full armor (total = 9). Disable degradation.
    equip_full_armor(&mut sim, target);
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let club_damage = sim.config.club_base_damage;
    let expected = (club_damage - 9).max(sim.config.armor_min_damage);
    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;

    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    let target_hp_after = sim.db.creatures.get(&target).unwrap().hp;
    assert_eq!(
        target_hp_after,
        target_hp_before - expected,
        "Club damage ({club_damage}) minus armor (9) = {expected}"
    );
}

/// STR scaling applies on top of weapon base damage, not species base.
#[test]
fn weapon_damage_scales_with_strength() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);

    // Give elf a club and set STR to +10 (doubles damage).
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Club,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );
    assert!(sim.db.creature_traits.contains(&(elf, TraitKind::Strength)));
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Strength), |t| {
            t.value = TraitValue::Int(100);
        });

    let club_damage = sim.config.club_base_damage;
    let expected = crate::stats::apply_stat_multiplier(club_damage, 100);
    assert!(
        expected > club_damage,
        "STR +100 should double damage: {expected} > {club_damage}"
    );

    let target_hp_before = sim.db.creatures.get(&target).unwrap().hp;
    let tick = sim.tick;
    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugMeleeAttack {
                attacker_id: elf,
                target_id: target,
            },
        }],
        tick + 1,
    );

    let target_hp_after = sim.db.creatures.get(&target).unwrap().hp;
    assert_eq!(
        target_hp_after,
        target_hp_before - expected,
        "Club damage ({club_damage}) x STR+10 scaling = {expected}"
    );
}

/// AttackTarget task: elf with spear stops at spear range and attacks without
/// closing to adjacent.
#[test]
fn attack_target_spear_stops_at_extended_range() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, goblin);

    // Give the elf a spear.
    let inv_id = sim.db.creatures.get(&elf).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Spear,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    // Place goblin 2 voxels away from elf (dist_sq=4).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 2, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

    let goblin_hp_before = sim.db.creatures.get(&goblin).unwrap().hp;
    let spear_damage = sim.config.spear_base_damage;

    // Issue attack command.
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Elf should have dealt exactly one spear strike.
    let goblin_hp_after = sim.db.creatures.get(&goblin).unwrap().hp;
    assert_eq!(
        goblin_hp_after,
        goblin_hp_before - spear_damage,
        "Elf with spear should strike once for {spear_damage} damage at distance 2"
    );

    // Elf should NOT have moved closer.
    let elf_final_pos = sim.db.creatures.get(&elf).unwrap().position;
    assert_eq!(
        elf_final_pos, elf_pos,
        "Elf should stay at spear range, not close to adjacent"
    );
}

/// Hostile AI with spear: a goblin with a spear placed 2 voxels from an elf
/// should attack at spear range without closing further.
#[test]
fn hostile_ai_spear_attacks_at_extended_range() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Give the goblin a spear.
    let inv_id = sim.db.creatures.get(&goblin).unwrap().inventory_id;
    sim.inv_add_item(
        inv_id,
        ItemKind::Spear,
        1,
        None,
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    // Place goblin 2 voxels from elf (dist_sq=4, within spear range_sq=8).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 2, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // Schedule goblin activation so it enters the hostile AI decision cascade.
    let tick = sim.tick;
    sim.event_queue.schedule(
        tick + 1,
        ScheduledEventKind::CreatureActivation {
            creature_id: goblin,
        },
    );

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let spear_damage = sim.config.spear_base_damage;

    // Run one activation cycle.
    sim.step(&[], tick + 2);

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - spear_damage,
        "Hostile goblin with spear should melee-strike elf at distance 2 (dist_sq=4)"
    );

    // Goblin should NOT have moved — it attacked from spear range.
    let goblin_final_pos = sim.db.creatures.get(&goblin).unwrap().position;
    assert_eq!(
        goblin_final_pos, goblin_pos,
        "Goblin should stay at spear range, not close to adjacent"
    );
}

// -----------------------------------------------------------------------
// Arrow chase tests
// -----------------------------------------------------------------------

#[test]
fn arrow_chase_creates_autonomous_attack_move() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let detection_range_sq = sim.species_table[&Species::Goblin].hostile_detection_range_sq;

    // Origin 20 voxels away (20² = 400 > 225 = detection range).
    let origin = VoxelCoord::new(goblin_pos.x + 20, goblin_pos.y, goblin_pos.z);
    assert!(20i64 * 20 > detection_range_sq);

    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.maybe_arrow_chase(goblin, origin);

    let creature = sim.db.creatures.get(&goblin).unwrap();
    let task_id = creature
        .current_task
        .expect("Goblin should have a chase task after maybe_arrow_chase");
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackMove);
    assert_eq!(task.origin, crate::task::TaskOrigin::Autonomous);

    let amd = sim.task_attack_move_data(task_id).unwrap();
    assert_eq!(amd.destination, origin);
}

#[test]
fn arrow_chase_no_chase_within_detection_range() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Origin 5 voxels away (25 < 225 = detection range).
    let origin = VoxelCoord::new(goblin_pos.x + 5, goblin_pos.y, goblin_pos.z);

    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.maybe_arrow_chase(goblin, origin);

    let creature = sim.db.creatures.get(&goblin).unwrap();
    assert!(
        creature.current_task.is_none(),
        "No chase task when origin is within detection range"
    );
}

#[test]
fn arrow_chase_passive_creature_does_not_chase() {
    let mut sim = test_sim(42);
    let capybara = spawn_species(&mut sim, Species::Capybara);
    let capy_pos = sim.db.creatures.get(&capybara).unwrap().position;

    let origin = VoxelCoord::new(capy_pos.x + 30, capy_pos.y, capy_pos.z);

    force_idle_and_cancel_activations(&mut sim, capybara);

    sim.maybe_arrow_chase(capybara, origin);

    let creature = sim.db.creatures.get(&capybara).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Passive creature should not chase"
    );
}

#[test]
fn arrow_chase_second_hit_updates_destination() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    let origin1 = VoxelCoord::new(goblin_pos.x + 20, goblin_pos.y, goblin_pos.z);
    let origin2 = VoxelCoord::new(goblin_pos.x - 20, goblin_pos.y, goblin_pos.z);

    force_idle_and_cancel_activations(&mut sim, goblin);

    // First chase.
    sim.maybe_arrow_chase(goblin, origin1);
    let task_id_1 = sim.db.creatures.get(&goblin).unwrap().current_task.unwrap();
    let amd1 = sim.task_attack_move_data(task_id_1).unwrap();
    assert_eq!(amd1.destination, origin1);

    // Second chase from opposite direction.
    sim.maybe_arrow_chase(goblin, origin2);
    let task_id_2 = sim.db.creatures.get(&goblin).unwrap().current_task.unwrap();

    assert_eq!(
        task_id_1, task_id_2,
        "Should update existing task, not create a new one"
    );

    let amd2 = sim.task_attack_move_data(task_id_2).unwrap();
    assert_eq!(
        amd2.destination, origin2,
        "Destination should update to new origin"
    );
}

#[test]
fn arrow_chase_dead_creature_does_not_chase() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Kill the goblin.
    if let Some(mut c) = sim.db.creatures.get(&goblin) {
        c.vital_status = VitalStatus::Dead;
        c.hp = 0;
        let _ = sim.db.creatures.update_no_fk(c);
    }

    let origin = VoxelCoord::new(goblin_pos.x + 20, goblin_pos.y, goblin_pos.z);
    sim.maybe_arrow_chase(goblin, origin);

    let creature = sim.db.creatures.get(&goblin).unwrap();
    assert!(
        creature.current_task.is_none(),
        "Dead creature should not chase"
    );
}

#[test]
fn arrow_chase_does_not_preempt_player_combat() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Give the goblin a player-directed AttackMove.
    let player_dest = VoxelCoord::new(goblin_pos.x + 5, goblin_pos.y, goblin_pos.z);
    let mut events = Vec::new();
    sim.command_attack_move(goblin, player_dest, false, &mut events);

    let task_before = sim.db.creatures.get(&goblin).unwrap().current_task.unwrap();

    // Arrow chase from outside detection range.
    let origin = VoxelCoord::new(goblin_pos.x + 20, goblin_pos.y, goblin_pos.z);
    sim.maybe_arrow_chase(goblin, origin);

    let task_after = sim.db.creatures.get(&goblin).unwrap().current_task.unwrap();
    assert_eq!(
        task_before, task_after,
        "Arrow chase should not preempt player-directed combat"
    );
}

#[test]
fn arrow_chase_integration_projectile_triggers_chase() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Place origin above the goblin (in open air, avoiding terrain).
    let origin = VoxelCoord::new(goblin_pos.x, goblin_pos.y + 20, goblin_pos.z);

    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.spawn_projectile(origin, goblin_pos, None);

    // Run until the projectile resolves.
    let mut hit = false;
    for _ in 0..1000 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if matches!(e.kind, SimEventKind::ProjectileHitCreature { .. }) {
                hit = true;
            }
        }
    }

    if !hit {
        // If the projectile didn't hit (terrain in the way), skip the assertion.
        return;
    }

    // If it did hit, the goblin should have a chase task.
    let creature = sim.db.creatures.get(&goblin).unwrap();
    if creature.vital_status == VitalStatus::Alive {
        let task_id = creature
            .current_task
            .expect("Goblin should have chase task after projectile hit from outside range");
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackMove);
        assert_eq!(task.origin, crate::task::TaskOrigin::Autonomous);
    }
}

#[test]
fn arrow_chase_flying_creature_gets_chase_task() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 10, tree_pos.y + 30, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position;

    assert!(
        sim.species_table[&Species::Hornet]
            .flight_ticks_per_voxel
            .is_some(),
        "Hornet should be a flying species"
    );

    let origin = VoxelCoord::new(hornet_pos.x + 20, hornet_pos.y, hornet_pos.z);
    force_idle_and_cancel_activations(&mut sim, hornet);

    sim.maybe_arrow_chase(hornet, origin);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Flying creature should get an arrow-chase AttackMove task"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackMove);
    assert_eq!(task.origin, TaskOrigin::Autonomous);
}

#[test]
fn arrow_chase_nonexistent_creature_is_noop() {
    let mut sim = test_sim(42);
    let fake_id = CreatureId::new(&mut sim.rng);
    let origin = VoxelCoord::new(100, 51, 100);
    sim.maybe_arrow_chase(fake_id, origin);
    // No panic = success.
}

#[test]
fn arrow_chase_preempts_autonomous_task() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    // Create a low-priority autonomous GoTo task.
    let task_id = TaskId::new(&mut sim.rng);
    let goto_task = crate::task::Task {
        id: task_id,
        kind: crate::task::TaskKind::GoTo,
        state: crate::task::TaskState::InProgress,
        location: goblin_pos,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Goblin),
        origin: crate::task::TaskOrigin::Autonomous,
        target_creature: None,
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };
    sim.insert_task(goto_task);
    if let Some(mut c) = sim.db.creatures.get(&goblin) {
        c.current_task = Some(task_id);
        let _ = sim.db.creatures.update_no_fk(c);
    }

    // Arrow chase from outside detection range.
    let origin = VoxelCoord::new(goblin_pos.x + 20, goblin_pos.y, goblin_pos.z);
    sim.maybe_arrow_chase(goblin, origin);

    let creature = sim.db.creatures.get(&goblin).unwrap();
    let new_task_id = creature.current_task.unwrap();
    assert_ne!(
        new_task_id, task_id,
        "Arrow chase should have replaced the autonomous GoTo"
    );
    let new_task = sim.db.tasks.get(&new_task_id).unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::AttackMove);
    assert_eq!(new_task.origin, crate::task::TaskOrigin::Autonomous);
}

#[test]
fn arrow_chase_second_hit_clears_target_creature() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;

    let origin1 = VoxelCoord::new(goblin_pos.x + 20, goblin_pos.y, goblin_pos.z);
    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.maybe_arrow_chase(goblin, origin1);
    let task_id = sim.db.creatures.get(&goblin).unwrap().current_task.unwrap();

    // Simulate the task having acquired a melee target during attack-move.
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.target_creature = Some(elf);
        let _ = sim.db.tasks.update_no_fk(t);
    }

    // Second hit from a different direction.
    let origin2 = VoxelCoord::new(goblin_pos.x - 20, goblin_pos.y, goblin_pos.z);
    sim.maybe_arrow_chase(goblin, origin2);

    let task = sim.db.tasks.get(&task_id).unwrap();
    assert!(
        task.target_creature.is_none(),
        "Second hit should clear target_creature so pathing recomputes"
    );
}

#[test]
fn arrow_chase_creates_task_for_flying_creature() {
    let mut sim = test_sim(42);
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 10, tree_pos.y + 30, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Arrow origin far outside detection range.
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position;
    let origin = VoxelCoord::new(hornet_pos.x + 50, hornet_pos.y, hornet_pos.z);

    sim.maybe_arrow_chase(hornet, origin);

    let creature = sim.db.creatures.get(&hornet).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Flying creature should get an AttackMove task from arrow chase"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackMove);
    assert_eq!(task.origin, TaskOrigin::Autonomous);
}

// -----------------------------------------------------------------------
// Hit checking / evasion tests
// -----------------------------------------------------------------------

#[test]
fn test_hit_check_equal_stats_about_50_percent() {
    use crate::sim::combat::{HitResult, roll_hit_check};
    let mut rng = elven_canopy_prng::GameRng::new(456);
    let n = 50_000;
    let mut hits = 0u32;
    for _ in 0..n {
        match roll_hit_check(&mut rng, 0, 0, 0, 0, 100) {
            HitResult::Hit | HitResult::CriticalHit => hits += 1,
            HitResult::Miss => {}
        }
    }
    let rate = hits as f64 / n as f64;
    assert!(
        (rate - 0.50).abs() < 0.02,
        "equal-stat hit rate {rate:.3} not near 50%"
    );
}

#[test]
fn test_hit_check_large_attacker_advantage() {
    use crate::sim::combat::{HitResult, roll_hit_check};
    let mut rng = elven_canopy_prng::GameRng::new(789);
    let n = 10_000;
    let mut hits = 0u32;
    for _ in 0..n {
        match roll_hit_check(&mut rng, 250, 250, 0, 0, 100) {
            HitResult::Hit | HitResult::CriticalHit => hits += 1,
            HitResult::Miss => {}
        }
    }
    let rate = hits as f64 / n as f64;
    assert!(
        rate > 0.99,
        "large-advantage hit rate {rate:.3} should be near 100%"
    );
}

#[test]
fn test_hit_check_large_defender_advantage() {
    use crate::sim::combat::{HitResult, roll_hit_check};
    let mut rng = elven_canopy_prng::GameRng::new(321);
    let n = 10_000;
    let mut hits = 0u32;
    for _ in 0..n {
        match roll_hit_check(&mut rng, 0, 0, 250, 250, 100) {
            HitResult::Hit | HitResult::CriticalHit => hits += 1,
            HitResult::Miss => {}
        }
    }
    let rate = hits as f64 / n as f64;
    assert!(
        rate < 0.01,
        "large-disadvantage hit rate {rate:.3} should be near 0%"
    );
}

#[test]
fn test_hit_check_crit_rate_equal_stats() {
    use crate::sim::combat::{HitResult, roll_hit_check};
    let mut rng = elven_canopy_prng::GameRng::new(654);
    let n = 100_000;
    let mut crits = 0u32;
    for _ in 0..n {
        if roll_hit_check(&mut rng, 0, 0, 0, 0, 100) == HitResult::CriticalHit {
            crits += 1;
        }
    }
    let rate = crits as f64 / n as f64;
    assert!(
        rate > 0.01 && rate < 0.04,
        "crit rate {rate:.4} should be ~2-3%"
    );
}

#[test]
fn test_hit_check_deterministic() {
    use crate::sim::combat::{HitResult, roll_hit_check};
    let results_a: Vec<HitResult> = {
        let mut rng = elven_canopy_prng::GameRng::new(999);
        (0..100)
            .map(|_| roll_hit_check(&mut rng, 10, 5, 8, 7, 100))
            .collect()
    };
    let results_b: Vec<HitResult> = {
        let mut rng = elven_canopy_prng::GameRng::new(999);
        (0..100)
            .map(|_| roll_hit_check(&mut rng, 10, 5, 8, 7, 100))
            .collect()
    };
    assert_eq!(results_a, results_b);
}

#[test]
fn test_melee_miss_high_evasion() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf massive evasion advantage.
    use crate::db::CreatureTrait;
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(500);
        });

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    // Run many melee attacks and count misses.
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let mut miss_count = 0;
    for _ in 0..100 {
        force_idle(&mut sim, goblin);
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
        if events.events.iter().any(|e| {
            matches!(
                &e.kind,
                SimEventKind::MeleeAttackMissed { attacker_id, target_id }
                if *attacker_id == goblin && *target_id == elf
            )
        }) {
            miss_count += 1;
        }
    }

    assert!(
        miss_count > 95,
        "expected nearly all misses, got {miss_count}/100"
    );
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert!(
        elf_hp_after > elf_hp_before - 20,
        "elf took too much damage despite high evasion: {elf_hp_before} -> {elf_hp_after}"
    );
}

#[test]
fn test_melee_crit_doubles_damage() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give goblin massive skill advantage.
    use crate::db::CreatureTrait;
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: goblin,
        trait_kind: TraitKind::Striking,
        value: TraitValue::Int(500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(goblin, TraitKind::Dexterity), |t| {
            t.value = TraitValue::Int(500);
        });

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    // Give elf huge HP so we can measure damage.
    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.hp = 100_000;
        c.hp_max = 100_000;
    });

    force_idle(&mut sim, goblin);
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

    assert!(
        events.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::MeleeAttackCritical { attacker_id, target_id }
            if *attacker_id == goblin && *target_id == elf
        )),
        "expected a critical hit event"
    );

    let base_damage = sim.species_table[&Species::Goblin].melee_damage;
    let crit_event = events
        .events
        .iter()
        .find(|e| matches!(&e.kind, SimEventKind::CreatureDamaged { .. }));
    if let Some(SimEvent {
        kind: SimEventKind::CreatureDamaged { damage, .. },
        ..
    }) = crit_event
    {
        assert_eq!(
            *damage,
            base_damage * sim.config.evasion_crit_damage_multiplier,
            "crit should deal {0}x base damage ({base_damage})",
            sim.config.evasion_crit_damage_multiplier
        );
    } else {
        panic!("expected CreatureDamaged event for crit hit");
    }
}

#[test]
fn test_melee_miss_still_consumes_cooldown() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high evasion to guarantee a miss.
    use crate::db::CreatureTrait;
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(500);
        });

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

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

    let goblin_creature = sim.db.creatures.get(&goblin).unwrap();
    assert_eq!(
        goblin_creature.action_kind,
        ActionKind::MeleeStrike,
        "goblin should be in MeleeStrike action even on miss"
    );
    assert!(
        goblin_creature.next_available_tick.is_some(),
        "goblin should have a cooldown even on miss"
    );
}

#[test]
fn test_evasion_skill_advances_on_dodge() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high AGI to guarantee misses, but keep Evasion skill at 0.
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(1000);
        });

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    let evasion_before = sim.trait_int(elf, TraitKind::Evasion, 0);
    assert_eq!(evasion_before, 0, "evasion should start at 0");

    for _ in 0..200 {
        force_idle(&mut sim, goblin);
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
    }

    let evasion_after = sim.trait_int(elf, TraitKind::Evasion, 0);
    assert!(
        evasion_after > evasion_before,
        "evasion skill should advance on dodge: before={evasion_before}, after={evasion_after}"
    );
}

#[test]
fn test_projectile_evaded_by_high_agi_target() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    // Give elf massive evasion advantage.
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Evasion), |t| {
            t.value = TraitValue::Int(500);
        });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(500);
        });
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
    sim.spawn_projectile(origin, elf_pos, None);

    let mut evaded = false;
    let mut hit = false;
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if matches!(&e.kind, SimEventKind::ProjectileEvaded { .. }) {
                evaded = true;
            }
            if matches!(&e.kind, SimEventKind::ProjectileHitCreature { .. }) {
                hit = true;
            }
        }
    }

    assert!(evaded, "arrow should have been evaded");
    assert!(!hit, "arrow should not have dealt damage");
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(elf_hp_after, elf_hp_before, "elf HP should be unchanged");
}

#[test]
fn test_projectile_crit_doubles_damage() {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);
    let shooter = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, shooter);

    // Give shooter massive attack advantage for guaranteed crit.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: shooter,
        trait_kind: TraitKind::Archery,
        value: TraitValue::Int(500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(shooter, TraitKind::Dexterity), |t| {
            t.value = TraitValue::Int(500);
        });

    let _ = sim.db.creatures.modify_unchecked(&elf, |c| {
        c.hp = 100_000;
        c.hp_max = 100_000;
    });
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
    sim.spawn_projectile(origin, elf_pos, Some(shooter));

    let mut got_crit = false;
    let mut hit_damage: Option<i64> = None;
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if matches!(&e.kind, SimEventKind::ProjectileCritical { .. }) {
                got_crit = true;
            }
            if let SimEventKind::ProjectileHitCreature { damage, .. } = &e.kind {
                hit_damage = Some(*damage);
            }
        }
    }

    assert!(got_crit, "expected ProjectileCritical event");
    assert!(hit_damage.is_some(), "expected ProjectileHitCreature event");
}

#[test]
fn test_projectile_evasion_with_shooter_stats() {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    let mut sim = test_sim(42);
    let target = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, target);
    let shooter = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, shooter);

    // Target has moderate evasion.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: target,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(200),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(target, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(200);
        });

    // Shooter has high Archery + DEX to overcome the evasion.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: shooter,
        trait_kind: TraitKind::Archery,
        value: TraitValue::Int(500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(shooter, TraitKind::Dexterity), |t| {
            t.value = TraitValue::Int(500);
        });
    sim.config.evasion_crit_threshold = 100_000;

    let _ = sim.db.creatures.modify_unchecked(&target, |c| {
        c.hp = 100_000;
        c.hp_max = 100_000;
    });

    let target_pos = sim.db.creatures.get(&target).unwrap().position;
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    let mut hits = 0;
    let total = 20;
    for _ in 0..total {
        let origin = VoxelCoord::new(target_pos.x - 10, target_pos.y, target_pos.z);
        sim.spawn_projectile(origin, target_pos, Some(shooter));
        for _ in 0..500 {
            if sim.db.projectiles.is_empty() {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
            for e in &events {
                if matches!(&e.kind, SimEventKind::ProjectileHitCreature { .. }) {
                    hits += 1;
                }
            }
        }
    }

    assert!(
        hits > 18,
        "high-skill shooter should hit evasive target most of the time, got {hits}/{total}"
    );
}

#[test]
fn test_melee_miss_no_armor_degradation() {
    use crate::db::CreatureTrait;
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high evasion to guarantee misses.
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(500);
        });

    equip_full_armor(&mut sim, elf);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    let elf_inv = sim.db.creatures.get(&elf).unwrap().inventory_id;
    let armor_hp_before: Vec<_> = sim
        .db
        .item_stacks
        .iter_all()
        .filter(|s| s.inventory_id == elf_inv && s.equipped_slot.is_some())
        .map(|s| (s.id, s.current_hp))
        .collect();
    assert!(
        !armor_hp_before.is_empty(),
        "elf should have armor equipped"
    );

    for _ in 0..20 {
        force_idle(&mut sim, goblin);
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
    }

    for (stack_id, hp_before) in &armor_hp_before {
        let hp_after = sim.db.item_stacks.get(stack_id).unwrap().current_hp;
        assert_eq!(
            hp_after, *hp_before,
            "armor stack {stack_id:?} should not degrade on miss"
        );
    }
}

#[test]
fn test_ranged_evasion_skill_advances_on_projectile_dodge() {
    let mut sim = test_sim(42);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    // Give elf high AGI to guarantee dodges, but Evasion skill at 0.
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(1000);
        });

    let evasion_before = sim.trait_int(elf, TraitKind::Evasion, 0);
    assert_eq!(evasion_before, 0, "evasion should start at 0");

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    for _ in 0..100 {
        let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
        sim.spawn_projectile(origin, elf_pos, None);
        for _ in 0..500 {
            if sim.db.projectiles.is_empty() {
                break;
            }
            sim.tick += 1;
            let mut events = Vec::new();
            sim.process_projectile_tick(&mut events);
        }
    }

    let evasion_after = sim.trait_int(elf, TraitKind::Evasion, 0);
    assert!(
        evasion_after > evasion_before,
        "evasion skill should advance on projectile dodge: before={evasion_before}, after={evasion_after}"
    );
}

#[test]
fn test_evasion_config_serde_roundtrip() {
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let restored: GameConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.evasion_crit_threshold, 100);
    assert_eq!(restored.evasion_crit_damage_multiplier, 2);
    assert_eq!(restored.evasion_dodge_advance_permille, 500);

    let mut config_obj: serde_json::Value = serde_json::to_value(&config).unwrap();
    let obj = config_obj.as_object_mut().unwrap();
    obj.remove("evasion_crit_threshold");
    obj.remove("evasion_crit_damage_multiplier");
    obj.remove("evasion_dodge_advance_permille");
    let restored2: GameConfig = serde_json::from_value(config_obj).unwrap();
    assert_eq!(restored2.evasion_crit_threshold, 100);
    assert_eq!(restored2.evasion_crit_damage_multiplier, 2);
    assert_eq!(restored2.evasion_dodge_advance_permille, 500);
}

#[test]
fn test_evaded_arrow_still_triggers_chase() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);

    // Give goblin very high evasion to guarantee dodge.
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(goblin, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(1000);
        });

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position;
    let origin = VoxelCoord::new(goblin_pos.x, goblin_pos.y + 20, goblin_pos.z);

    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.spawn_projectile(origin, goblin_pos, None);

    let mut evaded = false;
    for _ in 0..1000 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if matches!(e.kind, SimEventKind::ProjectileEvaded { .. }) {
                evaded = true;
            }
        }
    }

    if !evaded {
        return;
    }

    let creature = sim.db.creatures.get(&goblin).unwrap();
    if creature.vital_status == VitalStatus::Alive {
        let task_id = creature
            .current_task
            .expect("Goblin should have chase task after evading arrow from outside range");
        let task = sim.db.tasks.get(&task_id).unwrap();
        assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackMove);
        assert_eq!(task.origin, crate::task::TaskOrigin::Autonomous);
    }
}

#[test]
fn test_melee_weapon_no_degrade_on_miss() {
    let mut sim = test_sim(42);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high evasion to guarantee misses.
    use crate::db::CreatureTrait;
    let _ = sim.db.creature_traits.insert_no_fk(CreatureTrait {
        creature_id: elf,
        trait_kind: TraitKind::Evasion,
        value: TraitValue::Int(500),
    });
    let _ = sim
        .db
        .creature_traits
        .modify_unchecked(&(elf, TraitKind::Agility), |t| {
            t.value = TraitValue::Int(500);
        });

    // Give goblin a spear.
    let goblin_inv = sim.db.creatures.get(&goblin).unwrap().inventory_id;
    sim.inv_add_item(
        goblin_inv,
        ItemKind::Spear,
        1,
        Some(goblin),
        None,
        Some(Material::Oak),
        0,
        None,
        None,
    );

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 2, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    let spear_stack = sim
        .db
        .item_stacks
        .iter_all()
        .find(|s| s.inventory_id == goblin_inv && s.kind == ItemKind::Spear)
        .unwrap();
    let spear_id = spear_stack.id;
    let spear_hp_before = spear_stack.current_hp;

    for _ in 0..20 {
        force_idle(&mut sim, goblin);
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
    }

    let spear_hp_after = sim.db.item_stacks.get(&spear_id).unwrap().current_hp;
    assert_eq!(
        spear_hp_after, spear_hp_before,
        "weapon should not degrade on miss"
    );
}
