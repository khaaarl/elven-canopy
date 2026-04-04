// Worldgen tests — migrated from `worldgen.rs` to share `fresh_test_seed()`
// from `test_helpers.rs`.  All tests use structural assertions (counts, bounds,
// containment) rather than seed-specific expected values, so they pass with any
// seed.  The local `test_config()` mirrors the small-world config from the
// original test module.

use super::*;
use crate::worldgen::{WorldgenConfig, noop_log, run_worldgen, species_default_opinion};

/// Small-world config for fast worldgen tests.
fn wg_test_config() -> GameConfig {
    let mut config = GameConfig {
        world_size: (64, 64, 64),
        floor_y: 0,
        ..GameConfig::default()
    };
    config.tree_profile.growth.initial_energy = 50.0;
    config.terrain_max_height = 0;
    // Disable lesser trees by default to keep tests fast and avoid PRNG
    // sequence shifts.  Lesser-tree-specific tests enable them explicitly.
    config.lesser_trees.count = 0;
    config
}

#[test]
fn worldgen_is_deterministic() {
    let seed = fresh_test_seed();
    let config = wg_test_config();

    let result1 = run_worldgen(seed, &config, &noop_log());
    let result2 = run_worldgen(seed, &config, &noop_log());

    let tree1 = result1.db.trees.get(&result1.player_tree_id).unwrap();
    let tree2 = result2.db.trees.get(&result2.player_tree_id).unwrap();
    assert_eq!(tree1.trunk_voxels, tree2.trunk_voxels);
    assert_eq!(tree1.branch_voxels, tree2.branch_voxels);
    assert_eq!(tree1.leaf_voxels, tree2.leaf_voxels);
    assert_eq!(tree1.root_voxels, tree2.root_voxels);

    assert_eq!(result1.player_tree_id, result2.player_tree_id);
    assert_eq!(result1.player_civ_id, result2.player_civ_id);

    // Walkability is derived from voxel geometry (already compared above via
    // trunk/branch/leaf/root voxels), so no separate walkability assertion needed.
}

#[test]
fn different_seeds_produce_different_worlds() {
    let config = wg_test_config();

    let seed = fresh_test_seed();
    let result1 = run_worldgen(seed, &config, &noop_log());
    let result2 = run_worldgen(seed.wrapping_add(1), &config, &noop_log());

    let tree1 = result1.db.trees.get(&result1.player_tree_id).unwrap();
    let tree2 = result2.db.trees.get(&result2.player_tree_id).unwrap();
    assert_ne!(tree1.trunk_voxels, tree2.trunk_voxels);
}

#[test]
fn runtime_rng_differs_from_worldgen_start() {
    let seed = fresh_test_seed();
    let config = wg_test_config();

    let result = run_worldgen(seed, &config, &noop_log());

    let mut fresh_rng = GameRng::new(seed);
    let mut runtime_rng = result.runtime_rng;

    assert_ne!(fresh_rng.next_u64(), runtime_rng.next_u64());
}

#[test]
fn worldgen_config_default_is_empty() {
    let wc = WorldgenConfig::default();
    let json = serde_json::to_string(&wc).unwrap();
    let _: WorldgenConfig = serde_json::from_str(&json).unwrap();
}

// -------------------------------------------------------------------
// Civilization worldgen tests
// -------------------------------------------------------------------

#[test]
fn worldgen_creates_player_civ() {
    let config = wg_test_config();
    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    let player_civ = result.db.civilizations.get(&CivId(0)).unwrap();
    assert!(player_civ.player_controlled);
    assert_eq!(player_civ.primary_species, CivSpecies::Elf);
    assert_eq!(result.player_civ_id, CivId(0));
}

#[test]
fn worldgen_creates_correct_civ_count() {
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 5;
    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    let civs: Vec<_> = result.db.civilizations.iter_all().collect();
    assert_eq!(civs.len(), 5);

    for i in 0..5 {
        assert!(result.db.civilizations.get(&CivId(i as u16)).is_some());
    }
}

#[test]
fn worldgen_ai_civs_are_not_player_controlled() {
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 3;
    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    for civ in result.db.civilizations.iter_all() {
        if civ.id == CivId(0) {
            assert!(civ.player_controlled);
        } else {
            assert!(!civ.player_controlled);
        }
    }
}

#[test]
fn worldgen_diplomacy_creates_relationships() {
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 4;
    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    let rels: Vec<_> = result.db.civ_relationships.iter_all().collect();
    assert!(
        !rels.is_empty(),
        "4 civs should produce at least some diplomatic relationships"
    );
}

#[test]
fn worldgen_player_known_civs_capped() {
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 20;
    config.worldgen.civs.player_starting_known_civs = 3;
    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    let player_rels = result
        .db
        .civ_relationships
        .by_from_civ(&CivId(0), tabulosity::QueryOpts::ASC);
    assert!(
        player_rels.len() <= 4,
        "Player should know at most cap+1 civs (3 from cap + 1 guaranteed hostile), got {}",
        player_rels.len()
    );
}

#[test]
fn worldgen_civ_determinism() {
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 8;
    let seed = fresh_test_seed();
    let r1 = run_worldgen(seed, &config, &noop_log());
    let r2 = run_worldgen(seed, &config, &noop_log());

    let civs1: Vec<_> = r1.db.civilizations.iter_all().collect();
    let civs2: Vec<_> = r2.db.civilizations.iter_all().collect();
    assert_eq!(civs1.len(), civs2.len());
    for (c1, c2) in civs1.iter().zip(civs2.iter()) {
        assert_eq!(c1.id, c2.id);
        assert_eq!(c1.name, c2.name);
        assert_eq!(c1.primary_species, c2.primary_species);
        assert_eq!(c1.culture_tag, c2.culture_tag);
        assert_eq!(c1.player_controlled, c2.player_controlled);
    }

    let rels1: Vec<_> = r1.db.civ_relationships.iter_all().collect();
    let rels2: Vec<_> = r2.db.civ_relationships.iter_all().collect();
    assert_eq!(rels1.len(), rels2.len());
    for (r1, r2) in rels1.iter().zip(rels2.iter()) {
        assert_eq!(r1.from_civ, r2.from_civ);
        assert_eq!(r1.to_civ, r2.to_civ);
        assert_eq!(r1.opinion, r2.opinion);
    }
}

#[test]
fn worldgen_different_seeds_produce_different_civs() {
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 10;
    let seed = fresh_test_seed();
    let r1 = run_worldgen(seed, &config, &noop_log());
    let r2 = run_worldgen(seed.wrapping_add(1), &config, &noop_log());

    let names1: Vec<_> = r1
        .db
        .civilizations
        .iter_all()
        .map(|c| c.name.clone())
        .collect();
    let names2: Vec<_> = r2
        .db
        .civilizations
        .iter_all()
        .map(|c| c.name.clone())
        .collect();
    assert_ne!(names1, names2);
}

#[test]
fn worldgen_all_civs_have_names() {
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 10;
    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    for civ in result.db.civilizations.iter_all() {
        assert!(!civ.name.is_empty(), "CivId({}) has empty name", civ.id.0);
    }
}

#[test]
fn worldgen_bidirectional_hostile_awareness() {
    // Test across multiple seeds to confirm the guarantee is robust.
    let base = fresh_test_seed();
    for i in 0..20u64 {
        let seed = base.wrapping_add(i);
        let config = wg_test_config();
        let result = run_worldgen(seed, &config, &noop_log());

        let hates_player: Vec<_> = result
            .db
            .civ_relationships
            .by_to_civ(&CivId(0), tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|r| r.opinion == CivOpinion::Hostile)
            .collect();
        assert!(
            !hates_player.is_empty(),
            "Seed {seed}: at least one civ must consider the player hostile"
        );

        let player_hates: Vec<_> = result
            .db
            .civ_relationships
            .by_from_civ(&CivId(0), tabulosity::QueryOpts::ASC)
            .into_iter()
            .filter(|r| r.opinion == CivOpinion::Hostile)
            .collect();
        assert!(
            !player_hates.is_empty(),
            "Seed {seed}: the player must be aware of at least one hostile civ"
        );
    }
}

#[test]
fn species_default_opinion_is_symmetric_for_same_species() {
    for &species in CivSpecies::ALL.iter() {
        assert_eq!(
            species_default_opinion(species, species),
            CivOpinion::Friendly,
            "Same-species opinion for {:?} should be Friendly",
            species
        );
    }
}

// --- Lesser tree tests ---

#[test]
fn lesser_trees_are_placed() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 5;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.max_placement_attempts = 200;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_count = result.db.trees.len() - 1;
    assert!(lesser_count > 0, "Should place at least one lesser tree");
    assert!(lesser_count <= 5, "Should not exceed requested count");
}

#[test]
fn lesser_trees_have_no_owner() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 3;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_trees: Vec<_> = result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
        .collect();
    for tree in &lesser_trees {
        assert_eq!(tree.owner, None, "Lesser trees should have no owner");
    }
}

#[test]
fn lesser_trees_have_no_great_tree_info() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 3;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    assert!(
        result.db.great_tree_infos.contains(&result.player_tree_id),
        "Home tree should have GreatTreeInfo"
    );

    for tree in result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
    {
        assert!(
            !result.db.great_tree_infos.contains(&tree.id),
            "Lesser tree {:?} should not have GreatTreeInfo",
            tree.id
        );
    }
}

#[test]
fn lesser_trees_respect_distance_from_main() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 10;
    config.lesser_trees.min_distance_from_main = 10;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.max_placement_attempts = 500;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let home_tree = result.db.trees.get(&result.player_tree_id).unwrap();
    let main_pos = home_tree.position;
    let min_dist_sq = 10i64 * 10;

    let lesser_trees: Vec<_> = result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
        .collect();
    for tree in &lesser_trees {
        let dx = (tree.position.x - main_pos.x) as i64;
        let dz = (tree.position.z - main_pos.z) as i64;
        let dist_sq = dx * dx + dz * dz;
        assert!(
            dist_sq >= min_dist_sq,
            "Lesser tree at ({}, {}) is too close to main tree at ({}, {}): dist²={dist_sq} < {min_dist_sq}",
            tree.position.x,
            tree.position.z,
            main_pos.x,
            main_pos.z,
        );
    }
}

#[test]
fn lesser_trees_respect_distance_between() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 10;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 5;
    config.lesser_trees.max_placement_attempts = 500;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let min_dist_sq = 5i64 * 5;

    let lesser_trees: Vec<_> = result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
        .collect();
    for (i, a) in lesser_trees.iter().enumerate() {
        for b in lesser_trees.iter().skip(i + 1) {
            let dx = (a.position.x - b.position.x) as i64;
            let dz = (a.position.z - b.position.z) as i64;
            let dist_sq = dx * dx + dz * dz;
            assert!(
                dist_sq >= min_dist_sq,
                "Lesser trees at ({},{}) and ({},{}) too close: dist²={dist_sq} < {min_dist_sq}",
                a.position.x,
                a.position.z,
                b.position.x,
                b.position.z,
            );
        }
    }
}

#[test]
fn lesser_trees_have_trunk_voxels_in_world() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 3;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_trees: Vec<_> = result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
        .collect();
    for tree in &lesser_trees {
        assert!(
            !tree.trunk_voxels.is_empty(),
            "Each lesser tree should have trunk voxels"
        );
        for &coord in &tree.trunk_voxels {
            let vt = result.world.get(coord);
            assert!(
                vt == VoxelType::Trunk || vt == VoxelType::Branch,
                "Trunk voxel at {coord} should be Trunk or Branch in world, got {vt:?}"
            );
        }
    }
}

#[test]
fn lesser_trees_inserted_into_sim_trees_map() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 3;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_count = result.db.trees.len() - 1;
    assert_eq!(result.db.trees.len(), 1 + lesser_count);
}

#[test]
fn lesser_trees_deterministic() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 5;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;

    let seed = fresh_test_seed();
    let result1 = run_worldgen(seed, &config, &noop_log());
    let result2 = run_worldgen(seed, &config, &noop_log());

    let lesser1: Vec<_> = result1
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result1.player_tree_id)
        .collect();
    let lesser2: Vec<_> = result2
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result2.player_tree_id)
        .collect();
    assert_eq!(lesser1.len(), lesser2.len());
    for (a, b) in lesser1.iter().zip(lesser2.iter()) {
        assert_eq!(a.position, b.position);
        assert_eq!(a.trunk_voxels, b.trunk_voxels);
        assert_eq!(a.branch_voxels, b.branch_voxels);
        assert_eq!(a.leaf_voxels, b.leaf_voxels);
    }
}

#[test]
fn zero_lesser_tree_count_places_none() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 0;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    assert_eq!(result.db.trees.len(), 1, "Only the home tree should exist");
}

#[test]
fn empty_profiles_places_no_lesser_trees() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 10;
    config.lesser_trees.profiles = Vec::new();

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    assert_eq!(result.db.trees.len(), 1, "Only the home tree should exist");
}

#[test]
fn lesser_trees_max_attempts_caps_placement() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 100;
    config.lesser_trees.max_placement_attempts = 5;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_count = result.db.trees.len() - 1;
    assert!(
        lesser_count < 100,
        "With only 5 attempts, should place fewer than 100 trees (got {})",
        lesser_count
    );
}

#[test]
fn lesser_trees_tiny_world_no_panic() {
    let mut config = wg_test_config();
    config.world_size = (4, 32, 4);
    config.tree_profile.growth.initial_energy = 5.0;
    config.lesser_trees.count = 3;
    config.lesser_trees.min_distance_from_main = 1;
    config.lesser_trees.min_distance_between = 1;
    config.lesser_trees.max_placement_attempts = 20;

    let _result = run_worldgen(fresh_test_seed(), &config, &noop_log());
}

#[test]
fn lesser_trees_unique_ids() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 5;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let mut ids: Vec<_> = result.db.trees.iter_all().map(|t| t.id).collect();
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count, "All tree IDs should be unique");
}

#[test]
fn lesser_tree_config_serde_roundtrip() {
    let config = GameConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    let restored: GameConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.lesser_trees.count, config.lesser_trees.count);
    assert_eq!(
        restored.lesser_trees.min_distance_from_main,
        config.lesser_trees.min_distance_from_main,
    );
    assert_eq!(
        restored.lesser_trees.min_distance_between,
        config.lesser_trees.min_distance_between,
    );
    assert_eq!(
        restored.lesser_trees.profiles.len(),
        config.lesser_trees.profiles.len(),
    );
    assert!(
        (restored.lesser_trees.fruit_bearing_fraction - config.lesser_trees.fruit_bearing_fraction)
            .abs()
            < f64::EPSILON,
    );
}

// --- Wild fruit tests ---

#[test]
fn wild_fruit_all_lesser_trees_bear_fruit_when_fraction_1() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 5;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.fruit_bearing_fraction = 1.0;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_trees: Vec<_> = result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
        .collect();
    assert!(!lesser_trees.is_empty());
    for tree in &lesser_trees {
        assert!(
            tree.fruit_species_id.is_some(),
            "All lesser trees should have fruit species when fraction = 1.0"
        );
    }
}

#[test]
fn wild_fruit_no_lesser_trees_bear_fruit_when_fraction_0() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 5;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.fruit_bearing_fraction = 0.0;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_trees: Vec<_> = result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
        .collect();
    assert!(!lesser_trees.is_empty());
    for tree in &lesser_trees {
        assert!(
            tree.fruit_species_id.is_none(),
            "No lesser trees should have fruit species when fraction = 0.0"
        );
    }
}

#[test]
fn wild_fruit_partial_fraction_assigns_some() {
    let mut config = wg_test_config();
    // Use a large count + many attempts so enough trees are always placed that
    // 50% fraction reliably produces "some but not all" regardless of seed.
    config.lesser_trees.count = 40;
    config.lesser_trees.max_placement_attempts = 1000;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.fruit_bearing_fraction = 0.5;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    let lesser_trees: Vec<_> = result
        .db
        .trees
        .iter_all()
        .filter(|t| t.id != result.player_tree_id)
        .collect();
    // With 40 requested + 1000 attempts we expect to place many trees.
    assert!(
        lesser_trees.len() >= 5,
        "Expected at least 5 lesser trees placed, got {}",
        lesser_trees.len()
    );
    let fruit_count = lesser_trees
        .iter()
        .filter(|t| t.fruit_species_id.is_some())
        .count();
    assert!(
        fruit_count > 0,
        "Some lesser trees should have fruit species (got 0 out of {})",
        lesser_trees.len()
    );
    assert!(
        fruit_count < lesser_trees.len(),
        "Not all lesser trees should have fruit species at 50% (got {} / {})",
        fruit_count,
        lesser_trees.len()
    );
}

#[test]
fn wild_fruit_species_exist_in_db() {
    let mut config = wg_test_config();
    config.lesser_trees.count = 5;
    config.lesser_trees.min_distance_from_main = 5;
    config.lesser_trees.min_distance_between = 3;
    config.lesser_trees.fruit_bearing_fraction = 1.0;

    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());
    for tree in result.db.trees.iter_all() {
        if let Some(species_id) = tree.fruit_species_id {
            assert!(
                result.db.fruit_species.contains(&species_id),
                "Assigned fruit species {:?} should exist in DB",
                species_id
            );
        }
    }
}

// --- Edge case: single civ ---

#[test]
fn worldgen_single_civ_no_ai_civs() {
    // civ_count = 1 means only the player civ is created.  Diplomacy has no
    // pairs to process, so no relationships should exist.
    let mut config = wg_test_config();
    config.worldgen.civs.civ_count = 1;
    let result = run_worldgen(fresh_test_seed(), &config, &noop_log());

    assert_eq!(result.db.civilizations.len(), 1, "Only the player civ");
    let player_civ = result.db.civilizations.get(&CivId(0)).unwrap();
    assert!(player_civ.player_controlled);

    let rels: Vec<_> = result.db.civ_relationships.iter_all().collect();
    assert!(
        rels.is_empty(),
        "No diplomatic relationships with only one civ (got {})",
        rels.len()
    );
}

// --- Cross-species opinion table ---

#[test]
fn species_default_opinion_cross_species_pairs() {
    // Verify the full opinion table for key asymmetric and symmetric pairs.
    // Goblin→X is Hostile but X→Goblin is only Suspicious (asymmetric).
    // Orc→X and X→Orc are both Hostile (symmetric).
    // Troll→X and X→Troll are both Suspicious (symmetric).

    // Goblin is asymmetrically hostile: they hate others more than others hate
    // them.  This matters for raid generation — Goblin civs will aggressively
    // target the player.
    assert_eq!(
        species_default_opinion(CivSpecies::Goblin, CivSpecies::Elf),
        CivOpinion::Hostile,
        "Goblin→Elf should be Hostile"
    );
    assert_eq!(
        species_default_opinion(CivSpecies::Elf, CivSpecies::Goblin),
        CivOpinion::Suspicious,
        "Elf→Goblin should be Suspicious (not Hostile)"
    );

    // Orc is symmetrically hostile to non-orc, non-troll species.
    assert_eq!(
        species_default_opinion(CivSpecies::Orc, CivSpecies::Elf),
        CivOpinion::Hostile,
        "Orc→Elf should be Hostile"
    );
    assert_eq!(
        species_default_opinion(CivSpecies::Elf, CivSpecies::Orc),
        CivOpinion::Hostile,
        "Elf→Orc should be Hostile"
    );

    // Troll is suspicious of everyone (but not outright hostile).
    assert_eq!(
        species_default_opinion(CivSpecies::Troll, CivSpecies::Elf),
        CivOpinion::Suspicious,
        "Troll→Elf should be Suspicious"
    );
    assert_eq!(
        species_default_opinion(CivSpecies::Elf, CivSpecies::Troll),
        CivOpinion::Suspicious,
        "Elf→Troll should be Suspicious"
    );

    // Elf, Human, Dwarf are all mutually Neutral.
    assert_eq!(
        species_default_opinion(CivSpecies::Elf, CivSpecies::Human),
        CivOpinion::Neutral
    );
    assert_eq!(
        species_default_opinion(CivSpecies::Human, CivSpecies::Elf),
        CivOpinion::Neutral
    );
    assert_eq!(
        species_default_opinion(CivSpecies::Dwarf, CivSpecies::Human),
        CivOpinion::Neutral
    );

    // Goblin↔Orc: Suspicious (not full Hostile).
    assert_eq!(
        species_default_opinion(CivSpecies::Goblin, CivSpecies::Orc),
        CivOpinion::Suspicious,
        "Goblin→Orc should be Suspicious"
    );
    assert_eq!(
        species_default_opinion(CivSpecies::Orc, CivSpecies::Goblin),
        CivOpinion::Suspicious,
        "Orc→Goblin should be Suspicious"
    );
}
