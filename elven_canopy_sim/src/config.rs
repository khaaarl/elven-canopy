// Data-driven game configuration.
//
// All tunable simulation parameters live here in `GameConfig`, loaded from
// JSON at startup. The sim never uses magic numbers — it reads from the
// config. This enables balance iteration without recompilation, and in
// multiplayer all clients must have identical configs (enforced via hash
// comparison at session handshake).
//
// Species-specific behavioral data (speed, heartbeat interval, edge
// restrictions) lives in `SpeciesData` entries keyed by `Species` in the
// `species` map — see `species.rs`.
//
// See also: `sim.rs` which owns the `GameConfig` as part of `SimState`,
// `species.rs` for the `SpeciesData` struct.
//
// **Critical constraint: determinism.** Config values feed directly into
// simulation logic. All clients must use identical configs for identical
// results.

use crate::nav::EdgeType;
use crate::species::SpeciesData;
use crate::types::Species;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level game configuration. Loaded from JSON, never mutated at runtime.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameConfig {
    /// Number of real-world milliseconds per simulation tick.
    pub tick_duration_ms: u32,

    /// Interval (in ticks) between tree heartbeat events (fruit production,
    /// mana capacity updates).
    pub tree_heartbeat_interval_ticks: u64,

    /// Reference movement speed used for nav graph edge cost computation.
    /// This is the flat-surface speed used when building the graph; individual
    /// creature species may move faster or slower.
    pub nav_base_speed: f32,

    /// Multiplier applied to movement speed when climbing raw trunk.
    pub climb_speed_multiplier: f32,

    /// Multiplier applied to movement speed when using stairs/ramps.
    pub stair_speed_multiplier: f32,

    /// Base mana generated per elf per heartbeat tick.
    pub mana_base_generation_rate: f32,

    /// Range of mood-based multipliers on mana generation.
    /// `(min_multiplier, max_multiplier)` — interpolated from worst to best mood.
    pub mana_mood_multiplier_range: (f32, f32),

    /// Mana cost to grow one voxel of platform.
    pub platform_mana_cost_per_voxel: f32,

    /// Mana cost to grow one voxel of bridge/walkway.
    pub bridge_mana_cost_per_voxel: f32,

    /// Base rate of fruit production per tree per heartbeat tick.
    pub fruit_production_base_rate: f32,

    /// World dimensions in voxels (x, y, z).
    pub world_size: (u32, u32, u32),

    /// Initial mana stored in the player's home tree.
    pub starting_mana: f32,

    /// Maximum mana the starting tree can hold.
    pub starting_mana_capacity: f32,

    // -- Tree generation --

    /// Radius of the tree trunk in voxels.
    pub tree_trunk_radius: u32,

    /// Height of the tree trunk in voxels.
    pub tree_trunk_height: u32,

    /// Y-level where branches start growing.
    pub tree_branch_start_y: u32,

    /// Vertical spacing between branch levels.
    pub tree_branch_interval: u32,

    /// Number of branches to generate.
    pub tree_branch_count: u32,

    /// Length of each branch in voxels (from trunk surface).
    pub tree_branch_length: u32,

    /// Cross-section radius of branches in voxels.
    pub tree_branch_radius: u32,

    // -- Branch forking --

    /// Per-eligible-step probability of spawning a sub-branch fork.
    pub tree_branch_fork_chance: f32,

    /// Minimum steps along a branch before forking is eligible.
    pub tree_branch_fork_min_step: u32,

    /// Angle offset from parent direction for fork (radians, ~34° at 0.6).
    pub tree_branch_fork_angle: f32,

    /// Child branch length = parent remaining steps * this ratio.
    pub tree_branch_fork_length_ratio: f32,

    /// Child branch radius = parent effective radius * this ratio.
    pub tree_branch_fork_radius_ratio: f32,

    /// Maximum fork generations (0 = no forks, 1 = one level, etc.).
    pub tree_branch_fork_max_depth: u32,

    // -- Navigation --

    /// Vertical spacing between trunk surface nav nodes.
    pub nav_node_vertical_spacing: u32,

    /// Number of concentric ground nav rings around the trunk base.
    pub ground_ring_count: u32,

    /// Voxel distance between successive ground rings.
    pub ground_ring_spacing: u32,

    // -- Species --

    /// Per-species behavioral data (speed, heartbeat interval, edge
    /// restrictions, spawn rules). Keyed by `Species` enum.
    pub species: BTreeMap<Species, SpeciesData>,
}

impl Default for GameConfig {
    fn default() -> Self {
        let mut species = BTreeMap::new();
        species.insert(
            Species::Elf,
            SpeciesData {
                base_speed: 0.1,
                heartbeat_interval_ticks: 30,
                allowed_edge_types: None, // elves can traverse all edges
                ground_only: false,
            },
        );
        species.insert(
            Species::Capybara,
            SpeciesData {
                base_speed: 0.06,
                heartbeat_interval_ticks: 40,
                allowed_edge_types: Some(vec![EdgeType::ForestFloor]),
                ground_only: true,
            },
        );

        Self {
            tick_duration_ms: 100,
            tree_heartbeat_interval_ticks: 100,
            nav_base_speed: 0.1,
            climb_speed_multiplier: 0.4,
            stair_speed_multiplier: 0.7,
            mana_base_generation_rate: 1.0,
            mana_mood_multiplier_range: (0.2, 2.0),
            platform_mana_cost_per_voxel: 10.0,
            bridge_mana_cost_per_voxel: 15.0,
            fruit_production_base_rate: 0.5,
            world_size: (256, 128, 256),
            starting_mana: 100.0,
            starting_mana_capacity: 500.0,
            tree_trunk_radius: 3,
            tree_trunk_height: 40,
            tree_branch_start_y: 12,
            tree_branch_interval: 6,
            tree_branch_count: 5,
            tree_branch_length: 8,
            tree_branch_radius: 1,
            tree_branch_fork_chance: 0.08,
            tree_branch_fork_min_step: 3,
            tree_branch_fork_angle: 0.6,
            tree_branch_fork_length_ratio: 0.6,
            tree_branch_fork_radius_ratio: 0.65,
            tree_branch_fork_max_depth: 2,
            nav_node_vertical_spacing: 4,
            ground_ring_count: 4,
            ground_ring_spacing: 8,
            species,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_serializes() {
        let config = GameConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let restored: GameConfig = serde_json::from_str(&json).unwrap();
        // Verify a few fields survived the roundtrip.
        assert_eq!(config.tick_duration_ms, restored.tick_duration_ms);
        assert_eq!(
            config.tree_heartbeat_interval_ticks,
            restored.tree_heartbeat_interval_ticks
        );
        assert_eq!(config.world_size, restored.world_size);
        // Verify species data survived.
        assert_eq!(config.species.len(), restored.species.len());
        let elf_data = &restored.species[&Species::Elf];
        assert_eq!(elf_data.heartbeat_interval_ticks, 30);
    }

    #[test]
    fn config_loads_from_json_string() {
        let json = r#"{
            "tick_duration_ms": 50,
            "tree_heartbeat_interval_ticks": 200,
            "nav_base_speed": 0.2,
            "climb_speed_multiplier": 0.3,
            "stair_speed_multiplier": 0.6,
            "mana_base_generation_rate": 2.0,
            "mana_mood_multiplier_range": [0.1, 3.0],
            "platform_mana_cost_per_voxel": 8.0,
            "bridge_mana_cost_per_voxel": 12.0,
            "fruit_production_base_rate": 0.8,
            "world_size": [128, 64, 128],
            "starting_mana": 200.0,
            "starting_mana_capacity": 1000.0,
            "tree_trunk_radius": 4,
            "tree_trunk_height": 50,
            "tree_branch_start_y": 15,
            "tree_branch_interval": 8,
            "tree_branch_count": 6,
            "tree_branch_length": 10,
            "tree_branch_radius": 2,
            "tree_branch_fork_chance": 0.08,
            "tree_branch_fork_min_step": 3,
            "tree_branch_fork_angle": 0.6,
            "tree_branch_fork_length_ratio": 0.6,
            "tree_branch_fork_radius_ratio": 0.65,
            "tree_branch_fork_max_depth": 2,
            "nav_node_vertical_spacing": 5,
            "ground_ring_count": 3,
            "ground_ring_spacing": 10,
            "species": {
                "Elf": {
                    "base_speed": 0.2,
                    "heartbeat_interval_ticks": 100,
                    "allowed_edge_types": null,
                    "ground_only": false
                },
                "Capybara": {
                    "base_speed": 0.05,
                    "heartbeat_interval_ticks": 200,
                    "allowed_edge_types": ["ForestFloor"],
                    "ground_only": true
                }
            }
        }"#;
        let config: GameConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.tick_duration_ms, 50);
        assert_eq!(config.world_size, (128, 64, 128));
        assert_eq!(config.tree_trunk_radius, 4);
        assert_eq!(config.nav_node_vertical_spacing, 5);
        let capy = &config.species[&Species::Capybara];
        assert_eq!(capy.heartbeat_interval_ticks, 200);
        assert!(capy.ground_only);
    }
}
