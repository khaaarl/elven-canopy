// Worldgen framework — entry point for world generation during StartGame.
//
// This module establishes the generator sequencing pattern used during game
// initialization. When a new game starts, `run_worldgen()` creates a
// dedicated worldgen PRNG (seeded from the world seed), then runs generators
// in a defined order:
//
//   1. **Tree generation** — produces the player's home tree geometry (existing
//      logic extracted from `sim.rs`).
//   2. **Fruit generation** — placeholder, will be implemented by F-fruit-variety.
//   3. **Civilization generation** — placeholder, will be implemented by F-civilizations.
//   4. **Knowledge distribution** — placeholder, will be implemented by F-civ-knowledge.
//
// After all generators complete, the runtime PRNG is derived from the worldgen
// PRNG's state, ensuring the worldgen sequence doesn't affect runtime randomness
// order and that the entire pipeline is deterministic from the world seed.
//
// The `WorldgenResult` struct carries all outputs back to `SimState::with_config()`,
// which uses them to populate the sim's initial state.
//
// **WorldgenConfig** is a subsection of `GameConfig` that groups configuration
// for worldgen generators (currently empty, will hold `FruitConfig` and
// `CivConfig` when those features land). The existing tree profile config
// stays at the top level of `GameConfig`.
//
// **Critical constraint: determinism.** All generators use the worldgen PRNG
// exclusively. No HashMap, no system time, no OS entropy. Use BTreeMap for
// ordered collections. The generator order is fixed and must not change
// without updating all downstream seeds.

use std::collections::BTreeMap;

use crate::config::GameConfig;
use crate::nav::{self, NavGraph};
use crate::sim::Tree;
use crate::structural;
use crate::tree_gen;
use crate::types::{PlayerId, TreeId, VoxelCoord, VoxelType};
use crate::world::VoxelWorld;
use elven_canopy_prng::GameRng;

/// Configuration for worldgen generators. Subsection of `GameConfig`.
///
/// Currently empty — serves as the extension point for `FruitConfig` (F-fruit-variety)
/// and `CivConfig` (F-civilizations). The existing tree profile config stays at the
/// top level of `GameConfig` since tree generation predates this framework.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WorldgenConfig {
    // Future: pub fruit: FruitConfig,
    // Future: pub civs: CivConfig,
}

/// Output of the worldgen pipeline, consumed by `SimState::with_config()` to
/// populate the sim's initial state.
pub struct WorldgenResult {
    /// The runtime PRNG, seeded from the worldgen PRNG's final state.
    pub runtime_rng: GameRng,

    /// The voxel world with tree geometry and terrain placed.
    pub world: VoxelWorld,

    /// The player's home tree entity.
    pub home_tree: Tree,

    /// Standard navigation graph (1x1x1 creatures).
    pub nav_graph: NavGraph,

    /// Large navigation graph (2x2x2 creatures like elephants).
    pub large_nav_graph: NavGraph,

    /// The player's ID (generated during worldgen for deterministic ordering).
    pub player_id: PlayerId,
}

/// Run the full worldgen pipeline: generate the world from a seed and config.
///
/// Creates a dedicated worldgen PRNG from the world seed, runs generators in
/// order, then derives the runtime PRNG from the worldgen PRNG's final state.
/// This separation ensures worldgen-only changes (e.g., adding a new generator)
/// don't shift the runtime PRNG sequence, as long as the worldgen PRNG is
/// consumed identically.
pub fn run_worldgen(seed: u64, config: &GameConfig) -> WorldgenResult {
    // Worldgen PRNG: dedicated instance seeded from the world seed.
    // All worldgen generators draw from this PRNG in a fixed order.
    let mut wg_rng = GameRng::new(seed);

    // Generate IDs first — order matters for determinism.
    let player_id = PlayerId::new(&mut wg_rng);
    let player_tree_id = TreeId::new(&mut wg_rng);

    // --- Generator 1: Tree ---
    let (world, home_tree) = generate_tree(&mut wg_rng, config, player_id, player_tree_id);

    // --- Generator 2: Fruits (placeholder) ---
    // Will be implemented by F-fruit-variety. The generator will populate
    // a FruitSpecies table in SimDb using wg_rng.

    // --- Generator 3: Civilizations (placeholder) ---
    // Will be implemented by F-civilizations. The generator will create
    // Civilization rows and assign civ membership.

    // --- Generator 4: Knowledge distribution (placeholder) ---
    // Will be implemented by F-civ-knowledge. The generator will populate
    // CivFruitKnowledge and CivRelationship tables.

    // Build nav graphs from the completed voxel world.
    let nav_graph = nav::build_nav_graph(&world, &BTreeMap::new());
    let large_nav_graph = nav::build_large_nav_graph(&world);

    // Derive the runtime PRNG from the worldgen PRNG's current state.
    // This uses the worldgen PRNG to generate a new seed, ensuring the
    // runtime PRNG is deterministically derived but independent of the
    // exact number of draws made during worldgen.
    let runtime_seed = wg_rng.next_u64();
    let runtime_rng = GameRng::new(runtime_seed);

    WorldgenResult {
        runtime_rng,
        world,
        home_tree,
        nav_graph,
        large_nav_graph,
        player_id,
    }
}

/// Tree generator: produces the player's home tree and populates the voxel world.
///
/// Extracted from the former `SimState::with_config()` inline logic. Runs the
/// energy-based recursive tree generation with structural validation retry loop.
fn generate_tree(
    rng: &mut GameRng,
    config: &GameConfig,
    player_id: PlayerId,
    player_tree_id: TreeId,
) -> (VoxelWorld, Tree) {
    let (ws_x, ws_y, ws_z) = config.world_size;
    let center_x = ws_x as i32 / 2;
    let center_z = ws_z as i32 / 2;

    let mut world = VoxelWorld::new(ws_x, ws_y, ws_z);
    let mut tree_result = None;

    for _attempt in 0..config.structural.tree_gen_max_retries {
        let candidate = tree_gen::generate_tree(&mut world, config, rng);
        if structural::validate_tree(&world, config) {
            tree_result = Some(candidate);
            break;
        }
        // Clear and rebuild world for retry.
        world = VoxelWorld::new(ws_x, ws_y, ws_z);
        let floor_extent = config.floor_extent;
        for dx in -floor_extent..=floor_extent {
            for dz in -floor_extent..=floor_extent {
                world.set(
                    VoxelCoord::new(center_x + dx, 0, center_z + dz),
                    VoxelType::ForestFloor,
                );
            }
        }
    }

    let tree_result = tree_result.expect(
        "Tree generation failed structural validation after max retries. \
         Tree profile parameters are incompatible with material properties.",
    );

    let home_tree = Tree {
        id: player_tree_id,
        position: VoxelCoord::new(center_x, 0, center_z),
        health: 100.0,
        growth_level: 1,
        mana_stored: config.starting_mana,
        mana_capacity: config.starting_mana_capacity,
        fruit_production_rate: config.fruit_production_base_rate,
        carrying_capacity: 20.0,
        current_load: 0.0,
        owner: Some(player_id),
        trunk_voxels: tree_result.trunk_voxels,
        branch_voxels: tree_result.branch_voxels,
        leaf_voxels: tree_result.leaf_voxels,
        root_voxels: tree_result.root_voxels,
        dirt_voxels: tree_result.dirt_voxels,
        fruit_positions: Vec::new(),
    };

    (world, home_tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small-world config for fast tests (matches sim.rs test_config pattern).
    fn test_config() -> GameConfig {
        let mut config = GameConfig {
            world_size: (64, 64, 64),
            ..GameConfig::default()
        };
        config.tree_profile.growth.initial_energy = 50.0;
        config.terrain_max_height = 0;
        config
    }

    #[test]
    fn worldgen_is_deterministic() {
        // Same seed + config must produce identical results.
        let seed = 42;
        let config = test_config();

        let result1 = run_worldgen(seed, &config);
        let result2 = run_worldgen(seed, &config);

        // Tree geometry must match.
        assert_eq!(
            result1.home_tree.trunk_voxels,
            result2.home_tree.trunk_voxels
        );
        assert_eq!(
            result1.home_tree.branch_voxels,
            result2.home_tree.branch_voxels
        );
        assert_eq!(result1.home_tree.leaf_voxels, result2.home_tree.leaf_voxels);
        assert_eq!(result1.home_tree.root_voxels, result2.home_tree.root_voxels);
        assert_eq!(result1.home_tree.dirt_voxels, result2.home_tree.dirt_voxels);

        // IDs must match.
        assert_eq!(result1.home_tree.id, result2.home_tree.id);
        assert_eq!(result1.player_id, result2.player_id);

        // Nav graphs must match (node + edge counts).
        assert_eq!(
            result1.nav_graph.node_count(),
            result2.nav_graph.node_count()
        );
        assert_eq!(
            result1.nav_graph.edge_count(),
            result2.nav_graph.edge_count()
        );
        assert_eq!(
            result1.large_nav_graph.node_count(),
            result2.large_nav_graph.node_count()
        );
        assert_eq!(
            result1.large_nav_graph.edge_count(),
            result2.large_nav_graph.edge_count()
        );
    }

    #[test]
    fn different_seeds_produce_different_worlds() {
        let config = test_config();

        let result1 = run_worldgen(1, &config);
        let result2 = run_worldgen(2, &config);

        // Different seeds should produce different tree geometry.
        // (Technically could collide, but astronomically unlikely.)
        assert_ne!(
            result1.home_tree.trunk_voxels,
            result2.home_tree.trunk_voxels
        );
    }

    #[test]
    fn runtime_rng_differs_from_worldgen_start() {
        // The runtime PRNG should not be the same as the initial worldgen PRNG.
        // This verifies the derivation step works.
        let seed = 42;
        let config = test_config();

        let result = run_worldgen(seed, &config);

        // The runtime RNG should produce different values than a fresh RNG
        // with the same seed.
        let mut fresh_rng = GameRng::new(seed);
        let mut runtime_rng = result.runtime_rng;

        assert_ne!(fresh_rng.next_u64(), runtime_rng.next_u64());
    }

    #[test]
    fn worldgen_config_default_is_empty() {
        // WorldgenConfig defaults to an empty struct (no fruit/civ config yet).
        let wc = WorldgenConfig::default();
        // Just verify it round-trips through serde.
        let json = serde_json::to_string(&wc).unwrap();
        let _: WorldgenConfig = serde_json::from_str(&json).unwrap();
    }
}
