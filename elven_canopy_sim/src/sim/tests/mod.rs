//! Top-level test module for the simulator. Re-exports shared helpers from
//! `test_helpers` and declares domain-specific test submodules. Only core
//! tests (basic sim setup, determinism, serialization, spatial index,
//! checksum) live directly here; all domain tests are in submodules.

use super::*;
use crate::building;
use crate::db::{Blueprint, CompletedStructure, MoveAction, TaskKindTag};
use crate::event::SimEventKind;
use crate::inventory::{self, ItemKind, Material};
use crate::preemption;
use crate::recipe::Recipe;
use crate::task::{Task, TaskKind, TaskOrigin, TaskState};
use crate::types::{ActiveRecipeId, NotificationId};

// ---------------------------------------------------------------------------
// Shared test helpers (re-exported so submodules get them via `use super::*`)
// ---------------------------------------------------------------------------

pub(super) mod test_helpers;
use test_helpers::*;

// ---------------------------------------------------------------------------
// Domain-specific test submodules
// ---------------------------------------------------------------------------

mod activation_tests;
mod activity_tests;
mod combat_tests;
mod commands_tests;
mod construction_tests;
mod crafting_tests;
mod creature_tests;
mod dinner_party_tests;
mod diplomacy_tests;
mod equipment_tests;
mod flee_tests;
mod foraging_tests;
mod grazing_tests;
mod greenhouse_tests;
mod hp_tests;
mod inventory_tests;
mod llm_tests;
mod logistics_tests;
mod mana_tests;
mod mood_tests;
mod movement_tests;
mod needs_tests;
mod paths_tests;
mod raid_tests;
mod skills_tests;
mod social_dance_tests;
mod social_tests;
mod taming_tests;
mod worldgen_tests;

// =========================================================================
// Core tests: basic sim setup, determinism, serialization, tree
// =========================================================================

#[test]
fn new_sim_has_home_tree() {
    let sim = test_sim(legacy_test_seed());
    assert!(sim.db.trees.contains(&sim.player_tree_id));
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert_eq!(tree.owner, sim.player_civ_id);
    let great_info = sim.db.great_tree_infos.get(&sim.player_tree_id).unwrap();
    assert_eq!(great_info.mana_stored, sim.config.starting_mana);
}

#[test]
fn determinism_two_sims_same_seed() {
    let seed = legacy_test_seed();
    let sim_a = test_sim(seed);
    let sim_b = test_sim(seed);
    assert_eq!(sim_a.player_civ_id, sim_b.player_civ_id);
    assert_eq!(sim_a.player_tree_id, sim_b.player_tree_id);
    assert_eq!(sim_a.tick, sim_b.tick);
}

#[test]
fn step_advances_tick() {
    let mut sim = test_sim(legacy_test_seed());
    sim.step(&[], 100);
    assert_eq!(sim.tick, 100);
}

#[test]
fn step_updates_world_sim_tick() {
    let mut sim = test_sim(legacy_test_seed());
    assert_eq!(sim.voxel_zone(sim.home_zone_id()).unwrap().sim_tick, 0);
    sim.step(&[], 10);
    assert_eq!(sim.voxel_zone(sim.home_zone_id()).unwrap().sim_tick, 10);
    sim.step(&[], 25);
    assert_eq!(sim.voxel_zone(sim.home_zone_id()).unwrap().sim_tick, 25);
}

#[test]
fn tree_heartbeat_reschedules() {
    let mut sim = test_sim(legacy_test_seed());
    let heartbeat_interval = sim.config.tree_heartbeat_interval_ticks;

    // Step past the first heartbeat.
    sim.step(&[], heartbeat_interval + 1);

    // The tree heartbeat should have rescheduled at tick = 2 * heartbeat_interval.
    // Other periodic events (e.g. LogisticsHeartbeat) may sit earlier in the queue,
    // so pop events until we find the TreeHeartbeat and verify its tick.
    let mut found_tree_heartbeat = false;
    while let Some(evt) = sim.event_queue.pop_if_ready(u64::MAX) {
        if matches!(evt.kind, ScheduledEventKind::TreeHeartbeat { .. }) {
            assert_eq!(evt.tick, heartbeat_interval * 2);
            found_tree_heartbeat = true;
            break;
        }
    }
    assert!(
        found_tree_heartbeat,
        "TreeHeartbeat not found in event queue"
    );
}

#[test]
fn serialization_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    sim.step(&[], 50);
    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();
    assert_eq!(sim.tick, restored.tick);
    assert_eq!(sim.player_civ_id, restored.player_civ_id);
    assert_eq!(sim.player_tree_id, restored.player_tree_id);
}

#[test]
fn serialization_roundtrip_preserves_tree_and_great_tree_info() {
    let mut sim = test_sim(legacy_test_seed());
    // Fast-forward a bit so fruit may have spawned.
    sim.step(&[], 500);

    let tree_id = sim.player_tree_id;
    let orig_tree = sim.db.trees.get(&tree_id).unwrap().clone();
    let orig_info = sim.db.great_tree_infos.get(&tree_id).unwrap();

    let json = serde_json::to_string(&sim).unwrap();
    let restored: SimState = serde_json::from_str(&json).unwrap();

    // Tree row fields survive roundtrip.
    let rt = restored.db.trees.get(&tree_id).unwrap();
    assert_eq!(rt.position, orig_tree.position);
    assert_eq!(rt.health, orig_tree.health);
    assert_eq!(rt.growth_level, orig_tree.growth_level);
    assert_eq!(rt.owner, orig_tree.owner);
    assert_eq!(rt.trunk_voxels.len(), orig_tree.trunk_voxels.len());
    assert_eq!(rt.branch_voxels.len(), orig_tree.branch_voxels.len());
    assert_eq!(rt.leaf_voxels.len(), orig_tree.leaf_voxels.len());
    assert_eq!(rt.root_voxels.len(), orig_tree.root_voxels.len());
    assert_eq!(rt.fruit_species_id, orig_tree.fruit_species_id);
    // TreeFruit rows survive roundtrip — count by tree_id.
    let orig_fruit_count = sim
        .db
        .tree_fruits
        .count_by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    let rt_fruit_count = restored
        .db
        .tree_fruits
        .count_by_tree_id(&tree_id, tabulosity::QueryOpts::ASC);
    assert_eq!(orig_fruit_count, rt_fruit_count);

    // GreatTreeInfo row fields survive roundtrip.
    let ri = restored.db.great_tree_infos.get(&tree_id).unwrap();
    assert_eq!(ri.mana_stored, orig_info.mana_stored);
    assert_eq!(ri.mana_capacity, orig_info.mana_capacity);
    assert_eq!(
        ri.fruit_production_rate_ppm,
        orig_info.fruit_production_rate_ppm
    );
    assert_eq!(ri.carrying_capacity, orig_info.carrying_capacity);
    assert_eq!(ri.current_load, orig_info.current_load);
}

#[test]
fn tree_owner_index_finds_player_tree() {
    let sim = test_sim(legacy_test_seed());
    let civ_id = sim.player_civ_id.unwrap();
    let trees_for_civ = sim
        .db
        .trees
        .by_owner(&Some(civ_id), tabulosity::QueryOpts::ASC);
    assert_eq!(trees_for_civ.len(), 1);
    assert_eq!(trees_for_civ[0].id, sim.player_tree_id);
}

#[test]
fn tree_owner_index_lesser_trees_have_no_owner() {
    let sim = test_sim(legacy_test_seed());
    let unowned = sim.db.trees.by_owner(&None, tabulosity::QueryOpts::ASC);
    // All trees except the player's tree should have no owner.
    assert_eq!(unowned.len(), sim.db.trees.len() - 1);
}

#[test]
fn remove_tree_fruit_at_no_op_when_not_found() {
    let mut sim = test_sim(legacy_test_seed());
    let bogus_pos = VoxelCoord::new(0, 0, 0);
    let before = sim.db.tree_fruits.len();
    // Should not panic on a position with no fruit.
    sim.remove_tree_fruit_at(bogus_pos);
    assert_eq!(
        before,
        sim.db.tree_fruits.len(),
        "No fruit should have been removed"
    );
}

#[test]
fn remove_tree_fruit_at_removes_correct_fruit() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;
    let species_id = {
        let id = insert_test_fruit_species(&mut sim);
        let mut t = sim.db.trees.get(&tree_id).unwrap();
        t.fruit_species_id = Some(id);
        sim.db.update_tree(t).unwrap();
        id
    };

    // Manually insert two TreeFruit rows.
    let fruit_a = VoxelCoord::new(10, 60, 10);
    let fruit_b = VoxelCoord::new(11, 60, 11);
    let hz = sim.home_zone_id();
    let _ = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        zone_id: hz,
        tree_id,
        position: VoxelBox::point(fruit_a),
        species_id,
    });
    let _ = sim.db.insert_tree_fruit_auto(|id| crate::db::TreeFruit {
        id,
        zone_id: hz,
        tree_id,
        position: VoxelBox::point(fruit_b),
        species_id,
    });

    // Remove fruit_a — fruit_b should remain.
    sim.remove_tree_fruit_at(fruit_a);

    assert!(
        sim.db
            .tree_fruits
            .by_position(&VoxelBox::point(fruit_a), tabulosity::QueryOpts::ASC)
            .is_empty(),
        "fruit_a should be removed"
    );
    assert!(
        !sim.db
            .tree_fruits
            .by_position(&VoxelBox::point(fruit_b), tabulosity::QueryOpts::ASC)
            .is_empty(),
        "fruit_b should still be present"
    );
}

#[test]
fn determinism_after_stepping() {
    let seed = legacy_test_seed();
    let mut sim_a = test_sim(seed);
    let mut sim_b = test_sim(seed);

    let cmds = vec![SimCommand {
        player_name: String::new(),
        tick: 50,
        action: SimAction::SpawnCreature {
            zone_id: sim_a.home_zone_id(),
            species: Species::Elf,
            position: VoxelCoord::new(128, 1, 128),
        },
    }];

    sim_a.step(&cmds, 200);
    sim_b.step(&cmds, 200);

    assert_eq!(sim_a.tick, sim_b.tick);
    // Verify PRNG state is identical by drawing from both.
    assert_eq!(sim_a.rng.next_u64(), sim_b.rng.next_u64());
}

#[test]
fn new_sim_has_tree_voxels() {
    let sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert!(
        !tree.trunk_voxels.is_empty(),
        "Tree should have trunk voxels"
    );
    // Branch count varies by seed — some low-energy trees produce only trunk.
    // Just verify the tree has some geometry beyond a bare trunk.
    assert!(
        !tree.leaf_voxels.is_empty() || !tree.branch_voxels.is_empty(),
        "Tree should have branch or leaf voxels"
    );
}

// =========================================================================
// Flat world (treeless) helper tests
// =========================================================================

#[test]
fn flat_world_has_home_tree_row() {
    let sim = flat_world_sim(legacy_test_seed());
    assert!(sim.db.trees.contains(&sim.player_tree_id));
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert_eq!(tree.owner, sim.player_civ_id);
    // The tree row exists but has no voxels.
    assert!(tree.trunk_voxels.is_empty());
    assert!(tree.branch_voxels.is_empty());
    assert!(tree.leaf_voxels.is_empty());
}

#[test]
fn flat_world_can_spawn_creatures() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(creature.species, Species::Elf);
    // Creature should be on the ground (y = floor_y + 1 = 1).
    assert_eq!(creature.position.min.y, 1);
}

#[test]
fn flat_world_has_clear_air_above_ground() {
    let sim = flat_world_sim(legacy_test_seed());
    let center_x = sim.config.world_size.0 as i32 / 2;
    let center_z = sim.config.world_size.2 as i32 / 2;
    // Ground at floor_y should be solid.
    let ground = VoxelCoord::new(center_x, sim.config.floor_y, center_z);
    assert_ne!(
        sim.voxel_zone(sim.home_zone_id()).unwrap().get(ground),
        VoxelType::Air
    );
    // Everything above floor_y should be air.
    for y in (sim.config.floor_y + 1)..10 {
        let pos = VoxelCoord::new(center_x, y, center_z);
        assert_eq!(
            sim.voxel_zone(sim.home_zone_id()).unwrap().get(pos),
            VoxelType::Air,
            "Expected air at y={y}"
        );
    }
}

#[test]
fn flat_world_has_civilizations() {
    let sim = flat_world_sim(legacy_test_seed());
    assert!(sim.player_civ_id.is_some());
    assert!(sim.db.civilizations.len() > 1, "Should have multiple civs");
}

#[test]
fn flat_world_serde_roundtrip() {
    let sim = flat_world_sim(legacy_test_seed());
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();
    assert_eq!(sim.tick, restored.tick);
    assert_eq!(sim.player_tree_id, restored.player_tree_id);
    assert_eq!(sim.player_civ_id, restored.player_civ_id);
    assert_eq!(sim.db.civilizations.len(), restored.db.civilizations.len());
    // Tree row with no voxels survives roundtrip.
    let tree = restored.db.trees.get(&restored.player_tree_id).unwrap();
    assert!(tree.trunk_voxels.is_empty());
}

#[test]
fn flat_world_different_seeds_same_geometry() {
    let seed = legacy_test_seed();
    let sim_a = flat_world_sim(seed);
    let sim_b = flat_world_sim(seed + 957);
    // Terrain geometry is identical regardless of seed (terrain_max_height=0
    // means no PRNG calls during terrain generation).
    let center_x = sim_a.config.world_size.0 as i32 / 2;
    let center_z = sim_a.config.world_size.2 as i32 / 2;
    for y in 0..10 {
        let pos = VoxelCoord::new(center_x, y, center_z);
        assert_eq!(
            sim_a.voxel_zone(sim_a.home_zone_id()).unwrap().get(pos),
            sim_b.voxel_zone(sim_b.home_zone_id()).unwrap().get(pos),
            "Voxel at y={y} should be identical across seeds"
        );
    }
}

#[test]
fn flat_world_has_no_fruit_species() {
    let sim = flat_world_sim(legacy_test_seed());
    assert_eq!(
        sim.db.fruit_species.len(),
        0,
        "Flat world should not generate fruit species"
    );
}

// =========================================================================
// JSON roundtrip (to_json / from_json) tests
// =========================================================================

#[test]
fn json_roundtrip_preserves_state() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn creatures and advance ticks.
    let cmds = vec![
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                zone_id: sim.home_zone_id(),
                species: Species::Elf,
                position: tree_pos,
            },
        },
        SimCommand {
            player_name: String::new(),
            tick: 1,
            action: SimAction::SpawnCreature {
                zone_id: sim.home_zone_id(),
                species: Species::Capybara,
                position: tree_pos,
            },
        },
    ];
    sim.step(&cmds, 200);

    let restored = SimState::from_json(&sim.to_json().unwrap()).unwrap();

    assert_eq!(sim.tick, restored.tick);
    assert_eq!(sim.db.creatures.len(), restored.db.creatures.len());
    for creature in sim.db.creatures.iter_all() {
        let restored_creature = restored.db.creatures.get(&creature.id).unwrap();
        assert_eq!(creature.position, restored_creature.position);
        assert_eq!(creature.species, restored_creature.species);
        assert_eq!(creature.name, restored_creature.name);
        assert_eq!(creature.name_meaning, restored_creature.name_meaning);
    }
    assert_eq!(sim.player_tree_id, restored.player_tree_id);
    assert_eq!(sim.player_civ_id, restored.player_civ_id);
}

#[test]
fn json_roundtrip_continues_deterministically() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Spawn creatures and advance.
    let spawn = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Elf,
            position: tree_pos,
        },
    };
    sim.step(&[spawn], 200);

    // Save and restore.
    let mut restored = SimState::from_json(&sim.to_json().unwrap()).unwrap();

    // Advance both 500 more ticks.
    sim.step(&[], 700);
    restored.step(&[], 700);

    // Creature positions must match.
    for creature in sim.db.creatures.iter_all() {
        let restored_creature = restored.db.creatures.get(&creature.id).unwrap();
        assert_eq!(
            creature.position, restored_creature.position,
            "Creature {:?} position diverged after roundtrip + 500 ticks",
            creature.id
        );
    }
    // PRNG state must match.
    assert_eq!(sim.rng.next_u64(), restored.rng.next_u64());
}

#[test]
fn elf_spawned_after_roundtrip_gets_name() {
    let sim = test_sim(legacy_test_seed());
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;

    // Save and restore (no creatures yet).
    let mut restored = SimState::from_json(&sim.to_json().unwrap()).unwrap();

    // Spawn an elf after the roundtrip.
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Elf,
            position: tree_pos,
        },
    };
    restored.step(&[cmd], 2);

    let elf = restored
        .db
        .creatures
        .iter_all()
        .find(|c| c.species == Species::Elf)
        .expect("elf should exist after roundtrip spawn");
    assert!(
        !elf.name.is_empty(),
        "Elf spawned after save/load should still get a Vaelith name"
    );
}

#[test]
fn from_json_rejects_invalid_json() {
    let result = SimState::from_json("not valid json {{{");
    assert!(result.is_err());
}

#[test]
fn from_json_rejects_wrong_schema() {
    let result = SimState::from_json(r#"{"tick": "not_a_number"}"#);
    assert!(result.is_err());
}

// =========================================================================
// State checksum tests
// =========================================================================

#[test]
fn state_checksum_deterministic() {
    let seed = legacy_test_seed();
    let sim_a = test_sim(seed);
    let sim_b = test_sim(seed);
    let hash_a = sim_a.state_checksum();
    let hash_b = sim_b.state_checksum();
    assert_eq!(
        hash_a, hash_b,
        "same seed should produce identical checksum"
    );
    assert_ne!(hash_a, 0, "checksum should not be zero");
}

#[test]
fn state_checksum_different_seeds() {
    let seed = legacy_test_seed();
    let sim_a = test_sim(seed);
    let sim_b = test_sim(seed + 1);
    assert_ne!(
        sim_a.state_checksum(),
        sim_b.state_checksum(),
        "different seeds should produce different checksums"
    );
}

#[test]
fn state_checksum_changes_after_mutation() {
    let mut sim = test_sim(legacy_test_seed());
    let before = sim.state_checksum();

    // Spawn an elf to mutate state.
    let tree_pos = sim.db.trees.get(&sim.player_tree_id).unwrap().position;
    let spawn_pos = VoxelCoord::new(tree_pos.x, 1, tree_pos.z);
    let cmd = SimCommand {
        player_name: String::new(),
        tick: 1,
        action: SimAction::SpawnCreature {
            zone_id: sim.home_zone_id(),
            species: Species::Elf,
            position: spawn_pos,
        },
    };
    sim.step(&[cmd], 1);

    let after = sim.state_checksum();
    assert_ne!(
        before, after,
        "checksum should change after spawning a creature"
    );
}

// =========================================================================
// Spatial index tests (tabulosity R*-tree on Creature.position)
// =========================================================================

#[test]
fn spatial_index_empty_before_spawn() {
    let sim = test_sim(legacy_test_seed());
    assert!(
        sim.creatures_at_voxel(sim.home_zone_id(), VoxelCoord::new(0, 0, 0))
            .is_empty(),
        "Spatial index should be empty before any creatures are spawned"
    );
}

#[test]
fn spatial_index_populated_after_spawn() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let pos = elf.position.min;

    // Elf has a [1,1,1] footprint — should be found at its anchor voxel.
    let at_pos = sim.creatures_at_voxel(sim.home_zone_id(), pos);
    assert!(
        at_pos.contains(&elf_id),
        "Elf should be in spatial index at its position"
    );
    assert_eq!(at_pos.len(), 1, "Only one creature at this voxel");
}

#[test]
fn spatial_index_tracks_movement() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let initial_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;

    // Run enough ticks for the elf to move at least once.
    sim.step(&[], sim.tick + 50_000);
    let new_pos = sim.db.creatures.get(&elf_id).unwrap().position.min;

    if new_pos != initial_pos {
        assert!(
            !sim.creatures_at_voxel(sim.home_zone_id(), initial_pos)
                .contains(&elf_id),
            "Elf should not be at old position after moving"
        );
        assert!(
            sim.creatures_at_voxel(sim.home_zone_id(), new_pos)
                .contains(&elf_id),
            "Elf should be at new position after moving"
        );
    } else {
        assert!(
            sim.creatures_at_voxel(sim.home_zone_id(), initial_pos)
                .contains(&elf_id)
        );
    }
}

#[test]
fn spatial_index_multiple_creatures_same_voxel() {
    let mut sim = test_sim(legacy_test_seed());
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    // Force both elves to the same position.
    let pos1 = sim.db.creatures.get(&elf1).unwrap().position.min;
    force_position(&mut sim, elf2, pos1);

    let at_pos = sim.creatures_at_voxel(sim.home_zone_id(), pos1);
    assert!(at_pos.contains(&elf1));
    assert!(at_pos.contains(&elf2));
    assert_eq!(at_pos.len(), 2);
    // Verify sorted for determinism (tabulosity spatial queries sort by PK).
    assert!(
        at_pos[0] <= at_pos[1],
        "Spatial index entries should be sorted by CreatureId"
    );
}

#[test]
fn spatial_index_query_empty_voxel() {
    let sim = test_sim(legacy_test_seed());
    let empty = sim.creatures_at_voxel(sim.home_zone_id(), VoxelCoord::new(999, 999, 999));
    assert!(empty.is_empty());
}

#[test]
fn spatial_index_survives_save_load_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let pos = sim.db.creatures.get(&elf_id).unwrap().position.min;
    assert!(
        sim.creatures_at_voxel(sim.home_zone_id(), pos)
            .contains(&elf_id)
    );

    // Roundtrip through JSON. Tabulosity rebuilds the spatial index from
    // the deserialized position data automatically.
    let json = sim.to_json().unwrap();
    let sim2 = SimState::from_json(&json).unwrap();

    let elf2 = sim2.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim2.creatures_at_voxel(sim2.home_zone_id(), elf2.position.min)
            .contains(&elf_id),
        "Spatial index should be rebuilt after deserialization"
    );
}

#[test]
fn spatial_index_consistent_after_many_ticks() {
    let mut sim = test_sim(legacy_test_seed());
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);
    let elf3 = spawn_elf(&mut sim);

    sim.step(&[], sim.tick + 100_000);

    // Every creature must be findable via spatial query at its current position.
    for &elf_id in &[elf1, elf2, elf3] {
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.creatures_at_voxel(sim.home_zone_id(), elf.position.min)
                .contains(&elf_id),
            "Creature {:?} should be at its position {:?}",
            elf_id,
            elf.position,
        );
    }
}

// =========================================================================
// Multi-voxel spatial index tests
// =========================================================================

#[test]
fn spatial_index_multi_voxel_creature_found_at_all_footprint_voxels() {
    let mut sim = test_sim(fresh_test_seed());
    let troll = spawn_creature(&mut sim, Species::Troll);
    let fp = sim.species_table[&Species::Troll].footprint;
    assert!(
        fp[0] > 1 || fp[1] > 1 || fp[2] > 1,
        "Troll should have multi-voxel footprint"
    );

    let anchor = sim.db.creatures.get(&troll).unwrap().position.min;

    // The troll should be found at every voxel in its footprint.
    for dx in 0..fp[0] as i32 {
        for dy in 0..fp[1] as i32 {
            for dz in 0..fp[2] as i32 {
                let voxel = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                assert!(
                    sim.creatures_at_voxel(sim.home_zone_id(), voxel)
                        .contains(&troll),
                    "Troll should be found at footprint voxel offset ({dx}, {dy}, {dz})"
                );
            }
        }
    }

    // Should NOT be found at an adjacent voxel outside the footprint.
    let outside = VoxelCoord::new(anchor.x + fp[0] as i32, anchor.y, anchor.z);
    assert!(
        !sim.creatures_at_voxel(sim.home_zone_id(), outside)
            .contains(&troll),
        "Troll should not be found outside its footprint"
    );
}

#[test]
fn spatial_index_multi_voxel_creature_tracks_movement() {
    let mut sim = test_sim(fresh_test_seed());
    let troll = spawn_creature(&mut sim, Species::Troll);
    let fp = sim.species_table[&Species::Troll].footprint;
    let old_anchor = sim.db.creatures.get(&troll).unwrap().position.min;

    // Move the troll to a new position.
    let new_anchor = VoxelCoord::new(old_anchor.x + 5, old_anchor.y, old_anchor.z + 5);
    force_position(&mut sim, troll, new_anchor);

    // Should be at all new footprint voxels.
    for dx in 0..fp[0] as i32 {
        for dy in 0..fp[1] as i32 {
            for dz in 0..fp[2] as i32 {
                let voxel =
                    VoxelCoord::new(new_anchor.x + dx, new_anchor.y + dy, new_anchor.z + dz);
                assert!(
                    sim.creatures_at_voxel(sim.home_zone_id(), voxel)
                        .contains(&troll),
                    "Troll should be at new footprint voxel offset ({dx}, {dy}, {dz})"
                );
            }
        }
    }

    // Should NOT be at any old footprint voxels (assuming no overlap).
    for dx in 0..fp[0] as i32 {
        for dy in 0..fp[1] as i32 {
            for dz in 0..fp[2] as i32 {
                let voxel =
                    VoxelCoord::new(old_anchor.x + dx, old_anchor.y + dy, old_anchor.z + dz);
                assert!(
                    !sim.creatures_at_voxel(sim.home_zone_id(), voxel)
                        .contains(&troll),
                    "Troll should not be at old footprint voxel offset ({dx}, {dy}, {dz})"
                );
            }
        }
    }
}

#[test]
fn spawn_creature_sets_correct_voxel_box_footprint() {
    let mut sim = test_sim(fresh_test_seed());
    // Check several species.
    for species in [Species::Elf, Species::Troll, Species::Elephant] {
        let id = spawn_creature(&mut sim, species);
        let creature = sim.db.creatures.get(&id).unwrap();
        let expected_fp = sim.species_table[&species].footprint;
        assert_eq!(
            creature.position.footprint_size(),
            expected_fp,
            "Species {:?} should have footprint {:?} but got {:?}",
            species,
            expected_fp,
            creature.position.footprint_size(),
        );
    }
}

#[test]
fn upgrade_creature_positions_fixes_multi_voxel_from_legacy_save() {
    let mut sim = test_sim(fresh_test_seed());
    let troll = spawn_creature(&mut sim, Species::Troll);
    let anchor = sim.db.creatures.get(&troll).unwrap().position.min;
    let expected_fp = sim.species_table[&Species::Troll].footprint;

    // Simulate a legacy save by shrinking position to a 1x1x1 point.
    {
        let mut c = sim.db.creatures.get(&troll).unwrap();
        c.position = VoxelBox::point(anchor);
        sim.db.update_creature(c).unwrap();
    }
    assert_eq!(
        sim.db
            .creatures
            .get(&troll)
            .unwrap()
            .position
            .footprint_size(),
        [1, 1, 1]
    );

    // Roundtrip through JSON to trigger rebuild_transient_state.
    let json = sim.to_json().unwrap();
    let sim2 = SimState::from_json(&json).unwrap();

    // Position should be upgraded to the correct footprint.
    let restored = sim2.db.creatures.get(&troll).unwrap();
    assert_eq!(
        restored.position.footprint_size(),
        expected_fp,
        "Troll position should be upgraded from 1x1x1 to {:?}",
        expected_fp,
    );
    assert_eq!(restored.position.min, anchor, "Anchor should be preserved");

    // Should be findable via spatial query at all footprint voxels.
    for dx in 0..expected_fp[0] as i32 {
        for dy in 0..expected_fp[1] as i32 {
            for dz in 0..expected_fp[2] as i32 {
                let voxel = VoxelCoord::new(anchor.x + dx, anchor.y + dy, anchor.z + dz);
                assert!(
                    sim2.creatures_at_voxel(sim2.home_zone_id(), voxel)
                        .contains(&troll),
                    "Troll should be in spatial index at upgraded footprint voxel ({dx}, {dy}, {dz})"
                );
            }
        }
    }
}

// =========================================================================
// Config backward compat
// =========================================================================

#[test]
fn config_backward_compat_missing_material_filter() {
    // Verify that default_config.json without material_filter in elf_default_wants
    // deserializes correctly (covered by the `#[serde(default)]` on the field).
    let json = r#"{"item_kind":"Bread","target_quantity":2}"#;
    let want: building::LogisticsWant = serde_json::from_str(json).unwrap();
    assert_eq!(want.material_filter, inventory::MaterialFilter::Any);
}

// ---------------------------------------------------------------------------
// Tests migrated from commands_tests.rs
// ---------------------------------------------------------------------------

// -----------------------------------------------------------------------
// Save/load roundtrip tests
// -----------------------------------------------------------------------

#[test]
fn save_load_preserves_world_voxels() {
    let sim = test_sim(legacy_test_seed());
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();

    // Roundtrip through JSON (world is now serialized, not rebuilt).
    let json = sim.to_json().unwrap();
    let restored = SimState::from_json(&json).unwrap();

    // Check trunk voxels survived serialization.
    for coord in &tree.trunk_voxels {
        assert_eq!(
            restored
                .voxel_zone(restored.home_zone_id())
                .unwrap()
                .get(*coord),
            VoxelType::Trunk,
            "Restored world missing trunk voxel at {coord}"
        );
    }
    // Check branch voxels.
    for coord in &tree.branch_voxels {
        assert_eq!(
            restored
                .voxel_zone(restored.home_zone_id())
                .unwrap()
                .get(*coord),
            VoxelType::Branch,
            "Restored world missing branch voxel at {coord}"
        );
    }
    // Check root voxels.
    for coord in &tree.root_voxels {
        assert_eq!(
            restored
                .voxel_zone(restored.home_zone_id())
                .unwrap()
                .get(*coord),
            VoxelType::Root,
            "Restored world missing root voxel at {coord}"
        );
    }
    // Check leaf voxels.
    for coord in &tree.leaf_voxels {
        assert_eq!(
            restored
                .voxel_zone(restored.home_zone_id())
                .unwrap()
                .get(*coord),
            VoxelType::Leaf,
            "Restored world missing leaf voxel at {coord}"
        );
    }
    // Check that a known solid voxel (first trunk) survived.
    let first_trunk = tree.trunk_voxels[0];
    assert_eq!(
        restored
            .voxel_zone(restored.home_zone_id())
            .unwrap()
            .get(first_trunk),
        VoxelType::Trunk,
        "First trunk voxel should be present after roundtrip"
    );
}

// -----------------------------------------------------------------------
// find_surface_position
// -----------------------------------------------------------------------

#[test]
fn find_surface_position_finds_air() {
    let sim = test_sim(legacy_test_seed());
    let center = sim.voxel_zone(sim.home_zone_id()).unwrap().size_x as i32 / 2;
    let pos = sim.find_surface_position(center, center, sim.home_zone_id());

    // The returned position should be Air (non-solid).
    assert!(
        !sim.voxel_zone(sim.home_zone_id())
            .unwrap()
            .get(pos)
            .is_solid(),
        "Surface position should be Air, got {:?}",
        sim.voxel_zone(sim.home_zone_id()).unwrap().get(pos)
    );

    // One below should be solid (the ground).
    if pos.y > 0 {
        let below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
        assert!(
            sim.voxel_zone(sim.home_zone_id())
                .unwrap()
                .get(below)
                .is_solid(),
            "Below surface should be solid, got {:?}",
            sim.voxel_zone(sim.home_zone_id()).unwrap().get(below)
        );
    }
}
