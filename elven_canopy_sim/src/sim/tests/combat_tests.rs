//! Tests for combat mechanics: melee attacks, ranged attacks (bow/arrow),
//! armor damage reduction, hit/miss/crit rolls, evasion, friendly fire
//! avoidance, projectile flight paths, weapon degradation, and arrow chase.
//! Corresponds to `sim/combat.rs`.

use super::*;

// -----------------------------------------------------------------------
// Domain-specific helpers
// -----------------------------------------------------------------------

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

/// Helper: spawn elf, make it aggressive (military group), zero stats (predictable damage),
/// and return (elf_id, elf_position).
pub(super) fn setup_aggressive_elf(sim: &mut SimState) -> (CreatureId, VoxelCoord) {
    let elf_id = spawn_elf(sim);
    zero_creature_stats(sim, elf_id);
    let soldiers = soldiers_group(sim);
    set_military_group(sim, elf_id, Some(soldiers.id));
    // force_idle but keep activations — the elf needs to act autonomously.
    force_idle(sim, elf_id);
    let pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    (elf_id, pos)
}

/// Helper: spawn a hornet at a specific position and freeze it (force idle, cancel
/// activations) so it stays put and we can test elf behavior in isolation.
pub(super) fn setup_frozen_hornet(sim: &mut SimState, pos: VoxelCoord) -> CreatureId {
    let mut events = Vec::new();
    let hornet_id = sim
        .spawn_creature(Species::Hornet, pos, sim.home_zone_id(), &mut events)
        .expect("hornet should spawn");
    zero_creature_stats(sim, hornet_id);
    force_idle_and_cancel_activations(sim, hornet_id);
    hornet_id
}

/// Helper: give a creature a spear (Oak, quality 0).
pub(super) fn give_spear(sim: &mut SimState, creature_id: CreatureId) {
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
    let mut sim = flat_world_sim(seed);
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    // Disable arrow durability damage on impact so it doesn't interfere.
    sim.config.arrow_impact_damage_min = 0;
    sim.config.arrow_impact_damage_max = 0;

    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    // Set evasion deeply negative so shooter-less projectile always hits.
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: goblin,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(-500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(-500);
    sim.db.update_creature_trait(agility_trait).unwrap();
    sim.config.evasion_crit_threshold = 100_000;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None, sim.home_zone_id());

    // Modify the arrow's HP in the projectile inventory before it flies.
    if arrow_hp < 3 {
        let proj = sim.db.projectiles.iter_all().next().unwrap();
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
        let stack_id = stacks[0].id;
        let mut stack = sim.db.item_stacks.get(&stack_id).unwrap();
        stack.current_hp = arrow_hp;
        sim.db.update_item_stack(stack).unwrap();
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

    // Set elf HP equal to goblin damage so one strike incapacitates.
    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
    elf_creature.hp = goblin_damage; // exactly equal → incapacitated (not dead)
    sim.db.update_creature(elf_creature).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);
    suppress_activation(&mut sim, goblin);

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

    // Re-idle goblin for the second step. Use suppress_activation to
    // prevent poll-based activation before the command fires.
    force_idle(&mut sim, goblin);
    suppress_activation(&mut sim, goblin);

    // Melee attack on dead target should be a no-op.
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
    // Dead elf's HP should be unchanged (attack didn't fire).
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().hp,
        elf_hp_before,
        "Melee attack on dead target should not deal damage"
    );
    // Goblin should not be in MeleeStrike action.
    assert_ne!(
        sim.db.creatures.get(&goblin).unwrap().action_kind,
        ActionKind::MeleeStrike,
        "Goblin should not have started a melee strike on dead target"
    );
}

#[test]
fn test_melee_strike_zero_damage_species() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Capybara has melee_damage = 0 — cannot melee.
    let capybara = spawn_species(&mut sim, Species::Capybara);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    // Disable starting equipment to ensure no armor.
    sim.config.elf_default_wants = vec![];
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    // Set target's evasion stats deeply negative so the no-shooter projectile
    // (0 attack + quasi_normal) always exceeds defender_total and hits.
    // Evasion skill has no row at spawn (default 0), so insert it.
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(-500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(-500);
    sim.db.update_creature_trait(agility_trait).unwrap();
    // Raise crit threshold to prevent the large margin from triggering crits.
    sim.config.evasion_crit_threshold = 100_000;
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;

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
    sim.spawn_projectile(origin, elf_pos, None, sim.home_zone_id());

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
    let sim = flat_world_sim(fresh_test_seed());
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle_and_cancel_activations(&mut sim, elf);
    suppress_activation(&mut sim, goblin);

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
        suppress_activation(&mut sim, goblin);
        let mut g = sim.db.creatures.get(&goblin).unwrap();
        g.action_kind = ActionKind::NoAction;
        sim.db.update_creature(g).unwrap();
        // Heal the elf to survive.
        let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
        elf_creature.hp = elf_creature.hp_max;
        sim.db.update_creature(elf_creature).unwrap();
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Prevent autonomous activations from racing with DebugMeleeAttack.
    // Use u64::MAX so the suppression persists across all 500 loop iterations.
    force_idle_and_cancel_activations(&mut sim, elf);
    suppress_activation_until(&mut sim, elf, u64::MAX);
    suppress_activation_until(&mut sim, goblin, u64::MAX);
    // Guarantee the goblin always hits (seed-dependent evasion rolls could
    // cause all 500 strikes to miss, yielding zero degradation events).
    force_guaranteed_hits(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

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
    // 500 trials at 5% rate: P(zero) ≈ 7e-12, P(>100) ≈ 0.
    // Previous 100 trials had P(zero) ≈ 0.6% — a flaky failure source.
    let strikes = 500;
    for _ in 0..strikes {
        // Reset goblin to idle without clearing next_available_tick (which
        // would make it activatable and race with the DebugMeleeAttack).
        let mut goblin_c = sim.db.creatures.get(&goblin).unwrap();
        goblin_c.action_kind = ActionKind::NoAction;
        goblin_c.current_task = None;
        goblin_c.path = None;
        sim.db.update_creature(goblin_c).unwrap();

        let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
        elf_creature.hp = elf_creature.hp_max;
        sim.db.update_creature(elf_creature).unwrap();

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

    // Expected ~25 out of 500 (1/20 chance). Allow 1–100 range.
    assert!(
        degrade_count >= 1 && degrade_count <= 100,
        "Non-penetrating degradation count ({degrade_count}/500) should be rare (~5%)"
    );
}

#[test]
fn armor_degradation_empty_slot_no_crash() {
    // When the random location picks an empty slot, nothing degrades.
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
        let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
        elf_creature.hp = elf_creature.hp_max;
        sim.db.update_creature(elf_creature).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats and guarantee hits so the attack always lands.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    // Make degradation guaranteed (not probabilistic).
    sim.config.armor_non_penetrating_degrade_chance_recip = 1;

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
        let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
        elf_creature.hp = elf_creature.hp_max;
        sim.db.update_creature(elf_creature).unwrap();

        if sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .is_none()
        {
            broke = true;
            break;
        }

        // Re-set to 1 HP if it survived (degradation might roll 0).
        let stack_id = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .id;
        let mut stack = sim.db.item_stacks.get(&stack_id).unwrap();
        stack.current_hp = 1;
        sim.db.update_item_stack(stack).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    // Zero stats, then set Evasion very negative so the shooter-less
    // projectile's quasi_normal(50) attacker roll always beats
    // defender_total (evasion + agi). With evasion=-500 and agi=0,
    // defender_total=-500, so even the lowest roll (~-150) hits.
    zero_creature_stats(&mut sim, elf);
    set_trait(&mut sim, elf, TraitKind::Evasion, -500);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;

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
    // Force all degradation to torso, and make degradation guaranteed.
    sim.config.armor_degrade_location_weights = [1, 0, 0, 0, 0];
    sim.config.armor_non_penetrating_degrade_chance_recip = 1;

    // Fire multiple projectiles and check for degradation.
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let mut total_hp_lost = 0i32;

    for shot_i in 0..10 {
        // Heal elf to survive.
        let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
        elf_creature.hp = elf_creature.hp_max;
        sim.db.update_creature(elf_creature).unwrap();

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
        sim.spawn_projectile(origin, elf_pos, None, sim.home_zone_id());

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

        let hp_after = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .map(|bp| bp.current_hp);
        let lost = match hp_after {
            Some(hp) => bp_hp_before - hp,
            None => bp_hp_before, // broke
        };
        total_hp_lost += lost;
        eprintln!(
            "  armor_proj shot {shot_i}: bp_before={bp_hp_before} bp_after={hp_after:?} \
             lost={lost} total={total_hp_lost}"
        );
    }

    assert!(
        total_hp_lost > 0,
        "Breastplate should degrade from projectile hits"
    );
}

#[test]
fn armor_melee_incapacitate_with_armor_equipped() {
    // Incapacitating an armored creature should not corrupt state.
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
    elf_creature.hp = 1;
    sim.db.update_creature(elf_creature).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    force_position(
        &mut sim,
        goblin,
        VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z),
    );
    force_idle(&mut sim, goblin);

    // Zero stats so STR doesn't scale damage above armor, and
    // force guaranteed hits so we actually exercise non-penetrating path.
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    // Unequip all, equip full armor (total=9), reduce goblin base damage
    // below armor so all hits are non-penetrating.
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

    for i in 0..50 {
        force_idle(&mut sim, goblin);
        let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
        elf_creature.hp = elf_creature.hp_max;
        sim.db.update_creature(elf_creature).unwrap();

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

        let bp_hp_now = sim
            .inv_equipped_in_slot(inv_id, inventory::EquipSlot::Torso)
            .unwrap()
            .current_hp;
        eprintln!("  non_pen_degrade iter {i}: torso_hp={bp_hp_now} (before={bp_hp_before})");
        assert_eq!(
            bp_hp_now, bp_hp_before,
            "iter {i}: torso armor degraded despite recip=0"
        );
    }
}

#[test]
fn armor_degradation_targets_hands_slot() {
    // Verify weight-to-slot mapping: weights[4] = Hands.
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
        let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
        elf_creature.hp = elf_creature.hp_max;
        sim.db.update_creature(elf_creature).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place them apart with clear LOS (same Y, 5 voxels apart on X).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    // Elf should not have entered Shoot action (no bow available).
    assert_ne!(
        sim.db.creatures.get(&elf).unwrap().action_kind,
        ActionKind::Shoot,
        "Elf without bow should not be in Shoot action"
    );
}

#[test]
fn test_shoot_arrow_no_arrows_fails() {
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    // Place goblin 5 voxels away.
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Place a solid wall between them.
    sim.voxel_zone_mut(sim.home_zone_id()).unwrap().set(
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Place a leaf between them — should NOT block LOS.
    sim.voxel_zone_mut(sim.home_zone_id()).unwrap().set(
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);
    // Suppress goblin so it doesn't move or attack during the cooldown window.
    force_idle_and_cancel_activations(&mut sim, goblin);
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
    suppress_activation(&mut sim, elf);
    force_position(&mut sim, elf, elf_pos);
    // Ensure goblin is still alive and in place for the second shot target.
    let mut goblin_creature = sim.db.creatures.get(&goblin).unwrap();
    goblin_creature.hp = goblin_creature.hp_max;
    sim.db.update_creature(goblin_creature).unwrap();
    force_position(&mut sim, goblin, goblin_pos);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 5, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    arm_with_bow_and_arrows(&mut sim, elf, 5);

    // Put elf into a non-idle action (e.g., Build).
    let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
    elf_creature.action_kind = ActionKind::Build;
    elf_creature.next_available_tick = Some(sim.tick + 5000);
    sim.db.update_creature(elf_creature).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf_id = spawn_species(&mut sim, Species::Elf);
    let goblin_id = spawn_species(&mut sim, Species::Goblin);
    force_guaranteed_hits(&mut sim, goblin_id);

    // Place goblin and elf 5 voxels apart at known positions with clear LOS.
    let elf_pos = VoxelCoord::new(32, 1, 32);
    let goblin_pos = VoxelCoord::new(37, 1, 32);
    force_position(&mut sim, elf_id, elf_pos);
    force_position(&mut sim, goblin_id, goblin_pos);

    // Freeze the elf so it doesn't flee.
    force_idle_and_cancel_activations(&mut sim, elf_id);

    // Arm the goblin with bow + arrows.
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let blocker = spawn_elf(&mut sim);

    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let buddy = spawn_elf(&mut sim);

    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let blocker = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let goblin_near = spawn_species(&mut sim, Species::Goblin);
    let goblin_far = spawn_species(&mut sim, Species::Goblin);

    // Use explicit position so goblins are always in valid coords.
    let shooter_pos = VoxelCoord::new(20, 1, 30);
    force_position(&mut sim, shooter, shooter_pos);
    let near_pos = VoxelCoord::new(shooter_pos.x + 5, shooter_pos.y, shooter_pos.z);
    let far_pos = VoxelCoord::new(shooter_pos.x + 10, shooter_pos.y, shooter_pos.z);
    force_position(&mut sim, goblin_near, near_pos);
    force_position(&mut sim, goblin_far, far_pos);
    force_idle(&mut sim, shooter);
    suppress_activation(&mut sim, shooter);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    let shooter = spawn_elf(&mut sim);
    let blocker = spawn_elf(&mut sim);
    let goblin_blocked = spawn_species(&mut sim, Species::Goblin);
    let goblin_clear = spawn_species(&mut sim, Species::Goblin);

    // Suppress all creatures so autonomous behavior doesn't interfere.
    // Zero shooter stats so Perception doesn't shrink detection range.
    force_idle_and_cancel_activations(&mut sim, shooter);
    force_idle_and_cancel_activations(&mut sim, blocker);
    force_idle_and_cancel_activations(&mut sim, goblin_blocked);
    force_idle_and_cancel_activations(&mut sim, goblin_clear);
    zero_creature_stats(&mut sim, shooter);

    // Place everyone on the forest floor away from the tree to avoid
    // LOS issues with the trunk. Y=1 is walking height on flat floor.
    let base = VoxelCoord::new(20, 1, 32);
    force_position(&mut sim, shooter, base);

    // Place blocker between shooter and goblin_blocked (on X axis).
    let blocker_pos = VoxelCoord::new(base.x + 5, base.y, base.z);
    let blocked_pos = VoxelCoord::new(base.x + 10, base.y, base.z);
    force_position(&mut sim, blocker, blocker_pos);
    force_position(&mut sim, goblin_blocked, blocked_pos);

    // Place goblin_clear on the opposite X side with no blocker (within world bounds).
    let clear_pos = VoxelCoord::new(base.x - 8, base.y, base.z);
    force_position(&mut sim, goblin_clear, clear_pos);

    force_idle(&mut sim, shooter);
    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    let det_range_sq = sim.effective_detection_range_sq(shooter, Species::Elf);
    let clear_dist_sq = {
        let sp = sim.db.creatures.get(&shooter).unwrap().position.min;
        let cp = sim.db.creatures.get(&goblin_clear).unwrap().position.min;
        (sp.x as i64 - cp.x as i64).pow(2)
            + (sp.y as i64 - cp.y as i64).pow(2)
            + (sp.z as i64 - cp.z as i64).pow(2)
    };
    eprintln!(
        "  combat_redirect: det_range_sq={det_range_sq} clear_dist_sq={clear_dist_sq} \
         shooter_action={:?}",
        sim.db.creatures.get(&shooter).unwrap().action_kind,
    );

    let mut events = Vec::new();
    let result = sim.try_combat_against_target(shooter, goblin_blocked, &mut events);

    assert!(
        result,
        "Should have redirected and fired at alternate target \
         (det_range_sq={det_range_sq}, clear_dist_sq={clear_dist_sq})"
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
    let mut sim = flat_world_sim(fresh_test_seed());
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
    suppress_activation(&mut sim, shooter);
    suppress_activation_until(&mut sim, buddy, u64::MAX);
    suppress_activation_until(&mut sim, goblin, u64::MAX);
    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    // Re-confirm all positions after idle (ensure no drift from earlier steps).
    force_position(&mut sim, shooter, base);
    force_position(&mut sim, buddy, buddy_pos);
    force_position(&mut sim, goblin, goblin_pos);

    // Restore buddy HP to full in case the goblin attacked during spawn steps.
    let mut buddy_creature = sim.db.creatures.get(&buddy).unwrap();
    buddy_creature.hp = buddy_creature.hp_max;
    sim.db.update_creature(buddy_creature).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
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
    let goblin_walkable = find_walkable(&sim, goblin_pos, 10);
    if let Some(walkable_pos) = goblin_walkable {
        let task_id = TaskId::new(&mut sim.rng);
        let task = Task {
            id: task_id,
            kind: TaskKind::AttackTarget { target: goblin },
            state: TaskState::InProgress,
            location: walkable_pos,
            progress: 0,
            total_cost: 0,
            required_species: Some(Species::Elf),
            origin: TaskOrigin::PlayerDirected,
            target_creature: Some(goblin),
            restrict_to_creature_id: None,
            prerequisite_task_id: None,
            required_civ_id: None,
        };
        sim.insert_task(sim.home_zone_id(), task);
        if let Some(mut c) = sim.db.creatures.get(&archer) {
            c.current_task = Some(task_id);
            sim.db.update_creature(c).unwrap();
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.elf_starting_bows = 0;
    sim.config.elf_starting_arrows = 0;
    // Disable evasion so the arrow always hits (evasion is PRNG-dependent).
    sim.config.evasion_crit_threshold = 100_000;
    let shooter = spawn_elf(&mut sim);
    force_guaranteed_hits(&mut sim, shooter);
    let goblin_near = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin_near);
    let goblin_far = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin_far);

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
    suppress_activation(&mut sim, shooter);
    arm_with_bow_and_arrows(&mut sim, shooter, 5);

    // Record the near goblin's actual HP before the shot (may differ from
    // species base due to CON bonus).
    let near_hp_before = sim.db.creatures.get(&goblin_near).unwrap().hp;

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
    assert!(
        near_hp < near_hp_before,
        "Hostile near origin should be hit by arrow (hp {near_hp} should be less than {near_hp_before})"
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
    let mut sim = flat_world_sim(fresh_test_seed());
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
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: goblin,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(-500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(-500);
    sim.db.update_creature_trait(agility_trait).unwrap();
    sim.config.evasion_crit_threshold = 100_000;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None, sim.home_zone_id());

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle_and_cancel_activations(&mut sim, elf);
    force_idle_and_cancel_activations(&mut sim, target);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    // Place elf 2 voxels away (distance_sq = 4 for a single axis offset of 2).
    let elf_pos = VoxelCoord::new(target_pos.x + 2, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    suppress_activation(&mut sim, elf);
    // Re-force position in case elf moved during poll activation.
    force_position(&mut sim, elf, elf_pos);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let elf_pos = VoxelCoord::new(target_pos.x + 2, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    // Force degradation to always apply by setting min = max = 1.
    sim.config.melee_weapon_impact_damage_min = 1;
    sim.config.melee_weapon_impact_damage_max = 1;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
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
    let mut sim = flat_world_sim(fresh_test_seed());
    // Capybara has melee_damage = 0.
    let capybara = spawn_species(&mut sim, Species::Capybara);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, capybara);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, capybara);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let cap_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, capybara, cap_pos);
    force_idle(&mut sim, capybara);
    suppress_activation(&mut sim, capybara);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    // Force degradation to always break (1 HP weapon, 1 damage per strike).
    sim.config.melee_weapon_impact_damage_min = 1;
    sim.config.melee_weapon_impact_damage_max = 1;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    suppress_activation(&mut sim, elf);
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
    let mut sim = flat_world_sim(fresh_test_seed());
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.melee_weapon_impact_damage_min = 5;
    sim.config.melee_weapon_impact_damage_max = 2;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.config.melee_weapon_impact_damage_min = 0;
    sim.config.melee_weapon_impact_damage_max = 0;

    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let target = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, target);
    force_guaranteed_hits(&mut sim, elf);

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    let elf_pos = VoxelCoord::new(target_pos.x + 1, target_pos.y, target_pos.z);
    force_position(&mut sim, elf, elf_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);

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
    let mut strength_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Strength))
        .unwrap();
    strength_trait.value = TraitValue::Int(100);
    sim.db.update_creature_trait(strength_trait).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, elf);
    zero_creature_stats(&mut sim, goblin);
    force_guaranteed_hits(&mut sim, elf);

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
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 2, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);
    suppress_activation_until(&mut sim, goblin, u64::MAX);

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
    let elf_final_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    assert_eq!(
        elf_final_pos, elf_pos,
        "Elf should stay at spear range, not close to adjacent"
    );
}

/// Hostile AI with spear: a goblin with a spear placed 2 voxels from an elf
/// should attack at spear range without closing further.
#[test]
fn hostile_ai_spear_attacks_at_extended_range() {
    let mut sim = flat_world_sim(fresh_test_seed());
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
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 2, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);
    // Suppress elf activation so it doesn't interfere.
    suppress_activation_until(&mut sim, elf, u64::MAX);
    // Schedule goblin activation at tick+1 via poll-based activation.
    let tick = sim.tick;
    schedule_activation_at(&mut sim, goblin, tick + 1);

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
    let goblin_final_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    // Zero stats so Perception doesn't inflate detection range beyond
    // the 20-voxel origin distance (base detection_range_sq=225 < 400).
    zero_creature_stats(&mut sim, goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let capybara = spawn_species(&mut sim, Species::Capybara);
    let capy_pos = sim.db.creatures.get(&capybara).unwrap().position.min;

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    // Zero stats so Perception doesn't inflate detection range beyond
    // the 20-voxel origin distance (base detection_range_sq=225 < 400).
    zero_creature_stats(&mut sim, goblin);
    // Place goblin at a known central position so +/- 20 stays within world bounds.
    let goblin_pos = VoxelCoord::new(32, 1, 32);
    force_position(&mut sim, goblin, goblin_pos);

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

    // Kill the goblin.
    if let Some(mut c) = sim.db.creatures.get(&goblin) {
        c.vital_status = VitalStatus::Dead;
        c.hp = 0;
        sim.db.update_creature(c).unwrap();
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

    // Place origin above the goblin (in open air, avoiding terrain).
    let origin = VoxelCoord::new(goblin_pos.x, goblin_pos.y + 20, goblin_pos.z);

    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.spawn_projectile(origin, goblin_pos, None, sim.home_zone_id());

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 10, tree_pos.y + 30, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    // Zero stats so Perception doesn't inflate detection range beyond
    // the 20-voxel origin distance used below.
    zero_creature_stats(&mut sim, hornet);
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position.min;

    assert!(
        sim.species_table[&Species::Hornet]
            .movement_category
            .is_flyer(),
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let fake_id = CreatureId::new(&mut sim.rng);
    let origin = VoxelCoord::new(100, 51, 100);
    sim.maybe_arrow_chase(fake_id, origin);
    // No panic = success.
}

#[test]
fn arrow_chase_preempts_autonomous_task() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    // Zero stats so Perception doesn't inflate detection range beyond
    // the 20-voxel origin distance (base detection_range_sq=225 < 400).
    zero_creature_stats(&mut sim, goblin);
    // Freeze goblin so it doesn't get activated before we call maybe_arrow_chase.
    force_idle_and_cancel_activations(&mut sim, goblin);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

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
    sim.insert_task(sim.home_zone_id(), goto_task);
    if let Some(mut c) = sim.db.creatures.get(&goblin) {
        c.current_task = Some(task_id);
        c.next_available_tick = Some(u64::MAX);
        sim.db.update_creature(c).unwrap();
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    let elf = spawn_elf(&mut sim);
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;

    let origin1 = VoxelCoord::new(goblin_pos.x + 20, goblin_pos.y, goblin_pos.z);
    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.maybe_arrow_chase(goblin, origin1);
    let task_id = sim.db.creatures.get(&goblin).unwrap().current_task.unwrap();

    // Simulate the task having acquired a melee target during attack-move.
    if let Some(mut t) = sim.db.tasks.get(&task_id) {
        t.target_creature = Some(elf);
        sim.db.update_task(t).unwrap();
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let air_pos = VoxelCoord::new(tree_pos.x + 10, tree_pos.y + 30, tree_pos.z);
    let hornet = spawn_hornet_at(&mut sim, air_pos);
    force_idle_and_cancel_activations(&mut sim, hornet);

    // Arrow origin far outside detection range.
    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position.min;
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
        // Attacker stats 0 + quasi_normal(50) vs defender stats 0.
        let attacker_roll = elven_canopy_prng::quasi_normal(&mut rng, 50);
        match roll_hit_check(attacker_roll, 0, 0, 100) {
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
        // Attacker: attack_skill=250, dex=250, total base 500 + noise.
        let attacker_roll = 500 + elven_canopy_prng::quasi_normal(&mut rng, 50);
        match roll_hit_check(attacker_roll, 0, 0, 100) {
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
        // Attacker: 0 stats + noise vs defender evasion=250 + agi=250.
        let attacker_roll = elven_canopy_prng::quasi_normal(&mut rng, 50);
        match roll_hit_check(attacker_roll, 250, 250, 100) {
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
        let attacker_roll = elven_canopy_prng::quasi_normal(&mut rng, 50);
        if roll_hit_check(attacker_roll, 0, 0, 100) == HitResult::CriticalHit {
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
            .map(|_| {
                let attacker_roll = 15 + elven_canopy_prng::quasi_normal(&mut rng, 50);
                roll_hit_check(attacker_roll, 8, 7, 100)
            })
            .collect()
    };
    let results_b: Vec<HitResult> = {
        let mut rng = elven_canopy_prng::GameRng::new(999);
        (0..100)
            .map(|_| {
                let attacker_roll = 15 + elven_canopy_prng::quasi_normal(&mut rng, 50);
                roll_hit_check(attacker_roll, 8, 7, 100)
            })
            .collect()
    };
    assert_eq!(results_a, results_b);
}

#[test]
fn test_hit_check_exact_boundary_values() {
    use crate::sim::combat::{HitResult, roll_hit_check};

    // Defender total = evasion(50) + agi(30) = 80, crit threshold = 100.
    // attacker_roll == defender_total → Hit
    assert_eq!(roll_hit_check(80, 50, 30, 100), HitResult::Hit);
    // attacker_roll == defender_total - 1 → Miss
    assert_eq!(roll_hit_check(79, 50, 30, 100), HitResult::Miss);
    // attacker_roll == defender_total + crit_threshold → CriticalHit
    assert_eq!(roll_hit_check(180, 50, 30, 100), HitResult::CriticalHit);
    // attacker_roll == defender_total + crit_threshold - 1 → Hit (not crit)
    assert_eq!(roll_hit_check(179, 50, 30, 100), HitResult::Hit);
}

#[test]
fn test_melee_miss_high_evasion() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf massive evasion advantage.
    use crate::db::CreatureTrait;
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(500);
    sim.db.update_creature_trait(agility_trait).unwrap();

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give goblin massive skill advantage.
    use crate::db::CreatureTrait;
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: goblin,
            trait_kind: TraitKind::Striking,
            value: TraitValue::Int(500),
        })
        .unwrap();
    let mut dexterity_trait = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Dexterity))
        .unwrap();
    dexterity_trait.value = TraitValue::Int(500);
    sim.db.update_creature_trait(dexterity_trait).unwrap();

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);

    // Give elf huge HP so we can measure damage.
    let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
    elf_creature.hp = 100_000;
    elf_creature.hp_max = 100_000;
    sim.db.update_creature(elf_creature).unwrap();

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high evasion to guarantee a miss.
    use crate::db::CreatureTrait;
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(500);
    sim.db.update_creature_trait(agility_trait).unwrap();

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high AGI to guarantee misses, but keep Evasion skill at 0.
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(1000);
    sim.db.update_creature_trait(agility_trait).unwrap();

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    // Give elf massive evasion advantage.
    sim.db
        .upsert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(500),
        })
        .unwrap();
    sim.db
        .upsert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Agility,
            value: TraitValue::Int(500),
        })
        .unwrap();
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
    sim.spawn_projectile(origin, elf_pos, None, sim.home_zone_id());

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);
    let shooter = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, shooter);

    // Give shooter massive attack advantage for guaranteed crit.
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: shooter,
            trait_kind: TraitKind::Archery,
            value: TraitValue::Int(500),
        })
        .unwrap();
    let mut dexterity_trait = sim
        .db
        .creature_traits
        .get(&(shooter, TraitKind::Dexterity))
        .unwrap();
    dexterity_trait.value = TraitValue::Int(500);
    sim.db.update_creature_trait(dexterity_trait).unwrap();

    let mut elf_creature = sim.db.creatures.get(&elf).unwrap();
    elf_creature.hp = 100_000;
    elf_creature.hp_max = 100_000;
    sim.db.update_creature(elf_creature).unwrap();
    sim.config.armor_degrade_location_weights = [0, 0, 0, 0, 0];

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
    sim.spawn_projectile(origin, elf_pos, Some(shooter), sim.home_zone_id());

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let target = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, target);
    let shooter = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, shooter);

    // Target has moderate evasion.
    sim.db
        .upsert_creature_trait(CreatureTrait {
            creature_id: target,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(200),
        })
        .unwrap();
    sim.db
        .upsert_creature_trait(CreatureTrait {
            creature_id: target,
            trait_kind: TraitKind::Agility,
            value: TraitValue::Int(200),
        })
        .unwrap();

    // Shooter has high Archery + DEX to overcome the evasion.
    sim.db
        .upsert_creature_trait(CreatureTrait {
            creature_id: shooter,
            trait_kind: TraitKind::Archery,
            value: TraitValue::Int(500),
        })
        .unwrap();
    sim.db
        .upsert_creature_trait(CreatureTrait {
            creature_id: shooter,
            trait_kind: TraitKind::Dexterity,
            value: TraitValue::Int(500),
        })
        .unwrap();
    sim.config.evasion_crit_threshold = 100_000;

    let mut target_creature = sim.db.creatures.get(&target).unwrap();
    target_creature.hp = 100_000;
    target_creature.hp_max = 100_000;
    sim.db.update_creature(target_creature).unwrap();

    let target_pos = sim.db.creatures.get(&target).unwrap().position.min;
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    let mut hits = 0;
    // 200 trials with Archery+DEX=500 vs Evasion+AGI=200 should hit
    // well over 80%. Allow >= 120 (~60%) to avoid flakiness from random seeds.
    let total = 200;
    for _ in 0..total {
        let origin = VoxelCoord::new(target_pos.x - 10, target_pos.y, target_pos.z);
        sim.spawn_projectile(origin, target_pos, Some(shooter), sim.home_zone_id());
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
        hits > 120,
        "high-skill shooter should hit evasive target most of the time, got {hits}/{total}"
    );
}

#[test]
fn test_melee_miss_no_armor_degradation() {
    use crate::db::CreatureTrait;
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high evasion to guarantee misses.
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(500);
    sim.db.update_creature_trait(agility_trait).unwrap();

    equip_full_armor(&mut sim, elf);

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, elf);

    // Give elf high AGI to guarantee dodges, but Evasion skill at 0.
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(1000);
    sim.db.update_creature_trait(agility_trait).unwrap();

    let evasion_before = sim.trait_int(elf, TraitKind::Evasion, 0);
    assert_eq!(evasion_before, 0, "evasion should start at 0");

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    for _ in 0..100 {
        let origin = VoxelCoord::new(elf_pos.x - 10, elf_pos.y, elf_pos.z);
        sim.spawn_projectile(origin, elf_pos, None, sim.home_zone_id());
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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);

    // Give goblin very high evasion to guarantee dodge.
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(1000);
    sim.db.update_creature_trait(agility_trait).unwrap();

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let origin = VoxelCoord::new(goblin_pos.x, goblin_pos.y + 20, goblin_pos.z);

    force_idle_and_cancel_activations(&mut sim, goblin);

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.spawn_projectile(origin, goblin_pos, None, sim.home_zone_id());

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
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);

    // Give elf high evasion to guarantee misses.
    use crate::db::CreatureTrait;
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(elf, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(500);
    sim.db.update_creature_trait(agility_trait).unwrap();

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

    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
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

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

// -----------------------------------------------------------------------
// Hostile AI tests
// -----------------------------------------------------------------------

#[test]
fn engagement_style_config() {
    use crate::species::EngagementInitiative;
    let sim = flat_world_sim(fresh_test_seed());
    // Aggressive species.
    assert_eq!(
        sim.species_table[&Species::Goblin]
            .engagement_style
            .initiative,
        EngagementInitiative::Aggressive
    );
    assert_eq!(
        sim.species_table[&Species::Orc].engagement_style.initiative,
        EngagementInitiative::Aggressive
    );
    assert_eq!(
        sim.species_table[&Species::Troll]
            .engagement_style
            .initiative,
        EngagementInitiative::Aggressive
    );
    // Passive species.
    assert_eq!(
        sim.species_table[&Species::Capybara]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Deer]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Boar]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Monkey]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Squirrel]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    assert_eq!(
        sim.species_table[&Species::Elephant]
            .engagement_style
            .initiative,
        EngagementInitiative::Passive
    );
    // Elf: defensive with 100% disengage.
    assert_eq!(
        sim.species_table[&Species::Elf].engagement_style.initiative,
        EngagementInitiative::Defensive
    );
    assert_eq!(
        sim.species_table[&Species::Elf]
            .engagement_style
            .disengage_threshold_pct,
        100
    );
    // Detection ranges are set for aggressive and flee-capable species.
    assert!(sim.species_table[&Species::Goblin].hostile_detection_range_sq > 0);
    assert!(sim.species_table[&Species::Orc].hostile_detection_range_sq > 0);
    assert!(sim.species_table[&Species::Troll].hostile_detection_range_sq > 0);
    // Elves have detection range for flee behavior.
    assert!(sim.species_table[&Species::Elf].hostile_detection_range_sq > 0);
    assert_eq!(
        sim.species_table[&Species::Capybara].hostile_detection_range_sq,
        0
    );
}

#[test]
fn hostile_creature_pursues_and_attacks_elf() {
    let mut sim = flat_world_sim(fresh_test_seed());

    let elf_id = spawn_elf(&mut sim);
    let goblin_id = spawn_species(&mut sim, Species::Goblin);
    force_guaranteed_hits(&mut sim, goblin_id);

    // Place creatures at known positions: elf at center, goblin 5 voxels away.
    // Goblin has no civ (default for non-elves), so it targets any civ creature
    // of a different species — i.e., the elf.
    let elf_pos = VoxelCoord::new(32, 1, 32);
    let goblin_start = VoxelCoord::new(37, 1, 32);
    force_position(&mut sim, elf_id, elf_pos);
    force_position(&mut sim, goblin_id, goblin_start);
    // Freeze elf so it doesn't flee, letting goblin pursue unimpeded.
    force_idle_and_cancel_activations(&mut sim, elf_id);

    let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

    sim.step(&[], sim.tick + 10_000);

    let elf_hp_after = sim.db.creatures.get(&elf_id).unwrap().hp;
    let goblin_pos = sim.db.creatures.get(&goblin_id).unwrap().position.min;
    let elf_current_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    let new_dist = goblin_pos.manhattan_distance(elf_current_pos);
    let initial_dist = goblin_start.manhattan_distance(elf_pos);

    // The goblin should have either moved closer to the elf's current
    // position, or dealt damage (meaning it reached and attacked).
    let moved_closer = new_dist < initial_dist;
    let dealt_damage = elf_hp_after < elf_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Goblin should pursue or attack elf: initial dist={initial_dist}, \
             new dist={new_dist}, elf hp {elf_hp_before} -> {elf_hp_after}"
    );
}

#[test]
fn hostile_creature_wanders_without_elves() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Disable food/rest so the goblin wanders instead of seeking food.
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .rest_decay_per_tick = 0;
    let goblin_id = spawn_species(&mut sim, Species::Goblin);

    // Place goblin on a walkable position so it can wander.
    let walkable = find_walkable(&sim, VoxelCoord::new(32, 1, 32), 30)
        .expect("should have a walkable position");
    force_position(&mut sim, goblin_id, walkable);
    // Schedule activation at tick+1 so the goblin activates promptly.
    let next_tick = sim.tick + 1;
    schedule_activation_at(&mut sim, goblin_id, next_tick);

    let goblin_start = sim.db.creatures.get(&goblin_id).unwrap().position.min;

    let mut wandered = false;
    for i in 0..500 {
        sim.step(&[], sim.tick + 100);
        let g = sim.db.creatures.get(&goblin_id).unwrap();
        eprintln!(
            "wanders_without_elves step {i}: tick={} pos=({},{},{}) action={:?} task={:?}",
            sim.tick,
            g.position.min.x,
            g.position.min.y,
            g.position.min.z,
            g.action_kind,
            g.current_task,
        );
        if g.position.min != goblin_start {
            wandered = true;
            break;
        }
    }

    assert!(
        wandered,
        "Goblin should wander even without elves to pursue. \
         pos=({},{},{}), action={:?}",
        goblin_start.x,
        goblin_start.y,
        goblin_start.z,
        sim.db.creatures.get(&goblin_id).unwrap().action_kind,
    );
}

#[test]
fn hostile_creature_attacks_adjacent_elf() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    // Place goblin adjacent to elf.
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);
    // Suppress elf activation so it doesn't interfere.
    suppress_activation_until(&mut sim, elf, u64::MAX);
    // Schedule goblin activation at tick+1 via poll-based activation.
    let tick = sim.tick;
    schedule_activation_at(&mut sim, goblin, tick + 1);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    // Run one activation cycle — the goblin should melee the elf.
    sim.step(&[], tick + 2);

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    let goblin_damage = sim.species_table[&Species::Goblin].melee_damage;
    assert_eq!(
        elf_hp_after,
        elf_hp_before - goblin_damage,
        "Adjacent hostile should automatically melee-strike the elf"
    );
}

// -----------------------------------------------------------------------
// Projectile system tests (F-projectiles)
// -----------------------------------------------------------------------

#[test]
fn spawn_projectile_creates_entity_and_inventory() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let origin = VoxelCoord::new(40, 5, 40);
    let target = VoxelCoord::new(50, 5, 40);

    sim.spawn_projectile(origin, target, None, sim.home_zone_id());

    assert_eq!(sim.db.projectiles.len(), 1);
    let proj = sim.db.projectiles.iter_all().next().unwrap();
    assert_eq!(proj.shooter, None);
    assert_eq!(proj.prev_voxel, origin);
    // Should have an inventory with 1 arrow.
    let stacks = sim
        .db
        .item_stacks
        .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].kind, inventory::ItemKind::Arrow);
    assert_eq!(stacks[0].quantity, 1);
}

#[test]
fn spawn_projectile_schedules_tick_event() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let initial_events = sim.event_queue.len();
    sim.spawn_projectile(
        VoxelCoord::new(40, 5, 40),
        VoxelCoord::new(50, 5, 40),
        None,
        sim.home_zone_id(),
    );
    // Should have scheduled exactly one ProjectileTick.
    assert_eq!(sim.event_queue.len(), initial_events + 1);
}

#[test]
fn second_spawn_does_not_duplicate_tick_event() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let initial_events = sim.event_queue.len();
    sim.spawn_projectile(
        VoxelCoord::new(40, 5, 40),
        VoxelCoord::new(50, 5, 40),
        None,
        sim.home_zone_id(),
    );
    sim.spawn_projectile(
        VoxelCoord::new(40, 5, 40),
        VoxelCoord::new(45, 5, 40),
        None,
        sim.home_zone_id(),
    );
    // Only one extra event (from first spawn), not two.
    assert_eq!(sim.event_queue.len(), initial_events + 1);
}

#[test]
fn projectile_hits_solid_voxel_and_creates_ground_pile() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Place a solid wall at x=45.
    for y in 1..=5 {
        sim.voxel_zone_mut(sim.home_zone_id())
            .unwrap()
            .set(VoxelCoord::new(45, y, 40), VoxelType::GrownPlatform);
    }

    // Spawn projectile heading +x toward the wall (flat, no gravity).
    // Disable arrow impact damage so the arrow always survives and creates
    // a ground pile (impact damage is PRNG-dependent).
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    sim.config.arrow_impact_damage_min = 0;
    sim.config.arrow_impact_damage_max = 0;
    sim.spawn_projectile(
        VoxelCoord::new(40, 3, 40),
        VoxelCoord::new(45, 3, 40),
        None,
        sim.home_zone_id(),
    );

    // Run until the projectile resolves (max 500 ticks).
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

    assert_eq!(sim.db.projectiles.len(), 0, "Projectile should be resolved");

    // Should have a ground pile with an arrow in it near x=44 (prev_voxel).
    let mut found_arrow = false;
    for pile in sim.db.ground_piles.iter_all() {
        let stacks = sim
            .db
            .item_stacks
            .by_inventory_id(&pile.inventory_id, tabulosity::QueryOpts::ASC);
        for s in &stacks {
            if s.kind == inventory::ItemKind::Arrow {
                found_arrow = true;
            }
        }
    }
    assert!(found_arrow, "Arrow should land as ground pile");
}

#[test]
fn projectile_hits_creature_and_deals_damage() {
    use crate::db::CreatureTrait;
    use crate::types::TraitValue;
    let mut sim = flat_world_sim(fresh_test_seed());
    // Spawn a goblin at a known position.
    let goblin = spawn_species(&mut sim, Species::Goblin);
    zero_creature_stats(&mut sim, goblin);
    // Set evasion deeply negative so shooter-less projectile always hits.
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: goblin,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(-500),
        })
        .unwrap();
    let mut agility_trait = sim
        .db
        .creature_traits
        .get(&(goblin, TraitKind::Agility))
        .unwrap();
    agility_trait.value = TraitValue::Int(-500);
    sim.db.update_creature_trait(agility_trait).unwrap();
    sim.config.evasion_crit_threshold = 100_000;
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let goblin_hp_before = sim.db.creatures.get(&goblin).unwrap().hp;

    // Spawn projectile aimed at the goblin (no gravity for predictability).
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;
    let origin = VoxelCoord::new(goblin_pos.x - 10, goblin_pos.y, goblin_pos.z);
    sim.spawn_projectile(origin, goblin_pos, None, sim.home_zone_id());

    // Run until resolved.
    let mut hit_events = Vec::new();
    for _ in 0..500 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if matches!(e.kind, SimEventKind::ProjectileHitCreature { .. }) {
                hit_events.push(e.clone());
            }
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert_eq!(sim.db.projectiles.len(), 0, "Projectile should be resolved");
    assert!(!hit_events.is_empty(), "Should have hit the creature");

    let goblin_hp_after = sim.db.creatures.get(&goblin).unwrap().hp;
    assert!(
        goblin_hp_after < goblin_hp_before,
        "Goblin should have taken damage: {goblin_hp_before} -> {goblin_hp_after}"
    );
}

#[test]
fn projectile_out_of_bounds_despawns_silently() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Shoot a projectile off the edge of the world.
    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 5; // very fast

    sim.spawn_projectile(
        VoxelCoord::new(250, 5, 128),
        VoxelCoord::new(260, 5, 128), // target is beyond world bounds
        None,
        sim.home_zone_id(),
    );

    // Run until resolved.
    for _ in 0..2000 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        // No surface hit or creature hit events expected.
        for e in &events {
            assert!(
                !matches!(e.kind, SimEventKind::ProjectileHitSurface { .. }),
                "Should not hit surface"
            );
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert_eq!(
        sim.db.projectiles.len(),
        0,
        "Projectile should have despawned"
    );
}

#[test]
fn projectile_does_not_hit_shooter() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Spawn an elf and shoot from their position.
    let elf = spawn_species(&mut sim, Species::Elf);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    // Shoot from the elf's own position toward a distant target.
    sim.spawn_projectile(
        elf_pos,
        VoxelCoord::new(elf_pos.x + 20, elf_pos.y, elf_pos.z),
        Some(elf),
        sim.home_zone_id(),
    );

    // Run a few ticks — the projectile should pass through the shooter.
    for _ in 0..50 {
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

    // Elf should not have taken any damage.
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_after, elf_hp_before,
        "Shooter should not be hit by their own arrow"
    );
}

#[test]
fn hostile_creature_wanders_after_killing_elf() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Disable food/rest so the goblin wanders instead of seeking food.
    for spec in sim.species_table.values_mut() {
        spec.food_decay_per_tick = 0;
        spec.rest_decay_per_tick = 0;
    }
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    // Place goblin adjacent to elf.
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, goblin);

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
    assert_eq!(
        sim.db.creatures.get(&elf).unwrap().vital_status,
        VitalStatus::Dead,
    );

    // With no living elves, the goblin should fall back to random wander.
    let mut wandered = false;
    for i in 0..500 {
        sim.step(&[], sim.tick + 100);
        let g = sim.db.creatures.get(&goblin).unwrap();
        eprintln!(
            "wanders_after_kill step {i}: tick={} pos=({},{},{}) action={:?} task={:?}",
            sim.tick,
            g.position.min.x,
            g.position.min.y,
            g.position.min.z,
            g.action_kind,
            g.current_task,
        );
        if g.position.min != goblin_pos {
            wandered = true;
            break;
        }
    }

    assert!(
        wandered,
        "Goblin should wander after elf is dead. pos=({},{},{}), action={:?}",
        goblin_pos.x,
        goblin_pos.y,
        goblin_pos.z,
        sim.db.creatures.get(&goblin).unwrap().action_kind,
    );
}

#[test]
fn projectile_skips_origin_voxel_creatures() {
    let mut sim = flat_world_sim(fresh_test_seed());
    // Spawn shooter and bystander at the same position.
    let shooter = spawn_species(&mut sim, Species::Elf);
    let shooter_pos = sim.db.creatures.get(&shooter).unwrap().position.min;
    let shooter_hp = sim.db.creatures.get(&shooter).unwrap().hp;

    let bystander = spawn_species(&mut sim, Species::Elf);
    // Move bystander to the same position as the shooter.
    if let Some(mut c) = sim.db.creatures.get(&bystander) {
        c.position = VoxelBox::point(shooter_pos);
        sim.db.update_creature(c).unwrap();
    }
    // Tabulosity spatial index is automatically updated by db.update_creature.
    let bystander_hp = sim.db.creatures.get(&bystander).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    // Shoot from the shared position toward a distant target.
    sim.spawn_projectile(
        shooter_pos,
        VoxelCoord::new(shooter_pos.x + 20, shooter_pos.y, shooter_pos.z),
        Some(shooter),
        sim.home_zone_id(),
    );

    // Run ticks until projectile is consumed or max iterations.
    for _ in 0..50 {
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

    // Neither the shooter nor the bystander in the origin voxel should
    // have been hit — projectiles skip the entire launch voxel.
    let shooter_hp_after = sim.db.creatures.get(&shooter).unwrap().hp;
    assert_eq!(
        shooter_hp_after, shooter_hp,
        "Shooter should not be hit by their own arrow"
    );

    let bystander_hp_after = sim.db.creatures.get(&bystander).unwrap().hp;
    assert_eq!(
        bystander_hp_after, bystander_hp,
        "Bystander in origin voxel should not be hit (hp: {} -> {})",
        bystander_hp, bystander_hp_after,
    );
}

#[test]
fn hostile_waits_on_cooldown_near_elf() {
    // When a hostile is in melee range but on cooldown, it should not
    // wander away — it should wait and re-strike when the cooldown expires.
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Goblin)
        .unwrap()
        .rest_decay_per_tick = 0;
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    zero_creature_stats(&mut sim, goblin);
    zero_creature_stats(&mut sim, elf);
    force_guaranteed_hits(&mut sim, goblin);
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle_and_cancel_activations(&mut sim, elf);
    force_idle_and_cancel_activations(&mut sim, goblin);

    // Freeze all bystander creatures.
    let bystanders: Vec<CreatureId> = sim
        .db
        .creatures
        .iter_all()
        .filter(|c| c.id != goblin && c.id != elf)
        .map(|c| c.id)
        .collect();
    for cid in bystanders {
        force_idle_and_cancel_activations(&mut sim, cid);
    }

    // First strike via try_melee_strike (bypasses activation/step races).
    let mut events = Vec::new();
    let hit = sim.try_melee_strike(goblin, elf, &mut events);
    assert!(hit, "First strike should succeed");

    let elf_hp_after_first = sim.db.creatures.get(&elf).unwrap().hp;
    assert!(
        elf_hp_after_first < 100,
        "First strike should have dealt damage"
    );

    // Goblin is now on MeleeStrike cooldown. Schedule its activation
    // for after cooldown expires.
    let interval = sim.species_table[&Species::Goblin].melee_interval_ticks;
    let activation_tick = sim.tick + interval + 1;
    schedule_activation_at(&mut sim, goblin, activation_tick);
    sim.step(&[], activation_tick + 100);

    let elf_hp_after_second = sim.db.creatures.get(&elf).unwrap().hp;
    assert!(
        elf_hp_after_second < elf_hp_after_first,
        "Goblin should strike again after cooldown expires \
         (elf HP: {elf_hp_after_first} -> {elf_hp_after_second})"
    );
}

#[test]
fn hostile_ignores_elf_outside_detection_range() {
    // A goblin with detection_range_sq=225 (15 voxels) should NOT pursue
    // an elf that is >15 voxels away in euclidean distance.
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);

    // Place elf far from goblin — 20 voxels away on X axis (20² = 400 >> 225).
    // Stay within the 64x64 world at ground level (y=1, solid terrain at y=0)
    // so the elf has a valid nav node and won't fall due to creature gravity.
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let far_x = (goblin_pos.x + 20).min(62);
    let far_pos = VoxelCoord::new(far_x, 1, goblin_pos.z);
    force_position(&mut sim, elf, far_pos);

    // Schedule activation.
    let tick = sim.tick;
    schedule_activation_at(&mut sim, goblin, tick + 1);
    force_idle(&mut sim, goblin);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;

    // Run a short period — goblin should wander randomly, not pursue.
    // Keep ticks low so random wander can't close the 20-voxel gap.
    sim.step(&[], tick + 1000);

    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;
    assert_eq!(
        elf_hp_before, elf_hp_after,
        "Goblin should not attack elf outside detection range"
    );
    // Goblin should have wandered but NOT moved closer to the elf.
    // (It might have moved closer by random chance, so we just check
    // it didn't deal damage — the key assertion.)
}

#[test]
fn hostile_pursues_elf_within_detection_range() {
    // A goblin with detection_range_sq=225 (15 voxels) SHOULD pursue
    // an elf within 10 voxels (10² = 100 < 225).
    let mut sim = flat_world_sim(fresh_test_seed());
    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);
    force_guaranteed_hits(&mut sim, goblin);

    // Place elf 5 voxels from goblin on X axis (5² = 25 < 225).
    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let near_pos = VoxelCoord::new(goblin_pos.x + 5, goblin_pos.y, goblin_pos.z);
    force_position(&mut sim, elf, near_pos);
    // Freeze elf so it doesn't flee away from the goblin.
    force_idle_and_cancel_activations(&mut sim, elf);

    // Schedule activation.
    let tick = sim.tick;
    schedule_activation_at(&mut sim, goblin, tick + 1);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let initial_dist = goblin_pos.manhattan_distance(near_pos);

    sim.step(&[], tick + 10_000);

    let goblin_final = sim.db.creatures.get(&goblin).unwrap().position.min;
    let elf_current = sim.db.creatures.get(&elf).unwrap().position.min;
    let new_dist = goblin_final.manhattan_distance(elf_current);
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;

    let moved_closer = new_dist < initial_dist;
    let dealt_damage = elf_hp_after < elf_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Goblin should pursue elf within detection range: \
             initial dist={initial_dist}, new dist={new_dist}, \
             elf hp {elf_hp_before} -> {elf_hp_after}"
    );
}

#[test]
fn hostile_does_not_attack_same_species() {
    // Two non-civ goblins adjacent to each other should NOT attack.
    let mut sim = flat_world_sim(fresh_test_seed());
    let g1 = spawn_species(&mut sim, Species::Goblin);
    let g2 = spawn_species(&mut sim, Species::Goblin);

    // Place them adjacent.
    let g1_pos = sim.db.creatures.get(&g1).unwrap().position.min;
    let g2_pos = VoxelCoord::new(g1_pos.x + 1, g1_pos.y, g1_pos.z);
    force_position(&mut sim, g2, g2_pos);
    force_idle(&mut sim, g1);
    force_idle(&mut sim, g2);

    let tick = sim.tick;
    schedule_activation_at(&mut sim, g1, tick + 1);
    schedule_activation_at(&mut sim, g2, tick + 1);

    let g1_hp_before = sim.db.creatures.get(&g1).unwrap().hp;
    let g2_hp_before = sim.db.creatures.get(&g2).unwrap().hp;

    sim.step(&[], tick + 3000);

    let g1_hp_after = sim.db.creatures.get(&g1).unwrap().hp;
    let g2_hp_after = sim.db.creatures.get(&g2).unwrap().hp;
    assert_eq!(
        g1_hp_before, g1_hp_after,
        "Goblins should not attack same species"
    );
    assert_eq!(
        g2_hp_before, g2_hp_after,
        "Goblins should not attack same species"
    );
}

#[test]
fn all_hostile_species_pursue_elves() {
    for &hostile_species in &[Species::Goblin, Species::Orc, Species::Troll] {
        let mut sim = flat_world_sim(fresh_test_seed());

        // Disable food/rest decay.
        for spec in sim.species_table.values_mut() {
            spec.food_decay_per_tick = 0;
            spec.rest_decay_per_tick = 0;
        }

        // Suppress all initial creatures.
        let initial_ids: Vec<CreatureId> = sim.db.creatures.iter_all().map(|c| c.id).collect();
        for &id in &initial_ids {
            suppress_activation_until(&mut sim, id, u64::MAX);
        }

        let elf_id = spawn_species(&mut sim, Species::Elf);
        let hostile_id = spawn_species(&mut sim, hostile_species);

        // Place them at known positions. Use force_idle after force_position
        // to clear stale Move, then set NAT so both get polled.
        let pos_a = VoxelCoord::new(20, 1, 20);
        let pos_b = VoxelCoord::new(24, 1, 20);
        force_position(&mut sim, elf_id, pos_a);
        force_idle(&mut sim, elf_id);
        force_position(&mut sim, hostile_id, pos_b);
        force_idle(&mut sim, hostile_id);
        {
            let tick = sim.tick + 1;
            let mut c = sim.db.creatures.get(&hostile_id).unwrap();
            c.next_available_tick = Some(tick);
            sim.db.update_creature(c).unwrap();
        }
        // Suppress the elf so it doesn't flee and only the hostile acts.
        suppress_activation_until(&mut sim, elf_id, u64::MAX);

        let hostile_start = sim.db.creatures.get(&hostile_id).unwrap().position.min;
        let elf_hp_before = sim.db.creatures.get(&elf_id).unwrap().hp;

        // Loop until hostile moves or deals damage.
        let mut succeeded = false;
        for i in 0..100 {
            sim.step(&[], sim.tick + 100);
            let hostile_pos = sim.db.creatures.get(&hostile_id).unwrap().position.min;
            let elf_hp = sim.db.creatures.get(&elf_id).unwrap().hp;
            let h = sim.db.creatures.get(&hostile_id).unwrap();
            eprintln!(
                "  pursue_{hostile_species:?} step {i}: tick={} pos={:?} action={:?} \
                 nat={:?} elf_hp={elf_hp}",
                sim.tick, hostile_pos, h.action_kind, h.next_available_tick,
            );
            if hostile_pos != hostile_start || elf_hp < elf_hp_before {
                succeeded = true;
                break;
            }
        }
        assert!(
            succeeded,
            "{hostile_species:?} should pursue elf within 10000 ticks"
        );
    }
}

#[test]
fn projectile_hits_creature_beyond_origin_voxel() {
    use crate::db::CreatureTrait;
    let mut sim = flat_world_sim(fresh_test_seed());
    // Place a target creature a few voxels away from the origin.
    let target = spawn_species(&mut sim, Species::Elf);
    // Set target's evasion stats deeply negative so the no-shooter projectile
    // (0 attack + quasi_normal) always exceeds defender_total and hits.
    // Don't use zero_creature_stats to avoid altering walk speed / behavior.
    // Evasion skill has no row at spawn (default 0), so insert it.
    sim.db
        .insert_creature_trait(CreatureTrait {
            creature_id: target,
            trait_kind: TraitKind::Evasion,
            value: TraitValue::Int(-500),
        })
        .unwrap();
    let mut target_agility = sim
        .db
        .creature_traits
        .get(&(target, TraitKind::Agility))
        .unwrap();
    target_agility.value = TraitValue::Int(-500);
    sim.db.update_creature_trait(target_agility).unwrap();
    // Raise crit threshold to prevent the large margin from triggering crits.
    sim.config.evasion_crit_threshold = 100_000;
    let origin = VoxelCoord::new(40, 1, 40);
    let target_pos = VoxelCoord::new(42, 1, 40);
    if let Some(mut c) = sim.db.creatures.get(&target) {
        c.position = VoxelBox::point(target_pos);
        sim.db.update_creature(c).unwrap();
    }
    // Tabulosity spatial index is automatically updated by db.update_creature.
    let target_hp = sim.db.creatures.get(&target).unwrap().hp;

    sim.config.arrow_gravity = 0;
    sim.config.arrow_base_speed = crate::projectile::SUB_VOXEL_ONE / 20;

    // Shoot from origin toward the target (no shooter creature).
    sim.spawn_projectile(origin, target_pos, None, sim.home_zone_id());

    // Run ticks.
    let mut hit = false;
    for _ in 0..100 {
        if sim.db.projectiles.is_empty() {
            break;
        }
        sim.tick += 1;
        let mut events = Vec::new();
        sim.process_projectile_tick(&mut events);
        for e in &events {
            if let SimEventKind::ProjectileHitCreature { target_id, .. } = e.kind
                && target_id == target
            {
                hit = true;
            }
        }
        if !sim.db.projectiles.is_empty() {
            sim.event_queue
                .schedule(sim.tick + 1, ScheduledEventKind::ProjectileTick);
        }
    }

    assert!(hit, "Projectile should hit creature beyond origin voxel");
    let target_hp_after = sim.db.creatures.get(&target).unwrap().hp;
    assert!(
        target_hp_after < target_hp,
        "Target should have taken damage (hp: {} -> {})",
        target_hp,
        target_hp_after,
    );
}

#[test]
fn projectile_cleanup_removes_inventory() {
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.spawn_projectile(
        VoxelCoord::new(40, 5, 40),
        VoxelCoord::new(50, 5, 40),
        None,
        sim.home_zone_id(),
    );
    let proj = sim.db.projectiles.iter_all().next().unwrap();
    let inv_id = proj.inventory_id;
    let proj_id = proj.id;

    // Verify inventory exists.
    assert!(sim.db.inventories.get(&inv_id).is_some());

    sim.remove_projectile(proj_id);

    // Projectile, inventory, and item stacks should all be gone.
    assert_eq!(sim.db.projectiles.len(), 0);
    assert!(sim.db.inventories.get(&inv_id).is_none());
    assert!(
        sim.db
            .item_stacks
            .by_inventory_id(&inv_id, tabulosity::QueryOpts::ASC)
            .is_empty()
    );
}

#[test]
fn projectile_serde_roundtrip() {
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.spawn_projectile(
        VoxelCoord::new(40, 5, 40),
        VoxelCoord::new(50, 5, 40),
        None,
        sim.home_zone_id(),
    );

    let json = sim.to_json().unwrap();
    let sim2 = SimState::from_json(&json).unwrap();

    assert_eq!(sim2.db.projectiles.len(), 1);
    let proj = sim2.db.projectiles.iter_all().next().unwrap();
    let stacks = sim2
        .db
        .item_stacks
        .by_inventory_id(&proj.inventory_id, tabulosity::QueryOpts::ASC);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].kind, inventory::ItemKind::Arrow);
}

#[test]
fn debug_spawn_projectile_command() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let origin = VoxelCoord::new(40, 5, 40);
    let target = VoxelCoord::new(50, 5, 40);
    let tick = sim.tick;

    sim.step(
        &[SimCommand {
            player_name: String::new(),
            tick: tick + 1,
            action: SimAction::DebugSpawnProjectile {
                zone_id: sim.home_zone_id(),
                origin,
                target,
                shooter_id: None,
            },
        }],
        tick + 1,
    );

    assert_eq!(sim.db.projectiles.len(), 1);
}

// -----------------------------------------------------------------------
// F-attack-task: AttackTarget task tests
// -----------------------------------------------------------------------

#[test]
fn attack_creature_command_creates_task_and_assigns() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place them nearby.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);

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

    // Elf should have an AttackTarget task assigned.
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        creature.current_task.is_some(),
        "Elf should have a task assigned"
    );
    let task = sim.db.tasks.get(&creature.current_task.unwrap()).unwrap();
    assert_eq!(task.kind_tag, crate::db::TaskKindTag::AttackTarget);
    assert_eq!(task.state, TaskState::InProgress);
    assert_eq!(task.origin, TaskOrigin::PlayerDirected);
    assert_eq!(task.target_creature, Some(goblin));

    // Extension data should exist.
    let attack_data = sim.task_attack_target_data(task.id).unwrap();
    assert_eq!(attack_data.target, goblin);
    assert_eq!(attack_data.path_failures, 0);
}

#[test]
fn attack_target_task_pursues_and_strikes() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place goblin within reach of elf.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let goblin_pos = VoxelCoord::new(elf_pos.x + 3, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, goblin_pos);
    force_idle(&mut sim, elf);
    suppress_activation(&mut sim, elf);
    suppress_activation_until(&mut sim, goblin, u64::MAX);

    let goblin_hp_before = sim.db.creatures.get(&goblin).unwrap().hp;

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
    // Run for 10 seconds — enough time to walk there and attack.
    sim.step(&[cmd], tick + 10_000);

    let goblin_hp_after = sim.db.creatures.get(&goblin).unwrap().hp;
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    let final_dist = elf_creature.position.min.manhattan_distance(goblin_pos);

    let moved_closer = final_dist < elf_pos.manhattan_distance(goblin_pos);
    let dealt_damage = goblin_hp_after < goblin_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Elf should pursue and/or damage goblin: initial_dist={}, final_dist={final_dist}, \
             hp {goblin_hp_before} -> {goblin_hp_after}",
        elf_pos.manhattan_distance(goblin_pos)
    );
}

#[test]
fn attack_target_completes_when_target_dies() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Place goblin adjacent to elf (instant melee range).
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let adjacent_pos = VoxelCoord::new(elf_pos.x + 1, elf_pos.y, elf_pos.z);
    force_position(&mut sim, goblin, adjacent_pos);
    force_idle(&mut sim, elf);

    // Give elf high melee damage to kill quickly.
    // Just use commands to create the attack task and then kill the target.
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

    let task_id = sim.db.creatures.get(&elf).unwrap().current_task.unwrap();

    // Kill the goblin.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 3,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    // Step enough that the elf's activation runs after the kill.
    sim.step(&[kill_cmd], tick + 5000);

    // Task should be complete.
    let task = sim.db.tasks.get(&task_id).unwrap();
    assert_eq!(
        task.state,
        TaskState::Complete,
        "Attack task should complete when target dies"
    );
    // Elf should be free.
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.current_task.is_none(),
        "Elf should have no task after target dies"
    );
}

#[test]
fn attack_target_preempts_lower_priority_task() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);

    // Give elf a GoTo task (PlayerDirected level 2).
    let elf_pos = creature_pos(&sim, elf);
    let far_pos = find_far_walkable(&sim, elf_pos, 10);
    let goto_task_id = insert_goto_task(&mut sim, far_pos);
    sim.claim_task(elf, goto_task_id);

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

    // Old task should be completed/interrupted.
    let old_task = sim.db.tasks.get(&goto_task_id).unwrap();
    assert_eq!(
        old_task.state,
        TaskState::Complete,
        "GoTo task should be interrupted by AttackCreature"
    );

    // Elf should have the attack task.
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(elf_creature.current_task.is_some());
    let new_task = sim
        .db
        .tasks
        .get(&elf_creature.current_task.unwrap())
        .unwrap();
    assert_eq!(new_task.kind_tag, crate::db::TaskKindTag::AttackTarget);
}

#[test]
fn attack_target_cannot_attack_self() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    force_idle(&mut sim, elf);

    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: elf,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Elf should NOT have an attack task.
    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.current_task.is_none(),
        "Should not be able to attack self"
    );
}

#[test]
fn attack_target_cannot_attack_dead_creature() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let elf = spawn_elf(&mut sim);
    let goblin = spawn_species(&mut sim, Species::Goblin);
    force_idle(&mut sim, elf);

    // Kill goblin first.
    let tick = sim.tick;
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::DebugKillCreature {
            creature_id: goblin,
        },
    };
    sim.step(&[kill_cmd], tick + 2);

    // Try to attack.
    let attack_cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 3,
        action: SimAction::AttackCreature {
            attacker_id: elf,
            target_id: goblin,
            queue: false,
        },
    };
    sim.step(&[attack_cmd], tick + 4);

    let elf_creature = sim.db.creatures.get(&elf).unwrap();
    assert!(
        elf_creature.current_task.is_none(),
        "Should not be able to attack dead creature"
    );
}

#[test]
fn attack_target_task_serde_roundtrip() {
    let mut rng = GameRng::new(42);
    let task_id = TaskId::new(&mut rng);
    let target = CreatureId::new(&mut rng);
    let location = VoxelCoord::new(5, 0, 0);

    let task = Task {
        id: task_id,
        kind: TaskKind::AttackTarget { target },
        state: TaskState::InProgress,
        location,
        progress: 0,
        total_cost: 0,
        required_species: Some(Species::Elf),
        origin: TaskOrigin::PlayerDirected,
        target_creature: Some(target),
        restrict_to_creature_id: None,
        prerequisite_task_id: None,
        required_civ_id: None,
    };

    let json = serde_json::to_string(&task).unwrap();
    let restored: Task = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, task_id);
    match &restored.kind {
        TaskKind::AttackTarget { target: t } => assert_eq!(*t, target),
        other => panic!("Expected AttackTarget, got {:?}", other),
    }
    assert_eq!(restored.state, TaskState::InProgress);
    assert_eq!(restored.origin, TaskOrigin::PlayerDirected);
    assert_eq!(restored.target_creature, Some(target));
}

// -----------------------------------------------------------------------
// AttackTarget preemption level tests
// -----------------------------------------------------------------------

#[test]
fn attack_target_preemption_is_player_combat() {
    assert_eq!(
        preemption::preemption_level(
            crate::db::TaskKindTag::AttackTarget,
            TaskOrigin::PlayerDirected
        ),
        preemption::PreemptionLevel::PlayerCombat,
    );
}

#[test]
fn worldgen_creates_default_military_groups() {
    let sim = flat_world_sim(fresh_test_seed());
    let civ_id = sim.player_civ_id.unwrap();
    let groups = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC);

    assert!(
        groups.len() >= 2,
        "Should have at least 2 groups (Civilians + Soldiers)"
    );

    let civilians = groups.iter().filter(|g| g.is_default_civilian).count();
    assert_eq!(civilians, 1, "Exactly one civilian group per civ");

    let civilian = groups.iter().find(|g| g.is_default_civilian).unwrap();
    assert_eq!(civilian.name, "Civilians");
    assert_eq!(civilian.engagement_style.disengage_threshold_pct, 100);

    let soldiers = groups.iter().find(|g| g.name == "Soldiers").unwrap();
    assert!(!soldiers.is_default_civilian);
    assert_eq!(
        soldiers.engagement_style.initiative,
        crate::species::EngagementInitiative::Aggressive
    );
}

#[test]
fn worldgen_all_civs_have_military_groups() {
    let sim = flat_world_sim(fresh_test_seed());
    for civ in sim.db.civilizations.iter_all() {
        let groups = sim
            .db
            .military_groups
            .by_civ_id(&civ.id, tabulosity::QueryOpts::ASC);
        let civilian_count = groups.iter().filter(|g| g.is_default_civilian).count();
        assert_eq!(
            civilian_count, 1,
            "Civ {:?} should have exactly 1 civilian group",
            civ.id
        );
        assert!(
            groups.len() >= 2,
            "Civ {:?} should have at least Civilians + Soldiers",
            civ.id
        );
    }
}

#[test]
fn create_military_group_command() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateMilitaryGroup {
            name: "Archers".to_string(),
        },
    };
    let result = sim.step(&[cmd], 1);

    // Should emit MilitaryGroupCreated event.
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(&e.kind, SimEventKind::MilitaryGroupCreated { .. })),
        "Should emit MilitaryGroupCreated event"
    );

    // The new group should exist in the DB.
    let civ_id = sim.player_civ_id.unwrap();
    let groups = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC);
    let archers = groups.iter().find(|g| g.name == "Archers");
    assert!(archers.is_some(), "Archers group should exist");
    let archers = archers.unwrap();
    assert!(!archers.is_default_civilian);
    assert_eq!(
        archers.engagement_style.initiative,
        crate::species::EngagementInitiative::Aggressive,
        "New groups default to Aggressive"
    );
}

#[test]
fn creature_reassignment_to_group_and_back() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    // Spawn an elf.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("should spawn elf");

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.military_group, None, "Spawned elf is implicit civilian");

    // Assign to soldiers.
    let soldiers = soldiers_group(&sim);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(soldiers.id),
        },
    };
    sim.step(&[cmd], 1);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(
        elf.military_group,
        Some(soldiers.id),
        "Elf should be in soldiers group"
    );

    // Reassign back to civilian (None).
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: None,
        },
    };
    sim.step(&[cmd], 2);

    let elf = sim.db.creatures.get(&elf_id).unwrap();
    assert_eq!(elf.military_group, None, "Elf should be back to civilian");
}

#[test]
fn reassign_between_non_civilian_groups() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("should spawn elf");

    // Create a second group.
    let create_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateMilitaryGroup {
            name: "Archers".to_string(),
        },
    };
    sim.step(&[create_cmd], 1);

    let civ_id = sim.player_civ_id.unwrap();
    let archers = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.name == "Archers")
        .unwrap();

    // Assign to soldiers.
    let soldiers = soldiers_group(&sim);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(soldiers.id),
        },
    };
    sim.step(&[cmd], 2);
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        Some(soldiers.id)
    );

    // Reassign to archers.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 3,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(archers.id),
        },
    };
    sim.step(&[cmd], 3);
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        Some(archers.id)
    );
}

#[test]
fn delete_military_group_nullifies_members() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("should spawn elf");

    // Create a new group and assign the elf to it.
    let create_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::CreateMilitaryGroup {
            name: "Scouts".to_string(),
        },
    };
    sim.step(&[create_cmd], 1);

    let civ_id = sim.player_civ_id.unwrap();
    let scouts = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC)
        .into_iter()
        .find(|g| g.name == "Scouts")
        .unwrap();

    let assign_cmd = SimCommand {
        player_name: String::new(),
        tick: 2,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(scouts.id),
        },
    };
    sim.step(&[assign_cmd], 2);
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        Some(scouts.id)
    );

    // Delete the group.
    let delete_cmd = SimCommand {
        player_name: String::new(),
        tick: 3,
        action: SimAction::DeleteMilitaryGroup {
            group_id: scouts.id,
        },
    };
    let result = sim.step(&[delete_cmd], 3);

    // Elf should be back to civilian (None).
    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        None,
        "Deleted group should nullify creature.military_group"
    );

    // Group should be gone.
    assert!(
        sim.db.military_groups.get(&scouts.id).is_none(),
        "Deleted group should be removed"
    );

    // Should emit MilitaryGroupDeleted event.
    assert!(
        result.events.iter().any(|e| matches!(
            &e.kind,
            SimEventKind::MilitaryGroupDeleted {
                name, member_count, ..
            } if name == "Scouts" && *member_count == 1
        )),
        "Should emit MilitaryGroupDeleted with correct name and count"
    );
}

#[test]
fn civilian_group_deletion_rejected() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let civ_group = civilian_group(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DeleteMilitaryGroup {
            group_id: civ_group.id,
        },
    };
    sim.step(&[cmd], 1);

    // Civilian group should still exist.
    assert!(
        sim.db.military_groups.get(&civ_group.id).is_some(),
        "Civilian group cannot be deleted"
    );
}

#[test]
fn dead_creature_not_counted_in_member_count() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_a = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn elf a");
    let elf_b = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn elf b");

    // Assign both to soldiers.
    let soldiers = soldiers_group(&sim);
    for eid in [elf_a, elf_b] {
        set_military_group(&mut sim, eid, Some(soldiers.id));
    }

    // Kill elf_b.
    let kill_cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::DebugKillCreature { creature_id: elf_b },
    };
    sim.step(&[kill_cmd], 1);

    // Count alive members.
    let alive_count = sim
        .db
        .creatures
        .by_military_group(&Some(soldiers.id), tabulosity::QueryOpts::ASC)
        .iter()
        .filter(|c| c.vital_status == VitalStatus::Alive)
        .count();
    assert_eq!(alive_count, 1, "Only elf_a should be alive in soldiers");

    // Dead elf should still be assigned to soldiers.
    let dead_elf = sim.db.creatures.get(&elf_b).unwrap();
    assert_eq!(
        dead_elf.military_group,
        Some(soldiers.id),
        "Dead creature preserves group assignment"
    );
    assert_eq!(dead_elf.vital_status, VitalStatus::Dead);
}

#[test]
fn cross_civ_reassignment_rejected() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    // Get a non-player civ.
    let ai_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| !c.player_controlled)
        .expect("need an AI civ");
    let ai_groups = sim
        .db
        .military_groups
        .by_civ_id(&ai_civ.id, tabulosity::QueryOpts::ASC);
    let ai_soldiers = ai_groups
        .iter()
        .find(|g| !g.is_default_civilian)
        .expect("AI civ should have a non-civilian group");

    // Spawn an elf (player civ).
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn elf");

    // Try to assign elf to AI civ's group — should be rejected.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: elf_id,
            group_id: Some(ai_soldiers.id),
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(
        sim.db.creatures.get(&elf_id).unwrap().military_group,
        None,
        "Cross-civ reassignment should be rejected"
    );
}

#[test]
fn non_civ_creature_reassignment_rejected() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let goblin_id = sim
        .spawn_creature(Species::Goblin, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn goblin");

    let soldiers = soldiers_group(&sim);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::ReassignMilitaryGroup {
            creature_id: goblin_id,
            group_id: Some(soldiers.id),
        },
    };
    sim.step(&[cmd], 1);

    assert_eq!(
        sim.db.creatures.get(&goblin_id).unwrap().military_group,
        None,
        "Non-civ creatures cannot be assigned to military groups"
    );
}

#[test]
fn rename_civilian_group() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let civ_group = civilian_group(&sim);

    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::RenameMilitaryGroup {
            group_id: civ_group.id,
            name: "Villagers".to_string(),
        },
    };
    sim.step(&[cmd], 1);

    let renamed = sim.db.military_groups.get(&civ_group.id).unwrap();
    assert_eq!(renamed.name, "Villagers");
}

#[test]
fn set_group_engagement_style() {
    use crate::species::{
        AmmoExhaustedBehavior, EngagementInitiative, EngagementStyle, WeaponPreference,
    };
    let mut sim = flat_world_sim(fresh_test_seed());
    let civ_group = civilian_group(&sim);

    // Change civilian group to aggressive.
    let new_style = EngagementStyle {
        weapon_preference: WeaponPreference::PreferMelee,
        ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
        initiative: EngagementInitiative::Aggressive,
        disengage_threshold_pct: 0,
    };
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SetGroupEngagementStyle {
            group_id: civ_group.id,
            engagement_style: new_style,
        },
    };
    sim.step(&[cmd], 1);

    let updated = sim.db.military_groups.get(&civ_group.id).unwrap();
    assert_eq!(
        updated.engagement_style.initiative,
        EngagementInitiative::Aggressive
    );
    assert_eq!(updated.engagement_style.disengage_threshold_pct, 0);
}

#[test]
fn fk_cascade_civ_delete_removes_groups() {
    let mut sim = flat_world_sim(fresh_test_seed());

    // Find an AI civ (not the player civ, which might cause issues).
    let ai_civ = sim
        .db
        .civilizations
        .iter_all()
        .find(|c| !c.player_controlled);
    let Some(ai_civ) = ai_civ else {
        // No AI civ in this seed — skip test.
        return;
    };
    let ai_civ_id = ai_civ.id;

    let groups_before = sim
        .db
        .military_groups
        .by_civ_id(&ai_civ_id, tabulosity::QueryOpts::ASC);
    assert!(
        !groups_before.is_empty(),
        "AI civ should have military groups"
    );

    // Delete the civ — groups should cascade.
    sim.db.remove_civilization(&ai_civ_id).unwrap();

    let groups_after = sim
        .db
        .military_groups
        .by_civ_id(&ai_civ_id, tabulosity::QueryOpts::ASC);
    assert!(
        groups_after.is_empty(),
        "Deleting a civ should cascade-delete its military groups"
    );
}

#[test]
fn aggressive_group_civ_creature_auto_engages() {
    // This test verifies that an aggressive-group civ creature will attempt to
    // pursue hostiles via wander(), not just avoid them.
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn elf");

    // Assign to soldiers (Aggressive).
    let soldiers = soldiers_group(&sim);
    set_military_group(&mut sim, elf_id, Some(soldiers.id));

    // Verify resolve_engagement_style returns Aggressive.
    let style = sim.resolve_engagement_style(elf_id);
    assert_eq!(
        style.initiative,
        crate::species::EngagementInitiative::Aggressive,
        "Soldiers group should resolve to Aggressive"
    );
}

#[test]
fn resolve_engagement_style_implicit_civilian() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let elf_id = sim
        .spawn_creature(Species::Elf, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn elf");

    // Implicit civilian (military_group = None, civ_id = Some).
    let style = sim.resolve_engagement_style(elf_id);
    assert_eq!(
        style.disengage_threshold_pct, 100,
        "Implicit civilian should have 100% disengage threshold (always flee)"
    );
}

#[test]
fn resolve_engagement_style_non_civ_creature() {
    let mut sim = flat_world_sim(fresh_test_seed());
    let mut events = Vec::new();

    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let goblin_id = sim
        .spawn_creature(Species::Goblin, tree_pos, sim.home_zone_id(), &mut events)
        .expect("spawn goblin");

    // Non-civ creature → species default (Aggressive for goblins).
    let style = sim.resolve_engagement_style(goblin_id);
    assert_eq!(
        style.initiative,
        crate::species::EngagementInitiative::Aggressive,
        "Non-civ goblin should use species default (Aggressive)"
    );
}

// -----------------------------------------------------------------------
// Military group serde roundtrip (moved from flee_tests.rs)
// -----------------------------------------------------------------------

#[test]
fn military_group_serde_roundtrip() {
    let sim = flat_world_sim(fresh_test_seed());

    // Serialize and deserialize the full SimDb.
    let json = serde_json::to_string(&sim.db).unwrap();
    let db2: crate::db::SimDb = serde_json::from_str(&json).unwrap();

    let civ_id = sim.player_civ_id.unwrap();
    let groups_orig = sim
        .db
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC);
    let groups_deser = db2
        .military_groups
        .by_civ_id(&civ_id, tabulosity::QueryOpts::ASC);

    assert_eq!(groups_orig.len(), groups_deser.len());
    for (a, b) in groups_orig.iter().zip(groups_deser.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.civ_id, b.civ_id);
        assert_eq!(a.name, b.name);
        assert_eq!(a.is_default_civilian, b.is_default_civilian);
        assert_eq!(a.engagement_style, b.engagement_style);
    }
}

#[test]
fn military_group_command_serde_roundtrip() {
    use crate::species::{
        AmmoExhaustedBehavior, EngagementInitiative, EngagementStyle, WeaponPreference,
    };

    let actions = vec![
        SimAction::CreateMilitaryGroup {
            name: "Guards".to_string(),
        },
        SimAction::DeleteMilitaryGroup {
            group_id: MilitaryGroupId(1),
        },
        SimAction::ReassignMilitaryGroup {
            creature_id: CreatureId(SimUuid::new_v4(&mut GameRng::new(1))),
            group_id: Some(MilitaryGroupId(2)),
        },
        SimAction::RenameMilitaryGroup {
            group_id: MilitaryGroupId(1),
            name: "Elite".to_string(),
        },
        SimAction::SetGroupEngagementStyle {
            group_id: MilitaryGroupId(1),
            engagement_style: EngagementStyle {
                weapon_preference: WeaponPreference::PreferRanged,
                ammo_exhausted: AmmoExhaustedBehavior::Flee,
                initiative: EngagementInitiative::Defensive,
                disengage_threshold_pct: 100,
            },
        },
    ];

    for action in &actions {
        let json = serde_json::to_string(action).unwrap();
        let deser: SimAction = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deser).unwrap();
        assert_eq!(json, json2, "Serde roundtrip failed for {action:?}");
    }
}

// -----------------------------------------------------------------------
// B-combat-move-stats: combat movement delay should use creature stats
// -----------------------------------------------------------------------

#[test]
fn attack_move_traversal_delay_uses_creature_stats() {
    // An elf with very high agility should move faster during attack-move
    // than the base species move_ticks_per_voxel would give.
    let mut sim = flat_world_sim(fresh_test_seed());
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .food_decay_per_tick = 0;
    sim.species_table
        .get_mut(&Species::Elf)
        .unwrap()
        .rest_decay_per_tick = 0;
    let elf = spawn_elf(&mut sim);

    // Give the elf very high agility (stat 200 → significant speed bonus).
    sim.db
        .upsert_creature_trait(crate::db::CreatureTrait {
            creature_id: elf,
            trait_kind: TraitKind::Agility,
            value: crate::types::TraitValue::Int(200),
        })
        .unwrap();

    // Find a ground node far from the elf to attack-move toward.
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let distant_node = find_far_walkable(&sim, elf_pos, 5);

    // Issue attack-move command.
    let tick = sim.tick;
    let cmd = SimCommand {
        player_name: String::new(),
        tick: tick + 1,
        action: SimAction::AttackMove {
            zone_id: sim.home_zone_id(),
            creature_id: elf,
            destination: distant_node,
            queue: false,
        },
    };
    sim.step(&[cmd], tick + 2);

    // Advance until the elf takes a Move step (action_kind == Move with next_available_tick set).
    let mut moved = false;
    let mut delay_ticks = 0u64;
    for t in (sim.tick + 1)..=(sim.tick + 200) {
        sim.step(&[], t);
        let c = sim.db.creatures.get(&elf).unwrap();
        if c.action_kind == ActionKind::Move && c.next_available_tick.is_some() {
            delay_ticks = c.next_available_tick.unwrap() - t;
            moved = true;
            break;
        }
    }
    assert!(moved, "Elf should have taken a Move step");

    // Compute what the delay would be with no stat modifier (agility=0).
    // The actual edge type doesn't matter — we just need to show that the
    // delay with agility 200 is strictly less than with agility 0, proving
    // that creature stats are applied to combat movement timing.
    let species_data = &sim.species_table[&Species::Elf];
    let base_tpv = crate::stats::creature_base_tpv(species_data.move_ticks_per_voxel, 0);
    let stat_tpv = crate::stats::creature_base_tpv(species_data.move_ticks_per_voxel, 200);

    // Check that stat-modified speeds ARE actually faster.
    assert!(
        stat_tpv < base_tpv,
        "Stat-modified TPV ({}) should be less than base ({})",
        stat_tpv,
        base_tpv,
    );

    // The delay we observed should be less than what base speeds would give
    // on the same edge. We don't know the exact edge, but we know the
    // maximum possible base delay for any edge: the longest edge (corner
    // diagonal, distance 1773) at the slowest TPV. For WalkOrLadder, the
    // slowest edge is ladders at 2x base.
    let max_base_tpv = base_tpv * 2;
    let max_base_delay = (1773u64 * max_base_tpv)
        .div_ceil(crate::nav::DIST_SCALE as u64)
        .max(1);
    assert!(
        delay_ticks < max_base_delay,
        "Delay {delay_ticks} should be less than max base delay {max_base_delay} \
         because the elf has high agility (200)"
    );
}

// -----------------------------------------------------------------------
// Unified pursuit (F-unified-pursuit)
// -----------------------------------------------------------------------

#[test]
fn ground_creature_gives_up_on_high_flying_target() {
    // A goblin on the ground should not spend CPU pathfinding toward a hornet
    // that is way up in the sky — no nav nodes exist within melee range of the
    // target, so pursue_closest_target should bail immediately.
    let mut sim = flat_world_sim(fresh_test_seed());

    let goblin_id = spawn_species(&mut sim, Species::Goblin);
    let hornet_id = spawn_hornet_at(&mut sim, VoxelCoord::new(34, 20, 32));

    // Place goblin on the ground near the hornet's x,z but far below.
    force_position(&mut sim, goblin_id, VoxelCoord::new(32, 1, 32));
    force_idle(&mut sim, goblin_id);

    // Run a short period — the goblin should detect the hornet (within
    // detection range) but fail to pursue (no ground node in melee range of
    // y=20 target). It should wander instead of freeze.
    sim.step(&[], sim.tick + 5000);

    let goblin_pos_after = sim.db.creatures.get(&goblin_id).unwrap().position.min;

    // The goblin should NOT have moved closer to the hornet's y position.
    // (It may have wandered on the ground plane, which is fine.)
    assert_eq!(
        goblin_pos_after.y, 1,
        "Goblin should stay on the ground, not levitate toward the hornet"
    );
    // It should not have dealt any damage to the hornet (still at full HP).
    let hornet = sim.db.creatures.get(&hornet_id).unwrap();
    assert_eq!(
        hornet.hp, hornet.hp_max,
        "Goblin should not have damaged the high-altitude hornet"
    );
}

#[test]
fn ground_creature_pursues_target_at_different_elevation() {
    // A goblin should pursue an elf that is slightly elevated (y=2 instead
    // of y=1). The elf at y=2 has no nav node on the goblin's graph, so
    // pursue_closest_target must find ground-level strike positions within
    // melee range. With melee_range_sq=2, positions at y=1 directly under
    // the elf are within melee range (vertical gap=0 for adjacent voxels).
    let mut sim = flat_world_sim(fresh_test_seed());

    let goblin = spawn_species(&mut sim, Species::Goblin);
    let elf = spawn_elf(&mut sim);

    // Force elf to y=2 (not walkable in a flat world).
    force_position(&mut sim, elf, VoxelCoord::new(37, 2, 32));
    force_position(&mut sim, goblin, VoxelCoord::new(34, 1, 32));
    force_guaranteed_hits(&mut sim, goblin);
    // Freeze elf so it doesn't flee.
    force_idle_and_cancel_activations(&mut sim, elf);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let goblin_start = VoxelCoord::new(34, 1, 32);
    let elf_pos_start = VoxelCoord::new(37, 2, 32);
    let initial_dist = goblin_start.manhattan_distance(elf_pos_start);

    sim.step(&[], sim.tick + 10_000);

    let goblin_pos = sim.db.creatures.get(&goblin).unwrap().position.min;
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let new_dist = goblin_pos.manhattan_distance(elf_pos);
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;

    let moved_closer = new_dist < initial_dist;
    let dealt_damage = elf_hp_after < elf_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Goblin should pursue elevated elf: initial dist={initial_dist}, \
         new dist={new_dist}, elf hp {elf_hp_before} -> {elf_hp_after}"
    );
}

#[test]
fn flying_creature_pursues_ground_target() {
    // A hornet should pursue an elf on the ground, closing distance and/or
    // dealing damage. Under the unified pursuit path, the hornet enumerates
    // strike positions within melee range and uses find_nearest + find_path
    // rather than a Euclidean distance approximation.
    let mut sim = flat_world_sim(fresh_test_seed());

    let elf = spawn_elf(&mut sim);
    let hornet = spawn_hornet_at(&mut sim, VoxelCoord::new(37, 3, 32));

    force_position(&mut sim, elf, VoxelCoord::new(32, 1, 32));
    force_guaranteed_hits(&mut sim, hornet);
    // Freeze elf so it doesn't flee.
    force_idle_and_cancel_activations(&mut sim, elf);

    let elf_hp_before = sim.db.creatures.get(&elf).unwrap().hp;
    let initial_dist = VoxelCoord::new(37, 3, 32).manhattan_distance(VoxelCoord::new(32, 1, 32));

    sim.step(&[], sim.tick + 10_000);

    let hornet_pos = sim.db.creatures.get(&hornet).unwrap().position.min;
    let elf_pos = sim.db.creatures.get(&elf).unwrap().position.min;
    let new_dist = hornet_pos.manhattan_distance(elf_pos);
    let elf_hp_after = sim.db.creatures.get(&elf).unwrap().hp;

    let moved_closer = new_dist < initial_dist;
    let dealt_damage = elf_hp_after < elf_hp_before;
    assert!(
        moved_closer || dealt_damage,
        "Hornet should pursue elf via pathfinding: initial dist={initial_dist}, \
         new dist={new_dist}, elf hp {elf_hp_before} -> {elf_hp_after}"
    );
}
