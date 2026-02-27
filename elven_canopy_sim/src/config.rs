// Data-driven game configuration.
//
// All tunable simulation parameters live here in `GameConfig`, loaded from
// JSON at startup. The sim never uses magic numbers — it reads from the
// config. This enables balance iteration without recompilation, and in
// multiplayer all clients must have identical configs (enforced via hash
// comparison at session handshake).
//
// Tree generation parameters are grouped into a `TreeProfile` struct with
// nested sub-structs: `GrowthParams`, `SplitParams`, `CurvatureParams`,
// `RootParams`, `LeafParams`, and `TrunkParams`. Named preset constructors
// (`TreeProfile::fantasy_mega()`, `::oak()`, etc.) produce different tree
// archetypes by tuning the same parameter set.
//
// Species-specific behavioral data (speed, heartbeat interval, edge
// restrictions) lives in `SpeciesData` entries keyed by `Species` in the
// `species` map — see `species.rs`.
//
// See also: `sim.rs` which owns the `GameConfig` as part of `SimState`,
// `species.rs` for the `SpeciesData` struct, `tree_gen.rs` for the
// energy-based recursive segment growth algorithm that reads `TreeProfile`.
//
// **Critical constraint: determinism.** Config values feed directly into
// simulation logic. All clients must use identical configs for identical
// results.

use crate::nav::EdgeType;
use crate::species::SpeciesData;
use crate::types::Species;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Tree profile — nested parameter groups
// ---------------------------------------------------------------------------

/// Shape of leaf blobs at branch terminals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeafShape {
    /// Uniform sphere.
    Sphere,
    /// Vertically compressed ellipsoid — wider than tall.
    Cloud,
}

/// Controls energy budget, radius scaling, and step size for segment growth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrowthParams {
    /// Total energy budget for the tree (trunk + branches). Higher values
    /// produce taller, thicker trees.
    pub initial_energy: f32,
    /// Scaling factor: `radius = sqrt(energy * energy_to_radius)`. Controls
    /// how thick segments are relative to their remaining energy.
    pub energy_to_radius: f32,
    /// Minimum segment radius in voxels. Segments below this still place voxels.
    pub min_radius: f32,
    /// Distance in voxels per growth step.
    pub growth_step_length: f32,
    /// Energy consumed per growth step.
    pub energy_per_step: f32,
}

/// Controls when and how segments split into child branches.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SplitParams {
    /// Base probability of splitting at each eligible step.
    pub split_chance_base: f32,
    /// Number of child branches per split event (typically 1–2).
    pub split_count: u32,
    /// Fraction of parent's remaining energy given to each child.
    /// The continuation keeps `1 - split_energy_ratio * split_count`.
    pub split_energy_ratio: f32,
    /// Angle (radians) between parent direction and child direction.
    pub split_angle: f32,
    /// Random variance added to split angle (radians).
    pub split_angle_variance: f32,
    /// Minimum fraction of energy spent before splits become eligible (0.0–1.0).
    pub min_progress_for_split: f32,
}

/// Controls how segments curve during growth.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurvatureParams {
    /// Gravitropism strength. Positive = grow upward, negative = droop.
    /// Applied as a vertical bias to the direction each step.
    pub gravitropism: f32,
    /// Maximum angular deflection (radians) per step from random perturbation.
    pub random_deflection: f32,
    /// How much successive deflections correlate (0.0 = fully random each step,
    /// 1.0 = same deflection direction throughout). Creates smooth curves.
    pub deflection_coherence: f32,
}

/// Controls root segment generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootParams {
    /// Fraction of `initial_energy` allocated to roots (0.0–1.0).
    pub root_energy_fraction: f32,
    /// Number of root segments seeded at the trunk base.
    pub root_initial_count: u32,
    /// Gravitropism for roots — positive = grow downward.
    pub root_gravitropism: f32,
    /// Initial angle below horizontal for root segments (radians).
    /// 0 = horizontal, PI/2 = straight down.
    pub root_initial_angle: f32,
    /// Tendency to stay near the surface (y=0). Higher values keep roots
    /// shallow; 0.0 allows roots to dive deep.
    pub root_surface_tendency: f32,
}

/// Controls leaf blob generation at branch terminals.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeafParams {
    /// Shape of leaf blobs.
    pub leaf_shape: LeafShape,
    /// Probability each voxel within the blob radius is placed (0.0–1.0).
    pub leaf_density: f32,
    /// Radius of leaf blobs in voxels.
    pub leaf_size: u32,
    /// Overall canopy density factor. Multiplied with leaf_density for the
    /// effective placement probability. 0.0 = no leaves.
    pub canopy_density: f32,
}

/// Controls trunk-specific properties.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrunkParams {
    /// Flare factor at the trunk base. 0.0 = no flare, 1.0 = double radius at y=1.
    pub base_flare: f32,
    /// Initial growth direction as a unit vector [x, y, z].
    /// Default is straight up: [0.0, 1.0, 0.0].
    pub initial_direction: [f32; 3],
}

/// Complete tree generation profile — all parameters needed to grow a tree.
///
/// Named preset constructors produce different tree archetypes:
/// - `fantasy_mega()`: towering mega-tree (the default)
/// - `oak()`: broad spreading crown
/// - `conifer()`: tall and narrow
/// - `willow()`: drooping branches
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeProfile {
    pub growth: GrowthParams,
    pub split: SplitParams,
    pub curvature: CurvatureParams,
    pub roots: RootParams,
    pub leaves: LeafParams,
    pub trunk: TrunkParams,
}

impl TreeProfile {
    /// Fantasy mega-tree: the default preset. A towering tree with thick trunk,
    /// wide spread, many splits, and a generous root system.
    pub fn fantasy_mega() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 400.0,
                energy_to_radius: 0.08,
                min_radius: 0.5,
                growth_step_length: 1.0,
                energy_per_step: 1.0,
            },
            split: SplitParams {
                split_chance_base: 0.12,
                split_count: 1,
                split_energy_ratio: 0.35,
                split_angle: 0.7,
                split_angle_variance: 0.3,
                min_progress_for_split: 0.15,
            },
            curvature: CurvatureParams {
                gravitropism: 0.08,
                random_deflection: 0.15,
                deflection_coherence: 0.7,
            },
            roots: RootParams {
                root_energy_fraction: 0.15,
                root_initial_count: 5,
                root_gravitropism: 0.12,
                root_initial_angle: 0.3,
                root_surface_tendency: 0.8,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Sphere,
                leaf_density: 0.65,
                leaf_size: 3,
                canopy_density: 1.0,
            },
            trunk: TrunkParams {
                base_flare: 0.5,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Oak: broad spreading crown, thick trunk, moderate height.
    pub fn oak() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 250.0,
                energy_to_radius: 0.1,
                min_radius: 0.5,
                growth_step_length: 1.0,
                energy_per_step: 1.2,
            },
            split: SplitParams {
                split_chance_base: 0.18,
                split_count: 1,
                split_energy_ratio: 0.4,
                split_angle: 0.9,
                split_angle_variance: 0.4,
                min_progress_for_split: 0.1,
            },
            curvature: CurvatureParams {
                gravitropism: 0.04,
                random_deflection: 0.2,
                deflection_coherence: 0.6,
            },
            roots: RootParams {
                root_energy_fraction: 0.12,
                root_initial_count: 4,
                root_gravitropism: 0.15,
                root_initial_angle: 0.2,
                root_surface_tendency: 0.9,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Cloud,
                leaf_density: 0.7,
                leaf_size: 3,
                canopy_density: 1.0,
            },
            trunk: TrunkParams {
                base_flare: 0.3,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Conifer: tall and narrow, strong central leader.
    pub fn conifer() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 300.0,
                energy_to_radius: 0.06,
                min_radius: 0.5,
                growth_step_length: 1.0,
                energy_per_step: 0.8,
            },
            split: SplitParams {
                split_chance_base: 0.08,
                split_count: 2,
                split_energy_ratio: 0.2,
                split_angle: 0.6,
                split_angle_variance: 0.2,
                min_progress_for_split: 0.05,
            },
            curvature: CurvatureParams {
                gravitropism: 0.15,
                random_deflection: 0.05,
                deflection_coherence: 0.8,
            },
            roots: RootParams {
                root_energy_fraction: 0.1,
                root_initial_count: 3,
                root_gravitropism: 0.2,
                root_initial_angle: 0.5,
                root_surface_tendency: 0.5,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Sphere,
                leaf_density: 0.5,
                leaf_size: 2,
                canopy_density: 0.8,
            },
            trunk: TrunkParams {
                base_flare: 0.2,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Willow: drooping branches with negative gravitropism on higher generations.
    pub fn willow() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 200.0,
                energy_to_radius: 0.07,
                min_radius: 0.5,
                growth_step_length: 1.0,
                energy_per_step: 1.0,
            },
            split: SplitParams {
                split_chance_base: 0.15,
                split_count: 2,
                split_energy_ratio: 0.3,
                split_angle: 0.5,
                split_angle_variance: 0.3,
                min_progress_for_split: 0.1,
            },
            curvature: CurvatureParams {
                gravitropism: -0.1,
                random_deflection: 0.1,
                deflection_coherence: 0.9,
            },
            roots: RootParams {
                root_energy_fraction: 0.1,
                root_initial_count: 4,
                root_gravitropism: 0.1,
                root_initial_angle: 0.3,
                root_surface_tendency: 0.7,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Sphere,
                leaf_density: 0.4,
                leaf_size: 2,
                canopy_density: 1.2,
            },
            trunk: TrunkParams {
                base_flare: 0.15,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level game config
// ---------------------------------------------------------------------------

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

    /// Maximum number of fruit a single tree can bear at once.
    pub fruit_max_per_tree: u32,

    /// Number of fruit-spawn attempts during tree initialization (fast-forward).
    /// Each attempt runs the same code path as heartbeat-driven spawning, so
    /// not all attempts will succeed (chance roll + valid position required).
    pub fruit_initial_attempts: u32,

    /// World dimensions in voxels (x, y, z).
    pub world_size: (u32, u32, u32),

    /// Half-extent of the forest floor around the world center.
    /// The floor covers `(center - floor_extent)` to `(center + floor_extent)`
    /// in both X and Z at y=0. Also used by `rebuild_world()` when restoring
    /// transient state after deserialization.
    pub floor_extent: i32,

    /// Initial mana stored in the player's home tree.
    pub starting_mana: f32,

    /// Maximum mana the starting tree can hold.
    pub starting_mana_capacity: f32,

    /// Tree generation parameters — energy-based recursive growth profile.
    pub tree_profile: TreeProfile,

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
            fruit_max_per_tree: 20,
            fruit_initial_attempts: 12,
            world_size: (256, 128, 256),
            floor_extent: 20,
            starting_mana: 100.0,
            starting_mana_capacity: 500.0,
            tree_profile: TreeProfile::fantasy_mega(),
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
        // Verify tree profile survived.
        assert_eq!(
            config.tree_profile.growth.initial_energy,
            restored.tree_profile.growth.initial_energy
        );
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
            "fruit_max_per_tree": 25,
            "fruit_initial_attempts": 15,
            "world_size": [128, 64, 128],
            "floor_extent": 15,
            "starting_mana": 200.0,
            "starting_mana_capacity": 1000.0,
            "tree_profile": {
                "growth": {
                    "initial_energy": 300.0,
                    "energy_to_radius": 0.1,
                    "min_radius": 0.5,
                    "growth_step_length": 1.0,
                    "energy_per_step": 1.0
                },
                "split": {
                    "split_chance_base": 0.1,
                    "split_count": 1,
                    "split_energy_ratio": 0.3,
                    "split_angle": 0.8,
                    "split_angle_variance": 0.3,
                    "min_progress_for_split": 0.15
                },
                "curvature": {
                    "gravitropism": 0.05,
                    "random_deflection": 0.1,
                    "deflection_coherence": 0.7
                },
                "roots": {
                    "root_energy_fraction": 0.1,
                    "root_initial_count": 4,
                    "root_gravitropism": 0.1,
                    "root_initial_angle": 0.3,
                    "root_surface_tendency": 0.8
                },
                "leaves": {
                    "leaf_shape": "Sphere",
                    "leaf_density": 0.5,
                    "leaf_size": 3,
                    "canopy_density": 1.0
                },
                "trunk": {
                    "base_flare": 0.3,
                    "initial_direction": [0.0, 1.0, 0.0]
                }
            },
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
        assert_eq!(config.tree_profile.growth.initial_energy, 300.0);
        let capy = &config.species[&Species::Capybara];
        assert_eq!(capy.heartbeat_interval_ticks, 200);
        assert!(capy.ground_only);
    }

    #[test]
    fn preset_fantasy_mega_has_roots() {
        let profile = TreeProfile::fantasy_mega();
        assert!(profile.roots.root_energy_fraction > 0.0);
        assert!(profile.roots.root_initial_count > 0);
    }

    #[test]
    fn preset_oak_has_wider_splits() {
        let oak = TreeProfile::oak();
        let conifer = TreeProfile::conifer();
        assert!(oak.split.split_angle > conifer.split.split_angle);
    }
}
