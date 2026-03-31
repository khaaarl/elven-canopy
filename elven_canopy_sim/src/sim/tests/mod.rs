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
mod grazing_tests;
mod greenhouse_tests;
mod hp_tests;
mod inventory_tests;
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

// =========================================================================
// Core tests: basic sim setup, determinism, serialization, tree, nav graph
// =========================================================================

#[test]
fn new_sim_has_home_tree() {
    let sim = test_sim(legacy_test_seed());
    assert!(sim.db.trees.contains(&sim.player_tree_id));
    let tree = sim.db.trees.get(&sim.player_tree_id).unwrap();
    assert_eq!(tree.owner, sim.player_civ_id);
    let great_info = sim.db.great_tree_infos.get(&sim.player_tree_id).unwrap();
    assert_eq!(great_info.mana_stored, sim.config.starting_mana_mm);
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
    assert_eq!(sim.world.sim_tick, 0);
    sim.step(&[], 10);
    assert_eq!(sim.world.sim_tick, 10);
    sim.step(&[], 25);
    assert_eq!(sim.world.sim_tick, 25);
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
    assert_eq!(rt.fruit_positions, orig_tree.fruit_positions);
    assert_eq!(rt.fruit_species_id, orig_tree.fruit_species_id);

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
fn remove_fruit_from_trees_no_op_when_not_found() {
    let mut sim = test_sim(legacy_test_seed());
    let bogus_pos = VoxelCoord::new(0, 0, 0);
    // Capture fruit count before the call.
    let before: usize = sim
        .db
        .trees
        .iter_all()
        .map(|t| t.fruit_positions.len())
        .sum();
    // Should not panic on a position no tree owns.
    sim.remove_fruit_from_trees(bogus_pos);
    let after: usize = sim
        .db
        .trees
        .iter_all()
        .map(|t| t.fruit_positions.len())
        .sum();
    assert_eq!(before, after, "No fruit should have been removed");
}

#[test]
fn remove_fruit_from_trees_removes_correct_fruit() {
    let mut sim = test_sim(legacy_test_seed());
    let tree_id = sim.player_tree_id;

    // Manually add two fruit positions to the home tree.
    let fruit_a = VoxelCoord::new(10, 60, 10);
    let fruit_b = VoxelCoord::new(11, 60, 11);
    let mut tree = sim.db.trees.get(&tree_id).unwrap().clone();
    tree.fruit_positions.push(fruit_a);
    tree.fruit_positions.push(fruit_b);
    sim.db.update_tree(tree).unwrap();

    // Remove fruit_a — fruit_b should remain.
    sim.remove_fruit_from_trees(fruit_a);

    let tree = sim.db.trees.get(&tree_id).unwrap();
    assert!(
        !tree.fruit_positions.contains(&fruit_a),
        "fruit_a should be removed"
    );
    assert!(
        tree.fruit_positions.contains(&fruit_b),
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

#[test]
fn new_sim_has_nav_graph() {
    let sim = test_sim(legacy_test_seed());
    assert!(
        sim.nav_graph.node_count() > 0,
        "Nav graph should have nodes"
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
fn flat_world_has_nav_graph() {
    let sim = flat_world_sim(legacy_test_seed());
    assert!(
        sim.nav_graph.node_count() > 0,
        "Flat world should have nav nodes on the ground"
    );
}

#[test]
fn flat_world_can_spawn_creatures() {
    let mut sim = flat_world_sim(legacy_test_seed());
    let elf = spawn_elf(&mut sim);
    let creature = sim.db.creatures.get(&elf).unwrap();
    assert_eq!(creature.species, Species::Elf);
    // Creature should be on the ground (y = floor_y + 1 = 1).
    assert_eq!(creature.position.y, 1);
}

#[test]
fn flat_world_has_clear_air_above_ground() {
    let sim = flat_world_sim(legacy_test_seed());
    let center_x = sim.config.world_size.0 as i32 / 2;
    let center_z = sim.config.world_size.2 as i32 / 2;
    // Ground at floor_y should be solid.
    let ground = VoxelCoord::new(center_x, sim.config.floor_y, center_z);
    assert_ne!(sim.world.get(ground), VoxelType::Air);
    // Everything above floor_y should be air.
    for y in (sim.config.floor_y + 1)..10 {
        let pos = VoxelCoord::new(center_x, y, center_z);
        assert_eq!(sim.world.get(pos), VoxelType::Air, "Expected air at y={y}");
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
            sim_a.world.get(pos),
            sim_b.world.get(pos),
            "Voxel at y={y} should be identical across seeds"
        );
    }
    // Nav graph should have the same topology.
    assert_eq!(sim_a.nav_graph.node_count(), sim_b.nav_graph.node_count());
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
// Spatial index tests
// =========================================================================

#[test]
fn spatial_index_empty_before_spawn() {
    let sim = test_sim(legacy_test_seed());
    assert!(
        sim.spatial_index.is_empty(),
        "Spatial index should be empty before any creatures are spawned"
    );
}

#[test]
fn spatial_index_populated_after_spawn() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let elf = sim.db.creatures.get(&elf_id).unwrap();
    let pos = elf.position;

    // Elf has a [1,1,1] footprint — should be registered at exactly one voxel.
    let at_pos = sim.creatures_at_voxel(pos);
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
    let initial_pos = sim.db.creatures.get(&elf_id).unwrap().position;

    // Run enough ticks for the elf to move at least once.
    sim.step(&[], sim.tick + 50_000);
    let new_pos = sim.db.creatures.get(&elf_id).unwrap().position;

    if new_pos != initial_pos {
        assert!(
            !sim.creatures_at_voxel(initial_pos).contains(&elf_id),
            "Elf should not be at old position after moving"
        );
        assert!(
            sim.creatures_at_voxel(new_pos).contains(&elf_id),
            "Elf should be at new position after moving"
        );
    } else {
        assert!(sim.creatures_at_voxel(initial_pos).contains(&elf_id));
    }
}

#[test]
fn spatial_index_multiple_creatures_same_voxel() {
    let mut sim = test_sim(legacy_test_seed());
    let elf1 = spawn_elf(&mut sim);
    let elf2 = spawn_elf(&mut sim);

    // Force both elves to the same position so the test always exercises
    // multi-occupancy (spawn may place them at different nav nodes).
    let pos1 = sim.db.creatures.get(&elf1).unwrap().position;
    let pos2 = sim.db.creatures.get(&elf2).unwrap().position;
    if pos1 != pos2 {
        let species = sim.db.creatures.get(&elf2).unwrap().species;
        let footprint = sim.species_table[&species].footprint;
        SimState::deregister_creature_from_index(&mut sim.spatial_index, elf2, pos2, footprint);
        let mut c = sim.db.creatures.get(&elf2).unwrap().clone();
        c.position = pos1;
        sim.db.update_creature(c).unwrap();
        SimState::register_creature_in_index(&mut sim.spatial_index, elf2, pos1, footprint);
    }

    let at_pos = sim.creatures_at_voxel(pos1);
    assert!(at_pos.contains(&elf1));
    assert!(at_pos.contains(&elf2));
    assert_eq!(at_pos.len(), 2);
    // Verify sorted for determinism.
    assert!(
        at_pos[0] <= at_pos[1],
        "Spatial index entries should be sorted by CreatureId"
    );
}

#[test]
fn spatial_index_query_empty_voxel() {
    let sim = test_sim(legacy_test_seed());
    let empty = sim.creatures_at_voxel(VoxelCoord::new(999, 999, 999));
    assert!(empty.is_empty());
}

#[test]
fn spatial_index_survives_save_load_roundtrip() {
    let mut sim = test_sim(legacy_test_seed());
    let elf_id = spawn_elf(&mut sim);
    let pos = sim.db.creatures.get(&elf_id).unwrap().position;
    assert!(sim.creatures_at_voxel(pos).contains(&elf_id));

    // Roundtrip through JSON (spatial_index is #[serde(skip)]).
    let json = sim.to_json().unwrap();
    let sim2 = SimState::from_json(&json).unwrap();

    let elf2 = sim2.db.creatures.get(&elf_id).unwrap();
    assert!(
        sim2.creatures_at_voxel(elf2.position).contains(&elf_id),
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

    // Every creature must be in the index at its current position.
    for &elf_id in &[elf1, elf2, elf3] {
        let elf = sim.db.creatures.get(&elf_id).unwrap();
        assert!(
            sim.creatures_at_voxel(elf.position).contains(&elf_id),
            "Creature {:?} should be at its position {:?}",
            elf_id,
            elf.position,
        );
    }

    // Total entries should match total footprint voxels.
    let total_entries: usize = sim.spatial_index.values().map(|v| v.len()).sum();
    let expected: usize = sim
        .db
        .creatures
        .iter_all()
        .map(|c| {
            let fp = sim.species_table[&c.species].footprint;
            fp[0] as usize * fp[1] as usize * fp[2] as usize
        })
        .sum();
    assert_eq!(
        total_entries, expected,
        "Spatial index entry count should match total footprint voxels"
    );
}

#[test]
fn spatial_index_multi_voxel_footprint() {
    let mut index = BTreeMap::<VoxelCoord, Vec<CreatureId>>::new();
    let mut rng = GameRng::new(999);
    let cid = CreatureId::new(&mut rng);
    let anchor = VoxelCoord::new(5, 1, 5);
    let footprint = [2, 2, 2];

    SimState::register_creature_in_index(&mut index, cid, anchor, footprint);

    // Should be registered at 8 voxels (2x2x2).
    let mut registered_count = 0;
    for dx in 0..2 {
        for dy in 0..2 {
            for dz in 0..2 {
                let v = VoxelCoord::new(5 + dx, 1 + dy, 5 + dz);
                assert!(
                    index.get(&v).unwrap().contains(&cid),
                    "Creature should be at ({}, {}, {})",
                    5 + dx,
                    1 + dy,
                    5 + dz,
                );
                registered_count += 1;
            }
        }
    }
    assert_eq!(registered_count, 8);

    SimState::deregister_creature_from_index(&mut index, cid, anchor, footprint);
    assert!(index.is_empty(), "Index should be empty after deregister");
}

#[test]
fn spatial_index_sorted_entries() {
    let mut index = BTreeMap::<VoxelCoord, Vec<CreatureId>>::new();
    let pos = VoxelCoord::new(5, 1, 5);
    let fp = [1, 1, 1];

    let mut rng = GameRng::new(12345);
    let mut ids = [
        CreatureId::new(&mut rng),
        CreatureId::new(&mut rng),
        CreatureId::new(&mut rng),
    ];
    ids.sort();

    // Register in reverse order.
    SimState::register_creature_in_index(&mut index, ids[2], pos, fp);
    SimState::register_creature_in_index(&mut index, ids[0], pos, fp);
    SimState::register_creature_in_index(&mut index, ids[1], pos, fp);

    let entries = &index[&pos];
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0], ids[0]);
    assert_eq!(entries[1], ids[1]);
    assert_eq!(entries[2], ids[2]);
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
            restored.world.get(*coord),
            VoxelType::Trunk,
            "Restored world missing trunk voxel at {coord}"
        );
    }
    // Check branch voxels.
    for coord in &tree.branch_voxels {
        assert_eq!(
            restored.world.get(*coord),
            VoxelType::Branch,
            "Restored world missing branch voxel at {coord}"
        );
    }
    // Check root voxels.
    for coord in &tree.root_voxels {
        assert_eq!(
            restored.world.get(*coord),
            VoxelType::Root,
            "Restored world missing root voxel at {coord}"
        );
    }
    // Check leaf voxels.
    for coord in &tree.leaf_voxels {
        assert_eq!(
            restored.world.get(*coord),
            VoxelType::Leaf,
            "Restored world missing leaf voxel at {coord}"
        );
    }
    // Check that a known solid voxel (first trunk) survived.
    let first_trunk = tree.trunk_voxels[0];
    assert_eq!(
        restored.world.get(first_trunk),
        VoxelType::Trunk,
        "First trunk voxel should be present after roundtrip"
    );
}

#[test]
fn rebuild_transient_state_restores_nav_graph() {
    let sim = test_sim(legacy_test_seed());
    let json = sim.to_json().unwrap();

    // Deserialize — world is preserved but transient fields are default.
    let mut restored: SimState = serde_json::from_str(&json).unwrap();
    assert_eq!(
        restored.nav_graph.node_count(),
        0,
        "Before rebuild, nav_graph should be empty"
    );
    // World is now serialized, so it should be present after deserialization.
    assert_eq!(
        restored.world.size_x, sim.world.size_x,
        "After deserialization, world should be present"
    );

    // Rebuild transient state.
    restored.rebuild_transient_state();
    assert!(
        restored.nav_graph.node_count() > 0,
        "After rebuild, nav_graph should have nodes"
    );
    // Node count may differ very slightly because fruit voxels are placed
    // after the initial nav graph build but before serialization, so the
    // rebuilt world includes fruit while the original nav graph was built
    // without them. Allow a small tolerance.
    let diff =
        (restored.nav_graph.node_count() as i64 - sim.nav_graph.node_count() as i64).unsigned_abs();
    assert!(
        diff <= 5,
        "Rebuilt nav_graph node count ({}) should be close to original ({}), diff={}",
        restored.nav_graph.node_count(),
        sim.nav_graph.node_count(),
        diff,
    );
}

// -----------------------------------------------------------------------
// find_surface_position
// -----------------------------------------------------------------------

#[test]
fn find_surface_position_finds_air() {
    let sim = test_sim(legacy_test_seed());
    let center = sim.world.size_x as i32 / 2;
    let pos = sim.find_surface_position(center, center);

    // The returned position should be Air (non-solid).
    assert!(
        !sim.world.get(pos).is_solid(),
        "Surface position should be Air, got {:?}",
        sim.world.get(pos)
    );

    // One below should be solid (the ground).
    if pos.y > 0 {
        let below = VoxelCoord::new(pos.x, pos.y - 1, pos.z);
        assert!(
            sim.world.get(below).is_solid(),
            "Below surface should be solid, got {:?}",
            sim.world.get(below)
        );
    }
}
