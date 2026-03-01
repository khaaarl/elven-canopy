// Data-driven game configuration.
//
// All tunable simulation parameters live here in `GameConfig`, loaded from
// JSON at startup. The sim never uses magic numbers — it reads from the
// config. This enables balance iteration without recompilation, and in
// multiplayer all clients must have identical configs (enforced via hash
// comparison at session handshake).
//
// **The sim runs at 1000 ticks per simulated second.** `tick_duration_ms = 1`
// means each tick represents 1 millisecond of game time. All tick-denominated
// config values (heartbeat intervals, food decay rates, species speed params)
// are calibrated for this rate.
//
// Tree generation parameters are grouped into a `TreeProfile` struct with
// nested sub-structs: `GrowthParams`, `SplitParams`, `CurvatureParams`,
// `RootParams`, `LeafParams`, and `TrunkParams`. Named preset constructors
// (`TreeProfile::fantasy_mega()`, `::oak()`, etc.) produce different tree
// archetypes by tuning the same parameter set.
//
// Species-specific behavioral data (walk/climb speed as ticks-per-voxel,
// heartbeat interval, edge restrictions) lives in `SpeciesData` entries
// keyed by `Species` in the `species` map — see `species.rs`.
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
use crate::types::{FaceType, Species, VoxelType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Structural integrity — material and face properties
// ---------------------------------------------------------------------------

/// Per-voxel-type material properties for the structural solver.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaterialProperties {
    /// Mass per voxel of this material (unitless, relative).
    pub density: f32,
    /// Spring stiffness when two voxels of this material are face-adjacent.
    pub stiffness: f32,
    /// Maximum force a spring between two voxels of this material can sustain.
    pub strength: f32,
}

/// Per-face-type structural properties for building shell elements.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceProperties {
    /// Mass contribution of this face type to its parent BuildingInterior node.
    pub weight: f32,
    /// Spring stiffness for the face-to-neighbor connection.
    pub stiffness: f32,
    /// Maximum force the face spring can sustain.
    pub strength: f32,
}

/// Configuration for the spring-mass structural integrity solver.
///
/// All values are unitless and relative — only ratios between materials matter.
/// See `docs/drafts/structural_integrity.md` §5 for design rationale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StructuralConfig {
    /// Gravitational acceleration applied to each node (downward force = mass * gravity).
    pub gravity: f32,
    /// Fixed number of relaxation iterations per solve.
    pub max_iterations: u32,
    /// Damping scale factor. Internally scaled per-node as `damping_factor /
    /// local_stiffness` for Gauss-Seidel convergence. 1.0 is optimal; lower
    /// values under-relax for stability.
    pub damping_factor: f32,
    /// Base mass for BuildingInterior nodes (before face weight is added).
    pub building_interior_base_weight: f32,
    /// Stress ratio threshold for blueprint warnings (fraction of strength).
    pub warn_stress_ratio: f32,
    /// Stress ratio threshold for blueprint hard-block (multiple of strength).
    pub block_stress_ratio: f32,
    /// Maximum tree generation retry attempts before panicking.
    pub tree_gen_max_retries: u32,
    /// Per-voxel-type material properties.
    pub materials: BTreeMap<VoxelType, MaterialProperties>,
    /// Per-face-type structural properties.
    pub face_properties: BTreeMap<FaceType, FaceProperties>,
}

impl Default for StructuralConfig {
    fn default() -> Self {
        let mut materials = BTreeMap::new();
        materials.insert(
            VoxelType::Trunk,
            MaterialProperties {
                density: 1.0,
                stiffness: 50000.0,
                strength: 50000.0,
            },
        );
        materials.insert(
            VoxelType::Branch,
            MaterialProperties {
                density: 0.8,
                stiffness: 2000.0,
                strength: 2000.0,
            },
        );
        materials.insert(
            VoxelType::Root,
            MaterialProperties {
                density: 0.8,
                stiffness: 10000.0,
                strength: 10000.0,
            },
        );
        materials.insert(
            VoxelType::GrownPlatform,
            MaterialProperties {
                density: 0.6,
                stiffness: 20.0,
                strength: 8.0,
            },
        );
        materials.insert(
            VoxelType::GrownWall,
            MaterialProperties {
                density: 0.6,
                stiffness: 20.0,
                strength: 8.0,
            },
        );
        materials.insert(
            VoxelType::GrownStairs,
            MaterialProperties {
                density: 0.5,
                stiffness: 15.0,
                strength: 6.0,
            },
        );
        materials.insert(
            VoxelType::Bridge,
            MaterialProperties {
                density: 0.5,
                stiffness: 15.0,
                strength: 6.0,
            },
        );
        materials.insert(
            VoxelType::ForestFloor,
            MaterialProperties {
                density: 999.0,
                stiffness: 999.0,
                strength: 999.0,
            },
        );
        materials.insert(
            VoxelType::Dirt,
            MaterialProperties {
                density: 999.0,
                stiffness: 999.0,
                strength: 999.0,
            },
        );
        materials.insert(
            VoxelType::Leaf,
            MaterialProperties {
                density: 0.05,
                stiffness: 0.1,
                strength: 0.1,
            },
        );
        materials.insert(
            VoxelType::Fruit,
            MaterialProperties {
                density: 0.1,
                stiffness: 0.0,
                strength: 0.0,
            },
        );

        let mut face_props = BTreeMap::new();
        face_props.insert(
            FaceType::Wall,
            FaceProperties {
                weight: 0.3,
                stiffness: 15.0,
                strength: 10.0,
            },
        );
        face_props.insert(
            FaceType::Window,
            FaceProperties {
                weight: 0.1,
                stiffness: 3.0,
                strength: 2.0,
            },
        );
        face_props.insert(
            FaceType::Door,
            FaceProperties {
                weight: 0.15,
                stiffness: 1.0,
                strength: 1.0,
            },
        );
        face_props.insert(
            FaceType::Floor,
            FaceProperties {
                weight: 0.4,
                stiffness: 18.0,
                strength: 12.0,
            },
        );
        face_props.insert(
            FaceType::Ceiling,
            FaceProperties {
                weight: 0.3,
                stiffness: 15.0,
                strength: 10.0,
            },
        );
        face_props.insert(
            FaceType::Open,
            FaceProperties {
                weight: 0.0,
                stiffness: 0.0,
                strength: 0.0,
            },
        );

        Self {
            gravity: 1.0,
            max_iterations: 200,
            damping_factor: 1.0,
            building_interior_base_weight: 0.1,
            warn_stress_ratio: 0.5,
            block_stress_ratio: 1.0,
            tree_gen_max_retries: 4,
            materials,
            face_properties: face_props,
        }
    }
}

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
/// - `fantasy_mega()`: towering mega-tree (used in tests, not in UI)
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
    /// Number of real-world milliseconds per simulation tick. At 1000
    /// ticks/sec this is 1. Used by the GDScript frame loop to compute how
    /// many ticks to advance per frame.
    pub tick_duration_ms: u32,

    /// Interval (in ticks) between tree heartbeat events (fruit production,
    /// mana capacity updates).
    pub tree_heartbeat_interval_ticks: u64,

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

    /// Ticks of work per voxel during construction. An elf must accumulate
    /// this many activations-worth of work before one blueprint voxel
    /// materializes as solid.
    pub build_work_ticks_per_voxel: u64,

    /// Tree generation parameters — energy-based recursive growth profile.
    pub tree_profile: TreeProfile,

    /// Per-species behavioral data (speed, heartbeat interval, edge
    /// restrictions, spawn rules). Keyed by `Species` enum.
    pub species: BTreeMap<Species, SpeciesData>,

    /// Maximum height of dirt terrain above ForestFloor (1–4 voxels in
    /// practice). Set to 0 to disable terrain generation (backward compat
    /// for old saves — `#[serde(default)]` produces 0).
    #[serde(default)]
    pub terrain_max_height: i32,

    /// Noise cell size in voxels for terrain height generation. Larger values
    /// produce smoother, more gradual hills. Only used when
    /// `terrain_max_height > 0`.
    #[serde(default = "default_terrain_noise_scale")]
    pub terrain_noise_scale: f32,

    /// Structural integrity solver configuration. Backward-compatible: older
    /// configs without this field use `StructuralConfig::default()`.
    #[serde(default)]
    pub structural: StructuralConfig,
}

fn default_terrain_noise_scale() -> f32 {
    8.0
}

impl Default for GameConfig {
    fn default() -> Self {
        let mut species = BTreeMap::new();
        species.insert(
            Species::Elf,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: Some(1250),
                heartbeat_interval_ticks: 3000,
                allowed_edge_types: None, // elves can traverse all edges
                ground_only: false,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 3_333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                footprint: [1, 1, 1],
            },
        );
        species.insert(
            Species::Capybara,
            SpeciesData {
                walk_ticks_per_voxel: 800,
                climb_ticks_per_voxel: None,
                heartbeat_interval_ticks: 4000,
                allowed_edge_types: Some(vec![EdgeType::ForestFloor]),
                ground_only: true,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 3_333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                footprint: [1, 1, 1],
            },
        );
        species.insert(
            Species::Boar,
            SpeciesData {
                walk_ticks_per_voxel: 600,
                climb_ticks_per_voxel: None,
                heartbeat_interval_ticks: 4000,
                allowed_edge_types: Some(vec![EdgeType::ForestFloor]),
                ground_only: true,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 3_333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                footprint: [1, 1, 1],
            },
        );
        species.insert(
            Species::Deer,
            SpeciesData {
                walk_ticks_per_voxel: 400,
                climb_ticks_per_voxel: None,
                heartbeat_interval_ticks: 3500,
                allowed_edge_types: Some(vec![EdgeType::ForestFloor]),
                ground_only: true,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 3_333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                footprint: [1, 1, 1],
            },
        );
        species.insert(
            Species::Elephant,
            SpeciesData {
                walk_ticks_per_voxel: 700,
                climb_ticks_per_voxel: None,
                heartbeat_interval_ticks: 5000,
                allowed_edge_types: Some(vec![EdgeType::ForestFloor]),
                ground_only: true,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 3_333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                footprint: [2, 2, 2],
            },
        );
        species.insert(
            Species::Monkey,
            SpeciesData {
                walk_ticks_per_voxel: 550,
                climb_ticks_per_voxel: Some(800),
                heartbeat_interval_ticks: 3000,
                allowed_edge_types: None,
                ground_only: false,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 3_333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                footprint: [1, 1, 1],
            },
        );
        species.insert(
            Species::Squirrel,
            SpeciesData {
                walk_ticks_per_voxel: 700,
                climb_ticks_per_voxel: Some(600),
                heartbeat_interval_ticks: 2500,
                allowed_edge_types: None,
                ground_only: false,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 3_333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                footprint: [1, 1, 1],
            },
        );

        Self {
            tick_duration_ms: 1,
            tree_heartbeat_interval_ticks: 10000,
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
            build_work_ticks_per_voxel: 1000,
            tree_profile: TreeProfile::fantasy_mega(),
            species,
            terrain_max_height: 4,
            terrain_noise_scale: 8.0,
            structural: StructuralConfig::default(),
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
        // Verify species data survived (7 species: Elf, Capybara, Boar, Deer, Elephant, Monkey, Squirrel).
        assert_eq!(config.species.len(), 7);
        assert_eq!(config.species.len(), restored.species.len());
        let elf_data = &restored.species[&Species::Elf];
        assert_eq!(elf_data.heartbeat_interval_ticks, 3000);
        // Verify structural config survived.
        assert_eq!(config.structural.gravity, restored.structural.gravity);
        assert_eq!(
            config.structural.max_iterations,
            restored.structural.max_iterations
        );
        assert_eq!(
            config.structural.materials.len(),
            restored.structural.materials.len()
        );
        let trunk_mat = &restored.structural.materials[&VoxelType::Trunk];
        assert_eq!(trunk_mat.stiffness, 50000.0);
        assert_eq!(
            config.structural.face_properties.len(),
            restored.structural.face_properties.len()
        );
    }

    #[test]
    fn structural_config_backward_compatible() {
        // Old configs without "structural" should deserialize with defaults.
        let json = r#"{
            "tick_duration_ms": 1,
            "tree_heartbeat_interval_ticks": 10000,
            "mana_base_generation_rate": 1.0,
            "mana_mood_multiplier_range": [0.2, 2.0],
            "platform_mana_cost_per_voxel": 10.0,
            "bridge_mana_cost_per_voxel": 15.0,
            "fruit_production_base_rate": 0.5,
            "fruit_max_per_tree": 20,
            "fruit_initial_attempts": 12,
            "build_work_ticks_per_voxel": 1000,
            "world_size": [256, 128, 256],
            "floor_extent": 20,
            "starting_mana": 100.0,
            "starting_mana_capacity": 500.0,
            "tree_profile": {
                "growth": { "initial_energy": 200.0, "energy_to_radius": 0.07, "min_radius": 0.5, "growth_step_length": 1.0, "energy_per_step": 1.0 },
                "split": { "split_chance_base": 0.15, "split_count": 2, "split_energy_ratio": 0.3, "split_angle": 0.5, "split_angle_variance": 0.3, "min_progress_for_split": 0.1 },
                "curvature": { "gravitropism": -0.1, "random_deflection": 0.1, "deflection_coherence": 0.9 },
                "roots": { "root_energy_fraction": 0.1, "root_initial_count": 4, "root_gravitropism": 0.1, "root_initial_angle": 0.3, "root_surface_tendency": 0.7 },
                "leaves": { "leaf_shape": "Sphere", "leaf_density": 0.4, "leaf_size": 2, "canopy_density": 1.2 },
                "trunk": { "base_flare": 0.15, "initial_direction": [0.0, 1.0, 0.0] }
            },
            "species": {}
        }"#;
        let config: GameConfig = serde_json::from_str(json).unwrap();
        // structural should have defaulted.
        assert_eq!(config.structural.gravity, 1.0);
        assert_eq!(config.structural.max_iterations, 200);
        assert!(config.structural.materials.contains_key(&VoxelType::Trunk));
    }

    #[test]
    fn config_loads_from_json_string() {
        let json = r#"{
            "tick_duration_ms": 1,
            "tree_heartbeat_interval_ticks": 10000,
            "mana_base_generation_rate": 2.0,
            "mana_mood_multiplier_range": [0.1, 3.0],
            "platform_mana_cost_per_voxel": 8.0,
            "bridge_mana_cost_per_voxel": 12.0,
            "fruit_production_base_rate": 0.8,
            "fruit_max_per_tree": 25,
            "fruit_initial_attempts": 15,
            "build_work_ticks_per_voxel": 2000,
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
                    "walk_ticks_per_voxel": 500,
                    "climb_ticks_per_voxel": 1250,
                    "heartbeat_interval_ticks": 3000,
                    "allowed_edge_types": null,
                    "ground_only": false
                },
                "Capybara": {
                    "walk_ticks_per_voxel": 800,
                    "climb_ticks_per_voxel": null,
                    "heartbeat_interval_ticks": 4000,
                    "allowed_edge_types": ["ForestFloor"],
                    "ground_only": true
                }
            }
        }"#;
        let config: GameConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.tick_duration_ms, 1);
        assert_eq!(config.world_size, (128, 64, 128));
        assert_eq!(config.build_work_ticks_per_voxel, 2000);
        assert_eq!(config.tree_profile.growth.initial_energy, 300.0);
        let capy = &config.species[&Species::Capybara];
        assert_eq!(capy.heartbeat_interval_ticks, 4000);
        assert!(capy.ground_only);
    }

    #[test]
    fn species_data_hunger_fields_default_from_json() {
        // Old-format JSON without hunger fields — serde defaults must apply.
        let json = r#"{
            "walk_ticks_per_voxel": 500,
            "climb_ticks_per_voxel": 1250,
            "heartbeat_interval_ticks": 3000,
            "allowed_edge_types": null,
            "ground_only": false,
            "food_max": 1000000000000000,
            "food_decay_per_tick": 3333333333
        }"#;
        let data: SpeciesData = serde_json::from_str(json).unwrap();
        assert_eq!(data.food_hunger_threshold_pct, 50);
        assert_eq!(data.food_restore_pct, 40);
    }

    #[test]
    fn footprint_defaults_from_old_json() {
        // Old JSON without footprint field — serde default must apply [1,1,1].
        let json = r#"{
            "walk_ticks_per_voxel": 500,
            "climb_ticks_per_voxel": 1250,
            "heartbeat_interval_ticks": 3000,
            "allowed_edge_types": null,
            "ground_only": false,
            "food_max": 1000000000000000,
            "food_decay_per_tick": 3333333333
        }"#;
        let data: SpeciesData = serde_json::from_str(json).unwrap();
        assert_eq!(data.footprint, [1, 1, 1]);
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
