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
// See also: `sim/mod.rs` which owns the `GameConfig` as part of `SimState`,
// `species.rs` for the `SpeciesData` struct, `tree_gen.rs` for the
// energy-based recursive segment growth algorithm that reads `TreeProfile`.
//
// **Critical constraint: determinism.** Config values feed directly into
// simulation logic. All clients must use identical configs for identical
// results.

use crate::inventory::{ItemColor, ItemKind, Material, MaterialFilter};
use crate::nav::EdgeType;
use crate::species::{
    AmmoExhaustedBehavior, EngagementInitiative, EngagementStyle, SpeciesData, StatDistribution,
    WeaponPreference,
};
use crate::types::{
    CivSpecies, FaceType, MoodTier, Species, ThoughtKind, TraitKind, VoxelCoord, VoxelType,
};
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
    /// Per-link stiffness of rod springs along a strut's axis. The effective
    /// end-to-end stiffness of a strut is `k_link × spacing / N`, so longer
    /// struts are naturally more flexible (physically correct).
    pub strut_rod_stiffness: f32,
    /// Per-link strength of rod springs along a strut's axis.
    pub strut_rod_strength: f32,
    /// Spacing (in voxels) between rod spring connection points along a strut.
    /// E.g., 2 means connection points at every 2nd voxel. Lower spacing
    /// means more springs (`O(N/spacing)` per strut) and stiffer behavior.
    pub strut_rod_spacing: u32,
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
                stiffness: 60.0,
                strength: 24.0,
            },
        );
        materials.insert(
            VoxelType::GrownWall,
            MaterialProperties {
                density: 0.6,
                stiffness: 60.0,
                strength: 24.0,
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
        // Ladders are ~2% the mass of a solid wood voxel. Very low stiffness
        // so the weight-flow solver won't route structural load through them
        // (they're flexible, not load-bearing). Moderate strength relative to
        // their negligible weight so they easily support themselves.
        materials.insert(
            VoxelType::WoodLadder,
            MaterialProperties {
                density: 0.012,
                stiffness: 0.5,
                strength: 5.0,
            },
        );
        // Strut: sturdy player-built diagonal support. Face-adjacent springs
        // are slightly stiffer than GrownPlatform; the real structural benefit
        // comes from rod springs along the strut axis (see structural.rs).
        materials.insert(
            VoxelType::Strut,
            MaterialProperties {
                density: 0.6,
                stiffness: 25.0,
                strength: 12.0,
            },
        );
        materials.insert(
            VoxelType::RopeLadder,
            MaterialProperties {
                density: 0.005,
                stiffness: 0.2,
                strength: 2.0,
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
            strut_rod_stiffness: 150.0,
            strut_rod_strength: 150.0,
            strut_rod_spacing: 2,
            materials,
            face_properties: face_props,
        }
    }
}

// ---------------------------------------------------------------------------
// Thought config
// ---------------------------------------------------------------------------

/// Per-kind timing parameters for the creature thought system.
///
/// All durations are in ticks (1000 ticks = 1 sim second). Dedup cooldowns
/// prevent the same thought from being added twice within a window. Expiry
/// durations control how long a thought stays in the creature's thought list
/// before being cleaned up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThoughtConfig {
    /// Maximum number of thoughts a creature can hold. Oldest are dropped
    /// when this cap is exceeded.
    pub cap: usize,

    // --- Dedup cooldowns (ticks before an identical thought can be added again) ---
    pub dedup_slept_home_ticks: u64,
    pub dedup_slept_dormitory_ticks: u64,
    pub dedup_slept_ground_ticks: u64,
    pub dedup_ate_meal_ticks: u64,
    pub dedup_low_ceiling_ticks: u64,

    // --- Expiry durations (ticks after which a thought is removed) ---
    pub expiry_slept_home_ticks: u64,
    pub expiry_slept_dormitory_ticks: u64,
    pub expiry_slept_ground_ticks: u64,
    pub expiry_ate_meal_ticks: u64,
    pub expiry_low_ceiling_ticks: u64,
}

impl ThoughtConfig {
    /// Return the dedup cooldown ticks for a given thought kind.
    pub fn dedup_ticks(&self, kind: &ThoughtKind) -> u64 {
        match kind {
            ThoughtKind::SleptInOwnHome(_) => self.dedup_slept_home_ticks,
            ThoughtKind::SleptInDormitory(_) => self.dedup_slept_dormitory_ticks,
            ThoughtKind::SleptOnGround => self.dedup_slept_ground_ticks,
            ThoughtKind::AteMeal => self.dedup_ate_meal_ticks,
            ThoughtKind::LowCeiling(_) => self.dedup_low_ceiling_ticks,
        }
    }

    /// Return the expiry duration ticks for a given thought kind.
    pub fn expiry_ticks(&self, kind: &ThoughtKind) -> u64 {
        match kind {
            ThoughtKind::SleptInOwnHome(_) => self.expiry_slept_home_ticks,
            ThoughtKind::SleptInDormitory(_) => self.expiry_slept_dormitory_ticks,
            ThoughtKind::SleptOnGround => self.expiry_slept_ground_ticks,
            ThoughtKind::AteMeal => self.expiry_ate_meal_ticks,
            ThoughtKind::LowCeiling(_) => self.expiry_low_ceiling_ticks,
        }
    }
}

impl Default for ThoughtConfig {
    fn default() -> Self {
        Self {
            cap: 200,
            // 1 day cycle ≈ 150,000 ticks (150 sim-seconds ≈ 2.5 min real time).
            dedup_slept_home_ticks: 150_000,
            dedup_slept_dormitory_ticks: 150_000,
            dedup_slept_ground_ticks: 150_000,
            dedup_ate_meal_ticks: 150_000,
            // Low ceiling: reminder each visit (~30 sim-seconds).
            dedup_low_ceiling_ticks: 30_000,
            // Medium expiry (~10 min real time).
            expiry_slept_home_ticks: 600_000,
            expiry_slept_dormitory_ticks: 600_000,
            expiry_slept_ground_ticks: 600_000,
            // Shorter expiry (~2.5 min real time).
            expiry_ate_meal_ticks: 150_000,
            expiry_low_ceiling_ticks: 150_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Mood — per-ThoughtKind weights and tier thresholds
// ---------------------------------------------------------------------------

/// Configuration for deriving a mood score from a creature's active thoughts.
/// The score is the sum of per-ThoughtKind weights. Tier thresholds map the
/// numeric score to a `MoodTier` label. See `Creature::mood()` in `sim/needs.rs`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoodConfig {
    /// Weight for SleptInOwnHome thoughts.
    pub weight_slept_home: i32,
    /// Weight for SleptInDormitory thoughts.
    pub weight_slept_dormitory: i32,
    /// Weight for SleptOnGround thoughts.
    pub weight_slept_ground: i32,
    /// Weight for AteMeal thoughts.
    pub weight_ate_meal: i32,
    /// Weight for LowCeiling thoughts.
    pub weight_low_ceiling: i32,

    /// Scores at or below this are Devastated.
    pub tier_devastated_below: i32,
    /// Scores at or below this (but above devastated) are Miserable.
    pub tier_miserable_below: i32,
    /// Scores at or below this (but above miserable) are Unhappy.
    pub tier_unhappy_below: i32,
    /// Scores at or above this (but below happy) are Content.
    pub tier_content_above: i32,
    /// Scores at or above this (but below elated) are Happy.
    pub tier_happy_above: i32,
    /// Scores at or above this are Elated.
    pub tier_elated_above: i32,
}

impl MoodConfig {
    /// Return the mood weight for a given thought kind.
    pub fn mood_weight(&self, kind: &ThoughtKind) -> i32 {
        match kind {
            ThoughtKind::SleptInOwnHome(_) => self.weight_slept_home,
            ThoughtKind::SleptInDormitory(_) => self.weight_slept_dormitory,
            ThoughtKind::SleptOnGround => self.weight_slept_ground,
            ThoughtKind::AteMeal => self.weight_ate_meal,
            ThoughtKind::LowCeiling(_) => self.weight_low_ceiling,
        }
    }

    /// Map a numeric mood score to a `MoodTier`.
    pub fn tier(&self, score: i32) -> MoodTier {
        if score <= self.tier_devastated_below {
            MoodTier::Devastated
        } else if score <= self.tier_miserable_below {
            MoodTier::Miserable
        } else if score <= self.tier_unhappy_below {
            MoodTier::Unhappy
        } else if score >= self.tier_elated_above {
            MoodTier::Elated
        } else if score >= self.tier_happy_above {
            MoodTier::Happy
        } else if score >= self.tier_content_above {
            MoodTier::Content
        } else {
            MoodTier::Neutral
        }
    }
}

impl Default for MoodConfig {
    fn default() -> Self {
        Self {
            weight_slept_home: 80,
            weight_slept_dormitory: 30,
            weight_slept_ground: -100,
            weight_ate_meal: 60,
            weight_low_ceiling: -50,
            tier_devastated_below: -300,
            tier_miserable_below: -150,
            tier_unhappy_below: -30,
            tier_content_above: 30,
            tier_happy_above: 150,
            tier_elated_above: 300,
        }
    }
}

// ---------------------------------------------------------------------------
// Mood consequences — behavioral effects of mood tiers
// ---------------------------------------------------------------------------

/// Configuration for behavioral consequences of low mood. Currently: moping.
/// Unhappy+ creatures periodically abandon productive work to mope (idle at
/// home or current location). Probability is a Poisson-like process with
/// integer-only math: `roll % mean < elapsed` where `elapsed` is the heartbeat
/// interval and `mean` is the mean ticks between mope events for the tier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoodConsequencesConfig {
    /// Mean ticks between mope events at Unhappy tier. 0 = never.
    pub mope_mean_ticks_unhappy: u64,
    /// Mean ticks between mope events at Miserable tier. 0 = never.
    pub mope_mean_ticks_miserable: u64,
    /// Mean ticks between mope events at Devastated tier. 0 = never.
    pub mope_mean_ticks_devastated: u64,
    /// **Deprecated:** Superseded by the preemption system (`preemption.rs`).
    /// Retained for serde backward compatibility with old saves. No longer
    /// consulted by `check_mope()`.
    pub mope_can_interrupt_task: bool,
    /// Duration of mope idle in ticks.
    pub mope_duration_ticks: u64,
}

impl MoodConsequencesConfig {
    /// Return the mean ticks between mope events for the given mood tier.
    /// Returns 0 (never mope) for Neutral and positive tiers.
    pub fn mope_mean_ticks(&self, tier: MoodTier) -> u64 {
        match tier {
            MoodTier::Devastated => self.mope_mean_ticks_devastated,
            MoodTier::Miserable => self.mope_mean_ticks_miserable,
            MoodTier::Unhappy => self.mope_mean_ticks_unhappy,
            MoodTier::Neutral | MoodTier::Content | MoodTier::Happy | MoodTier::Elated => 0,
        }
    }
}

impl Default for MoodConsequencesConfig {
    fn default() -> Self {
        Self {
            mope_mean_ticks_unhappy: 500_000,
            mope_mean_ticks_miserable: 125_000,
            mope_mean_ticks_devastated: 50_000,
            mope_can_interrupt_task: true,
            mope_duration_ticks: 10_000,
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
// Lesser tree configuration
// ---------------------------------------------------------------------------

/// Configuration for lesser (non-sentient) tree placement during worldgen.
///
/// Lesser trees are normal-sized trees scattered across the forest floor,
/// providing ecological variety. They use the same energy-based generation
/// algorithm as the great tree but with much smaller profiles (real-life
/// scale at 2m/voxel). Placement uses rejection sampling: random XZ
/// positions are drawn within the forest floor bounds and rejected if too
/// close to the main tree or to an already-placed lesser tree.
///
/// See `worldgen.rs` for the placement algorithm, `tree_gen.rs` for the
/// generation algorithm.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LesserTreeConfig {
    /// Target number of lesser trees to place. Actual count may be lower if
    /// the placement area is too crowded to fit them all.
    pub count: u32,

    /// Minimum distance (in voxels) from the main tree center to any lesser
    /// tree trunk. Prevents lesser trees from spawning inside or adjacent to
    /// the great tree's root/trunk zone.
    pub min_distance_from_main: i32,

    /// Minimum distance (in voxels) between any two lesser tree trunks.
    /// Prevents canopy overlap between lesser trees.
    pub min_distance_between: i32,

    /// Maximum number of random position draws before giving up. Prevents
    /// infinite loops when the placement area is saturated.
    pub max_placement_attempts: u32,

    /// Tree profiles to randomly select from when generating each lesser
    /// tree. If empty, lesser tree generation is skipped.
    pub profiles: Vec<TreeProfile>,
}

impl Default for LesserTreeConfig {
    fn default() -> Self {
        Self {
            count: 500,
            min_distance_from_main: 25,
            min_distance_between: 12,
            max_placement_attempts: 7000,
            profiles: vec![
                TreeProfile::lesser_deciduous(),
                TreeProfile::lesser_conifer(),
                TreeProfile::lesser_tall_straight(),
                TreeProfile::lesser_thick_oak(),
                TreeProfile::lesser_bushy(),
                TreeProfile::lesser_sapling(),
            ],
        }
    }
}

impl TreeProfile {
    /// Small deciduous tree — real-life scale (~8-12 voxels / 16-24m tall).
    /// Broad crown with moderate branching. Suitable for lesser forest trees.
    /// Single-voxel trunk (min_radius < 0.5 prevents plus-sign cross-sections).
    pub fn lesser_deciduous() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 20.0,
                energy_to_radius: 0.01,
                min_radius: 0.3,
                growth_step_length: 1.0,
                energy_per_step: 1.0,
            },
            split: SplitParams {
                split_chance_base: 0.20,
                split_count: 1,
                split_energy_ratio: 0.35,
                split_angle: 0.8,
                split_angle_variance: 0.4,
                min_progress_for_split: 0.25,
            },
            curvature: CurvatureParams {
                gravitropism: 0.06,
                random_deflection: 0.15,
                deflection_coherence: 0.6,
            },
            roots: RootParams {
                root_energy_fraction: 0.0,
                root_initial_count: 0,
                root_gravitropism: 0.0,
                root_initial_angle: 0.0,
                root_surface_tendency: 0.0,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Cloud,
                leaf_density: 0.8,
                leaf_size: 3,
                canopy_density: 1.0,
            },
            trunk: TrunkParams {
                base_flare: 0.0,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Small conifer — real-life scale (~10-15 voxels / 20-30m tall).
    /// Narrow profile with strong central leader and small leaf clusters.
    /// Single-voxel trunk (min_radius < 0.5 prevents plus-sign cross-sections).
    pub fn lesser_conifer() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 25.0,
                energy_to_radius: 0.01,
                min_radius: 0.3,
                growth_step_length: 1.0,
                energy_per_step: 1.0,
            },
            split: SplitParams {
                split_chance_base: 0.10,
                split_count: 2,
                split_energy_ratio: 0.20,
                split_angle: 0.6,
                split_angle_variance: 0.2,
                min_progress_for_split: 0.10,
            },
            curvature: CurvatureParams {
                gravitropism: 0.12,
                random_deflection: 0.05,
                deflection_coherence: 0.8,
            },
            roots: RootParams {
                root_energy_fraction: 0.0,
                root_initial_count: 0,
                root_gravitropism: 0.0,
                root_initial_angle: 0.0,
                root_surface_tendency: 0.0,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Sphere,
                leaf_density: 0.7,
                leaf_size: 3,
                canopy_density: 1.0,
            },
            trunk: TrunkParams {
                base_flare: 0.0,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Tall straight tree — minimal branching, strong vertical growth (~15-20
    /// voxels tall). Like a young poplar or birch.
    pub fn lesser_tall_straight() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 30.0,
                energy_to_radius: 0.01,
                min_radius: 0.3,
                growth_step_length: 1.0,
                energy_per_step: 1.0,
            },
            split: SplitParams {
                split_chance_base: 0.06,
                split_count: 1,
                split_energy_ratio: 0.25,
                split_angle: 0.5,
                split_angle_variance: 0.2,
                min_progress_for_split: 0.40,
            },
            curvature: CurvatureParams {
                gravitropism: 0.15,
                random_deflection: 0.03,
                deflection_coherence: 0.9,
            },
            roots: RootParams {
                root_energy_fraction: 0.0,
                root_initial_count: 0,
                root_gravitropism: 0.0,
                root_initial_angle: 0.0,
                root_surface_tendency: 0.0,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Cloud,
                leaf_density: 0.7,
                leaf_size: 2,
                canopy_density: 1.0,
            },
            trunk: TrunkParams {
                base_flare: 0.0,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Thick old oak — wider trunk (plus-sign is OK here since it's a thick
    /// tree), heavy branching, large canopy. The biggest lesser tree variant.
    pub fn lesser_thick_oak() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 40.0,
                energy_to_radius: 0.05,
                min_radius: 0.5,
                growth_step_length: 1.0,
                energy_per_step: 1.2,
            },
            split: SplitParams {
                split_chance_base: 0.25,
                split_count: 1,
                split_energy_ratio: 0.40,
                split_angle: 0.9,
                split_angle_variance: 0.4,
                min_progress_for_split: 0.15,
            },
            curvature: CurvatureParams {
                gravitropism: 0.04,
                random_deflection: 0.18,
                deflection_coherence: 0.5,
            },
            roots: RootParams {
                root_energy_fraction: 0.0,
                root_initial_count: 0,
                root_gravitropism: 0.0,
                root_initial_angle: 0.0,
                root_surface_tendency: 0.0,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Cloud,
                leaf_density: 0.8,
                leaf_size: 3,
                canopy_density: 1.0,
            },
            trunk: TrunkParams {
                base_flare: 0.15,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Bushy shrub-tree — short and wide with aggressive branching. More bush
    /// than tree (~5-8 voxels tall), lots of leaves.
    pub fn lesser_bushy() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 12.0,
                energy_to_radius: 0.01,
                min_radius: 0.3,
                growth_step_length: 1.0,
                energy_per_step: 1.0,
            },
            split: SplitParams {
                split_chance_base: 0.35,
                split_count: 2,
                split_energy_ratio: 0.30,
                split_angle: 1.0,
                split_angle_variance: 0.5,
                min_progress_for_split: 0.10,
            },
            curvature: CurvatureParams {
                gravitropism: 0.02,
                random_deflection: 0.20,
                deflection_coherence: 0.4,
            },
            roots: RootParams {
                root_energy_fraction: 0.0,
                root_initial_count: 0,
                root_gravitropism: 0.0,
                root_initial_angle: 0.0,
                root_surface_tendency: 0.0,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Sphere,
                leaf_density: 0.9,
                leaf_size: 3,
                canopy_density: 1.2,
            },
            trunk: TrunkParams {
                base_flare: 0.0,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }

    /// Young sapling — very small and thin (~3-5 voxels tall), barely any
    /// branching. A sparse understory filler.
    pub fn lesser_sapling() -> Self {
        Self {
            growth: GrowthParams {
                initial_energy: 7.0,
                energy_to_radius: 0.01,
                min_radius: 0.3,
                growth_step_length: 1.0,
                energy_per_step: 1.0,
            },
            split: SplitParams {
                split_chance_base: 0.08,
                split_count: 1,
                split_energy_ratio: 0.30,
                split_angle: 0.6,
                split_angle_variance: 0.3,
                min_progress_for_split: 0.30,
            },
            curvature: CurvatureParams {
                gravitropism: 0.08,
                random_deflection: 0.10,
                deflection_coherence: 0.7,
            },
            roots: RootParams {
                root_energy_fraction: 0.0,
                root_initial_count: 0,
                root_gravitropism: 0.0,
                root_initial_angle: 0.0,
                root_surface_tendency: 0.0,
            },
            leaves: LeafParams {
                leaf_shape: LeafShape::Sphere,
                leaf_density: 0.6,
                leaf_size: 2,
                canopy_density: 0.8,
            },
            trunk: TrunkParams {
                base_flare: 0.0,
                initial_direction: [0.0, 1.0, 0.0],
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Initial spawning specs
// ---------------------------------------------------------------------------

/// Describes a group of creatures to spawn when the game starts. Per-creature
/// overrides (food, rest, bread) are indexed by creature index within the
/// group — any index beyond the vec length gets the species default.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitialCreatureSpec {
    pub species: Species,
    pub count: usize,
    pub spawn_position: VoxelCoord,
    #[serde(default)]
    pub food_pcts: Vec<u32>,
    #[serde(default)]
    pub rest_pcts: Vec<u32>,
    #[serde(default)]
    pub bread_counts: Vec<u32>,
    /// Per-creature starting equipment (indexed by creature index).
    /// Each inner Vec is a list of items to equip on that creature.
    #[serde(default)]
    pub initial_equipment: Vec<Vec<InitialEquipSpec>>,
}

/// Describes items to place on the ground when the game starts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitialGroundPileSpec {
    pub position: VoxelCoord,
    pub item_kind: ItemKind,
    pub quantity: u32,
    /// Optional material for the spawned items (e.g., Oak for wood armor).
    #[serde(default)]
    pub material: Option<Material>,
    /// Optional dye color for the spawned items.
    #[serde(default)]
    pub dye_color: Option<ItemColor>,
}

/// Describes an equipment item to give a starting creature.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitialEquipSpec {
    pub item_kind: ItemKind,
    #[serde(default)]
    pub material: Option<Material>,
    #[serde(default)]
    pub dye_color: Option<ItemColor>,
}

// ---------------------------------------------------------------------------
// Recipe system
// ---------------------------------------------------------------------------

/// An input requirement for a recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeInput {
    pub item_kind: ItemKind,
    pub quantity: u32,
    /// Material constraint for this input. `Any` (default) matches all materials.
    #[serde(default)]
    pub material_filter: MaterialFilter,
}

/// An output product from a recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeOutput {
    pub item_kind: ItemKind,
    pub quantity: u32,
    #[serde(default)]
    pub material: Option<Material>,
    #[serde(default)]
    pub quality: i32,
    /// If set, the crafted item gets this dye color on its `ItemStack`.
    /// Used by Press recipes to produce colored dye items.
    #[serde(default)]
    pub dye_color: Option<ItemColor>,
}

/// A subcomponent to attach to each output item. Records what components
/// went into crafting it (e.g., a Bow contains 1 Bowstring).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeSubcomponentRecord {
    pub input_kind: ItemKind,
    pub quantity_per_item: u32,
}

/// Parameters for component-based crafting recipes generated per fruit species.
///
/// Each recipe chain transforms extracted fruit components into useful items
/// based on the component's properties. Recipes are property-based: if a
/// fruit has a starchy part, it gets the flour/bread chain; if it has a
/// fine-fibrous part, it gets the thread chain, etc. Same-species constraint
/// applies — no mixing different fruit species in one recipe invocation.
///
/// See `recipe.rs` `build_component_recipes()` for the generation logic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentRecipeConfig {
    /// Mill: starchy component → flour. Work ticks per batch.
    #[serde(default = "default_mill_work_ticks")]
    pub mill_work_ticks: u64,
    /// Mill: units of starchy component consumed per batch.
    #[serde(default = "default_mill_input")]
    pub mill_input: u32,
    /// Mill: units of flour produced per batch.
    #[serde(default = "default_mill_output")]
    pub mill_output: u32,

    /// Bake: flour → bread. Work ticks per batch.
    #[serde(default = "default_bake_work_ticks")]
    pub bake_work_ticks: u64,
    /// Bake: units of flour consumed per loaf.
    #[serde(default = "default_bake_input")]
    pub bake_input: u32,
    /// Bake: loaves of bread produced per batch.
    #[serde(default = "default_bake_output")]
    pub bake_output: u32,

    /// Spin: fine-fibrous component → thread. Work ticks per batch.
    #[serde(default = "default_spin_work_ticks")]
    pub spin_work_ticks: u64,
    /// Spin: units of fine fiber consumed per batch.
    #[serde(default = "default_spin_input")]
    pub spin_input: u32,
    /// Spin: units of thread produced per batch.
    #[serde(default = "default_spin_output")]
    pub spin_output: u32,

    /// Twist: coarse-fibrous component → cord. Work ticks per batch.
    #[serde(default = "default_twist_work_ticks")]
    pub twist_work_ticks: u64,
    /// Twist: units of coarse fiber consumed per batch.
    #[serde(default = "default_twist_input")]
    pub twist_input: u32,
    /// Twist: units of cord produced per batch.
    #[serde(default = "default_twist_output")]
    pub twist_output: u32,

    /// Thread → bowstring. Work ticks.
    #[serde(default = "default_thread_bowstring_work_ticks")]
    pub thread_bowstring_work_ticks: u64,
    /// Thread → bowstring: units of thread consumed.
    #[serde(default = "default_thread_bowstring_input")]
    pub thread_bowstring_input: u32,
    /// Thread → bowstring: bowstrings produced.
    #[serde(default = "default_thread_bowstring_output")]
    pub thread_bowstring_output: u32,

    /// Cord → bowstring. Work ticks.
    #[serde(default = "default_cord_bowstring_work_ticks")]
    pub cord_bowstring_work_ticks: u64,
    /// Cord → bowstring: units of cord consumed.
    #[serde(default = "default_cord_bowstring_input")]
    pub cord_bowstring_input: u32,
    /// Cord → bowstring: bowstrings produced.
    #[serde(default = "default_cord_bowstring_output")]
    pub cord_bowstring_output: u32,

    /// Weave: thread → cloth. Work ticks per batch.
    #[serde(default = "default_weave_work_ticks")]
    pub weave_work_ticks: u64,
    /// Weave: units of thread consumed per cloth.
    #[serde(default = "default_weave_input")]
    pub weave_input: u32,
    /// Weave: units of cloth produced per batch.
    #[serde(default = "default_weave_output")]
    pub weave_output: u32,

    /// Sew tunic: cloth → tunic. Work ticks.
    #[serde(default = "default_sew_tunic_work_ticks")]
    pub sew_tunic_work_ticks: u64,
    /// Sew tunic: units of cloth consumed.
    #[serde(default = "default_sew_tunic_input")]
    pub sew_tunic_input: u32,
    /// Sew tunic: tunics produced.
    #[serde(default = "default_sew_tunic_output")]
    pub sew_tunic_output: u32,

    /// Sew leggings: cloth → leggings. Work ticks.
    #[serde(default = "default_sew_leggings_work_ticks")]
    pub sew_leggings_work_ticks: u64,
    /// Sew leggings: units of cloth consumed.
    #[serde(default = "default_sew_leggings_input")]
    pub sew_leggings_input: u32,
    /// Sew leggings: leggings produced.
    #[serde(default = "default_sew_leggings_output")]
    pub sew_leggings_output: u32,

    /// Sew boots: cloth → pair of boots. Work ticks.
    #[serde(default = "default_sew_boots_work_ticks")]
    pub sew_boots_work_ticks: u64,
    /// Sew boots: units of cloth consumed.
    #[serde(default = "default_sew_boots_input")]
    pub sew_boots_input: u32,
    /// Sew boots: pairs of boots produced.
    #[serde(default = "default_sew_boots_output")]
    pub sew_boots_output: u32,

    /// Sew hat: cloth → hat. Work ticks.
    #[serde(default = "default_sew_hat_work_ticks")]
    pub sew_hat_work_ticks: u64,
    /// Sew hat: units of cloth consumed.
    #[serde(default = "default_sew_hat_input")]
    pub sew_hat_input: u32,
    /// Sew hat: hats produced.
    #[serde(default = "default_sew_hat_output")]
    pub sew_hat_output: u32,

    /// Sew gloves: cloth → gloves. Work ticks.
    #[serde(default = "default_sew_gloves_work_ticks")]
    pub sew_gloves_work_ticks: u64,
    /// Sew gloves: units of cloth consumed.
    #[serde(default = "default_sew_gloves_input")]
    pub sew_gloves_input: u32,
    /// Sew gloves: gloves produced.
    #[serde(default = "default_sew_gloves_output")]
    pub sew_gloves_output: u32,

    /// Press: pigmented component → dye. Work ticks per batch.
    #[serde(default = "default_press_work_ticks")]
    pub press_work_ticks: u64,
    /// Press: units of pigmented component consumed per batch.
    #[serde(default = "default_press_input")]
    pub press_input: u32,
    /// Press: units of dye produced per batch.
    #[serde(default = "default_press_output")]
    pub press_output: u32,
}

impl Default for ComponentRecipeConfig {
    fn default() -> Self {
        Self {
            mill_work_ticks: default_mill_work_ticks(),
            mill_input: default_mill_input(),
            mill_output: default_mill_output(),
            bake_work_ticks: default_bake_work_ticks(),
            bake_input: default_bake_input(),
            bake_output: default_bake_output(),
            spin_work_ticks: default_spin_work_ticks(),
            spin_input: default_spin_input(),
            spin_output: default_spin_output(),
            twist_work_ticks: default_twist_work_ticks(),
            twist_input: default_twist_input(),
            twist_output: default_twist_output(),
            thread_bowstring_work_ticks: default_thread_bowstring_work_ticks(),
            thread_bowstring_input: default_thread_bowstring_input(),
            thread_bowstring_output: default_thread_bowstring_output(),
            cord_bowstring_work_ticks: default_cord_bowstring_work_ticks(),
            cord_bowstring_input: default_cord_bowstring_input(),
            cord_bowstring_output: default_cord_bowstring_output(),
            weave_work_ticks: default_weave_work_ticks(),
            weave_input: default_weave_input(),
            weave_output: default_weave_output(),
            sew_tunic_work_ticks: default_sew_tunic_work_ticks(),
            sew_tunic_input: default_sew_tunic_input(),
            sew_tunic_output: default_sew_tunic_output(),
            sew_leggings_work_ticks: default_sew_leggings_work_ticks(),
            sew_leggings_input: default_sew_leggings_input(),
            sew_leggings_output: default_sew_leggings_output(),
            sew_boots_work_ticks: default_sew_boots_work_ticks(),
            sew_boots_input: default_sew_boots_input(),
            sew_boots_output: default_sew_boots_output(),
            sew_hat_work_ticks: default_sew_hat_work_ticks(),
            sew_hat_input: default_sew_hat_input(),
            sew_hat_output: default_sew_hat_output(),
            sew_gloves_work_ticks: default_sew_gloves_work_ticks(),
            sew_gloves_input: default_sew_gloves_input(),
            sew_gloves_output: default_sew_gloves_output(),
            press_work_ticks: default_press_work_ticks(),
            press_input: default_press_input(),
            press_output: default_press_output(),
        }
    }
}

/// Parameters for wood-type Grow recipes generated per wood material.
///
/// Armor pieces, bows, and arrows are grown from the home tree's wood at
/// workshops. One recipe per wood type per item is generated at catalog build
/// time. Armor has zero inputs; bows consume one bowstring; arrows have zero
/// inputs.
///
/// See `recipe.rs` `build_wood_type_recipes()` for the generation logic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrowRecipeConfig {
    /// Duration of one work action for Grow recipes (ticks). The total recipe
    /// work_ticks is split into ceil(work_ticks / this) actions, each draining
    /// mana via `GameConfig::grow_mana_cost_per_mille`.
    #[serde(default = "default_grow_work_ticks_per_action")]
    pub grow_work_ticks_per_action: u64,

    /// Grow bow: work ticks.
    #[serde(default = "default_grow_bow_work_ticks")]
    pub grow_bow_work_ticks: u64,

    /// Grow arrow: work ticks.
    #[serde(default = "default_grow_arrow_work_ticks")]
    pub grow_arrow_work_ticks: u64,
    /// Grow arrow: quantity produced per batch.
    #[serde(default = "default_grow_arrow_output")]
    pub grow_arrow_output: u32,

    /// Grow helmet: work ticks.
    #[serde(default = "default_grow_helmet_work_ticks")]
    pub grow_helmet_work_ticks: u64,

    /// Grow breastplate: work ticks.
    #[serde(default = "default_grow_breastplate_work_ticks")]
    pub grow_breastplate_work_ticks: u64,

    /// Grow greaves: work ticks.
    #[serde(default = "default_grow_greaves_work_ticks")]
    pub grow_greaves_work_ticks: u64,

    /// Grow gauntlets: work ticks.
    #[serde(default = "default_grow_gauntlets_work_ticks")]
    pub grow_gauntlets_work_ticks: u64,

    /// Grow boots: work ticks.
    #[serde(default = "default_grow_boots_work_ticks")]
    pub grow_boots_work_ticks: u64,

    /// Grow spear: work ticks.
    #[serde(default = "default_grow_spear_work_ticks")]
    pub grow_spear_work_ticks: u64,

    /// Grow club: work ticks.
    #[serde(default = "default_grow_club_work_ticks")]
    pub grow_club_work_ticks: u64,
}

impl Default for GrowRecipeConfig {
    fn default() -> Self {
        Self {
            grow_work_ticks_per_action: default_grow_work_ticks_per_action(),
            grow_bow_work_ticks: default_grow_bow_work_ticks(),
            grow_arrow_work_ticks: default_grow_arrow_work_ticks(),
            grow_arrow_output: default_grow_arrow_output(),
            grow_helmet_work_ticks: default_grow_helmet_work_ticks(),
            grow_breastplate_work_ticks: default_grow_breastplate_work_ticks(),
            grow_greaves_work_ticks: default_grow_greaves_work_ticks(),
            grow_gauntlets_work_ticks: default_grow_gauntlets_work_ticks(),
            grow_boots_work_ticks: default_grow_boots_work_ticks(),
            grow_spear_work_ticks: default_grow_spear_work_ticks(),
            grow_club_work_ticks: default_grow_club_work_ticks(),
        }
    }
}

fn default_grow_work_ticks_per_action() -> u64 {
    1000
}

fn default_grow_bow_work_ticks() -> u64 {
    8000
}
fn default_grow_arrow_work_ticks() -> u64 {
    3000
}
fn default_grow_arrow_output() -> u32 {
    20
}
fn default_grow_helmet_work_ticks() -> u64 {
    7000
}
fn default_grow_breastplate_work_ticks() -> u64 {
    10000
}
fn default_grow_greaves_work_ticks() -> u64 {
    8000
}
fn default_grow_gauntlets_work_ticks() -> u64 {
    6000
}
fn default_grow_boots_work_ticks() -> u64 {
    6000
}
fn default_grow_spear_work_ticks() -> u64 {
    6000
}
fn default_grow_club_work_ticks() -> u64 {
    5000
}

fn default_mill_work_ticks() -> u64 {
    4000
}
fn default_mill_input() -> u32 {
    10
}
fn default_mill_output() -> u32 {
    10
}
fn default_bake_work_ticks() -> u64 {
    5000
}
fn default_bake_input() -> u32 {
    10
}
fn default_bake_output() -> u32 {
    1
}
fn default_spin_work_ticks() -> u64 {
    4000
}
fn default_spin_input() -> u32 {
    10
}
fn default_spin_output() -> u32 {
    5
}
fn default_twist_work_ticks() -> u64 {
    4000
}
fn default_twist_input() -> u32 {
    10
}
fn default_twist_output() -> u32 {
    5
}
fn default_thread_bowstring_work_ticks() -> u64 {
    3000
}
fn default_thread_bowstring_input() -> u32 {
    5
}
fn default_thread_bowstring_output() -> u32 {
    1
}
fn default_cord_bowstring_work_ticks() -> u64 {
    3000
}
fn default_cord_bowstring_input() -> u32 {
    5
}
fn default_cord_bowstring_output() -> u32 {
    1
}
fn default_weave_work_ticks() -> u64 {
    5000
}
fn default_weave_input() -> u32 {
    10
}
fn default_weave_output() -> u32 {
    1
}
fn default_sew_tunic_work_ticks() -> u64 {
    6000
}
fn default_sew_tunic_input() -> u32 {
    3
}
fn default_sew_tunic_output() -> u32 {
    1
}
fn default_sew_leggings_work_ticks() -> u64 {
    5000
}
fn default_sew_leggings_input() -> u32 {
    2
}
fn default_sew_leggings_output() -> u32 {
    1
}
fn default_sew_boots_work_ticks() -> u64 {
    5000
}
fn default_sew_boots_input() -> u32 {
    2
}
fn default_sew_boots_output() -> u32 {
    1
}
fn default_sew_hat_work_ticks() -> u64 {
    4000
}
fn default_sew_hat_input() -> u32 {
    1
}
fn default_sew_hat_output() -> u32 {
    1
}
fn default_sew_gloves_work_ticks() -> u64 {
    4000
}
fn default_sew_gloves_input() -> u32 {
    1
}
fn default_sew_gloves_output() -> u32 {
    1
}
fn default_press_work_ticks() -> u64 {
    3000
}
fn default_press_input() -> u32 {
    100
}
fn default_press_output() -> u32 {
    100
}

fn default_workshop_priority() -> u8 {
    8
}

fn default_greenhouse_default_priority() -> u8 {
    1
}

fn default_greenhouse_base_production_ticks() -> u64 {
    60_000
}

fn default_arrow_gravity() -> i64 {
    crate::projectile::EARTH_GRAVITY_SUB_VOXEL
}

fn default_arrow_damage_multiplier() -> i64 {
    20
}

fn default_arrow_base_speed() -> i64 {
    crate::projectile::SUB_VOXEL_ONE / 20 // ~50 voxels/sec
}

fn default_arrow_base_deviation_ppm() -> i64 {
    50_000 // 5% of range at DEX 0 (human baseline)
}

fn default_shoot_cooldown_ticks() -> u64 {
    3000 // 3 seconds between shots at 1000 ticks/sec
}

fn default_attack_path_retry_limit() -> u32 {
    3
}

fn default_defensive_pursuit_range_sq() -> i64 {
    25 // ~5 voxels
}

fn default_voxel_exclusion_retry_ticks() -> u64 {
    50
}

fn default_durability_worn_pct() -> i32 {
    70
}

fn default_durability_damaged_pct() -> i32 {
    40
}

fn default_arrow_impact_damage_min() -> i32 {
    0
}

fn default_arrow_impact_damage_max() -> i32 {
    3
}

fn default_armor_min_damage() -> i64 {
    1
}

fn default_armor_non_penetrating_degrade_chance_recip() -> i32 {
    20
}

fn default_armor_worn_penalty() -> i32 {
    1
}

fn default_armor_damaged_penalty() -> i32 {
    2
}

fn default_armor_degrade_location_weights() -> [i32; 5] {
    // [Torso, Legs, Head, Feet, Hands]
    [5, 4, 3, 2, 1]
}

fn default_item_durability() -> BTreeMap<ItemKind, i32> {
    BTreeMap::from([
        // Stackable projectiles — small range to keep stacking viable.
        (ItemKind::Arrow, 3),
        // Weapons — moderate durability.
        (ItemKind::Bow, 50),
        (ItemKind::Spear, 40),
        (ItemKind::Club, 50),
        // Armor — high durability.
        (ItemKind::Helmet, 40),
        (ItemKind::Breastplate, 60),
        (ItemKind::Greaves, 40),
        (ItemKind::Gauntlets, 30),
        // Clothing — moderate durability.
        (ItemKind::Tunic, 30),
        (ItemKind::Leggings, 25),
        (ItemKind::Boots, 20),
        (ItemKind::Hat, 20),
        (ItemKind::Gloves, 15),
    ])
}

fn default_spear_base_damage() -> i64 {
    15
}
fn default_spear_melee_range_sq() -> i64 {
    8
}
fn default_club_base_damage() -> i64 {
    20
}
fn default_club_melee_range_sq() -> i64 {
    3
}
fn default_melee_weapon_impact_damage_min() -> i32 {
    0
}
fn default_melee_weapon_impact_damage_max() -> i32 {
    2
}

fn default_fall_damage_per_voxel() -> i64 {
    10
}

// ---------------------------------------------------------------------------
// Civilization config
// ---------------------------------------------------------------------------

/// Configuration for the civilization worldgen generator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CivConfig {
    /// Number of civilizations to generate (including the player's elf civ).
    #[serde(default = "default_civ_count")]
    pub civ_count: u16,

    /// Weighted species distribution for NPC civs. The player's elf civ is
    /// always created first outside this distribution. Weights are relative —
    /// a species with weight 25 is 5x more likely than one with weight 5.
    #[serde(default = "default_species_weights")]
    pub species_weights: BTreeMap<CivSpecies, u16>,

    /// How many other civs the player's civ starts aware of.
    #[serde(default = "default_player_starting_known_civs")]
    pub player_starting_known_civs: u16,
}

fn default_civ_count() -> u16 {
    10
}

fn default_species_weights() -> BTreeMap<CivSpecies, u16> {
    let mut w = BTreeMap::new();
    w.insert(CivSpecies::Elf, 25);
    w.insert(CivSpecies::Human, 25);
    w.insert(CivSpecies::Dwarf, 20);
    w.insert(CivSpecies::Goblin, 15);
    w.insert(CivSpecies::Orc, 10);
    w.insert(CivSpecies::Troll, 5);
    w
}

fn default_player_starting_known_civs() -> u16 {
    5
}

impl Default for CivConfig {
    fn default() -> Self {
        Self {
            civ_count: default_civ_count(),
            species_weights: default_species_weights(),
            player_starting_known_civs: default_player_starting_known_civs(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fruit variety — worldgen config for procedural fruit species
// ---------------------------------------------------------------------------

/// Configuration for procedural fruit species generation during worldgen.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FruitConfig {
    /// Minimum number of fruit species to generate per world.
    pub min_species_per_world: u16,
    /// Maximum number of fruit species to generate per world.
    pub max_species_per_world: u16,
    /// Maximum number of separable parts per fruit (clamped to 4 internally).
    pub max_parts_per_fruit: u16,
    /// Rarity weights: `[common, uncommon, rare]`. Used for weighted random
    /// selection — higher values mean more likely.
    pub rarity_weights: [u32; 3],
    /// Per-coverage-category minimums. If a category isn't listed, its minimum
    /// is 0 (no guarantee). Keys are `CoverageCategory` variants.
    pub coverage_minimums: std::collections::BTreeMap<String, u16>,
    /// Temperature exponent for affinity-based fruit naming. Higher values
    /// make the naming algorithm more deterministic (strongly prefer the
    /// highest-scoring root); lower values allow more variety. Property
    /// roots use `2 * naming_temperature`, shape roots use `naming_temperature`.
    #[serde(default = "default_naming_temperature")]
    pub naming_temperature: u32,
}

impl FruitConfig {
    /// Look up the coverage minimum for a category, defaulting to 0.
    pub fn coverage_minimum(&self, cat: crate::fruit::CoverageCategory) -> u16 {
        let key = coverage_category_key(cat);
        self.coverage_minimums.get(&key).copied().unwrap_or(0)
    }
}

fn coverage_category_key(cat: crate::fruit::CoverageCategory) -> String {
    format!("{:?}", cat)
}

fn default_naming_temperature() -> u32 {
    2
}

impl Default for FruitConfig {
    fn default() -> Self {
        let mut minimums = std::collections::BTreeMap::new();
        // Gameplay-critical coverage categories and their minimum counts.
        minimums.insert("Starchy".into(), 3);
        minimums.insert("Sweet".into(), 3);
        minimums.insert("FibrousCoarse".into(), 2);
        minimums.insert("FibrousFine".into(), 2);
        minimums.insert("PigmentRed".into(), 1);
        minimums.insert("PigmentYellow".into(), 1);
        minimums.insert("PigmentBlue".into(), 1);
        minimums.insert("PigmentBlack".into(), 1);
        minimums.insert("PigmentWhite".into(), 1);
        minimums.insert("Fermentable".into(), 2);
        minimums.insert("Medicinal".into(), 1);
        minimums.insert("Aromatic".into(), 1);
        minimums.insert("Luminescent".into(), 1);
        minimums.insert("Psychoactive".into(), 1);
        minimums.insert("Stimulant".into(), 1);
        minimums.insert("ManaResonant".into(), 1);

        FruitConfig {
            min_species_per_world: 20,
            max_species_per_world: 40,
            max_parts_per_fruit: 4,
            rarity_weights: [60, 30, 10],
            coverage_minimums: minimums,
            naming_temperature: default_naming_temperature(),
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

    /// Base tree mana generated per elf per heartbeat, in millimana
    /// (1000 = 1.0 display mana).
    pub mana_base_generation_rate_mm: i64,

    /// Range of mood-based multipliers on mana generation, in per-mille
    /// (1000 = 1.0×). `(min_multiplier, max_multiplier)` — interpolated
    /// from worst to best mood.
    pub mana_mood_multiplier_range_permille: (i64, i64),

    /// Mana cost to grow one voxel of platform, in per-mille of the
    /// creature's mp_max (20 = 2%). Creature-scale cost = mp_max / 1000 × this.
    #[serde(default = "default_platform_mana_cost_per_mille")]
    pub platform_mana_cost_per_mille: u32,

    /// Default mana cost per work action for build types that don't have a
    /// specific config field (walls, struts, carving, ladders, furnishing),
    /// in per-mille of mp_max.
    #[serde(default = "default_mana_cost_per_mille")]
    pub default_mana_cost_per_mille: u32,

    /// Mana cost per work action for Grow-verb crafting recipes (magical
    /// shaping of wood at a workshop), in per-mille of mp_max.
    #[serde(default = "default_grow_mana_cost_per_mille")]
    pub grow_mana_cost_per_mille: u32,

    /// Number of consecutive wasted work actions (insufficient mana) before
    /// a creature abandons its current task. The task reverts to Available
    /// with all progress preserved.
    #[serde(default = "default_mana_abandon_threshold")]
    pub mana_abandon_threshold: u32,

    /// Fruit spawn chance per tree per heartbeat, in parts per million
    /// (500_000 = 50%).
    pub fruit_production_rate_ppm: u32,

    /// Maximum number of fruit a single tree can bear at once.
    pub fruit_max_per_tree: u32,

    /// Number of fruit-spawn attempts during tree initialization (fast-forward).
    /// Each attempt runs the same code path as heartbeat-driven spawning, so
    /// not all attempts will succeed (chance roll + valid position required).
    pub fruit_initial_attempts: u32,

    /// World dimensions in voxels (x, y, z).
    pub world_size: (u32, u32, u32),

    /// Y level of the forest floor. Tree trunk starts at `floor_y + 1`,
    /// creatures walk at `floor_y + 1`. Voxels below this are available
    /// for future underground content.
    #[serde(default = "default_floor_y")]
    pub floor_y: i32,

    /// Initial mana stored in the player's home tree, in millimana
    /// (1000 = 1.0 display mana).
    pub starting_mana_mm: i64,

    /// Maximum mana the starting tree can hold, in millimana.
    pub starting_mana_capacity_mm: i64,

    /// Ticks of work per voxel during construction. An elf must accumulate
    /// this many activations-worth of work before one blueprint voxel
    /// materializes as solid.
    pub build_work_ticks_per_voxel: u64,

    /// Ticks of work per voxel during carving (removal). An elf must
    /// accumulate this many activations-worth of work before one voxel is
    /// carved to Air. Defaults to `build_work_ticks_per_voxel` if absent.
    #[serde(default = "default_carve_ticks")]
    pub carve_work_ticks_per_voxel: u64,

    /// Ticks of work per furniture item when furnishing a building. An elf
    /// must accumulate this many activations-worth of work before one item
    /// is placed.
    #[serde(
        default = "default_furnish_ticks",
        alias = "furnish_work_ticks_per_bed"
    )]
    pub furnish_work_ticks_per_item: u64,

    /// Total ticks of sleep when resting in a dormitory bed (multi-activation
    /// task). Lower = faster sleep. Default 10_000 = 10 sim-seconds.
    #[serde(default = "default_sleep_ticks_bed")]
    pub sleep_ticks_bed: u64,

    /// Total ticks of sleep when resting on the ground (no bed available).
    /// Slower than bed sleep. Default 20_000 = 20 sim-seconds.
    #[serde(default = "default_sleep_ticks_ground")]
    pub sleep_ticks_ground: u64,

    /// Duration of one Sleep action in ticks (default 1000 = 1s).
    /// Sleep task repeats actions until rest is full.
    #[serde(default = "default_sleep_action_ticks")]
    pub sleep_action_ticks: u64,

    /// Duration of one Eat action in ticks (default 1500 = 1.5s).
    /// Covers both bread and fruit eating.
    #[serde(default = "default_eat_action_ticks")]
    pub eat_action_ticks: u64,

    /// Duration of one Harvest action in ticks (default 1500 = 1.5s).
    #[serde(default = "default_harvest_action_ticks")]
    pub harvest_action_ticks: u64,

    /// Duration of one AcquireItem action in ticks (default 1000 = 1s).
    #[serde(default = "default_acquire_item_action_ticks")]
    pub acquire_item_action_ticks: u64,

    /// Duration of one haul PickUp action in ticks (default 1000 = 1s).
    #[serde(default = "default_haul_pickup_action_ticks")]
    pub haul_pickup_action_ticks: u64,

    /// Duration of one haul DropOff action in ticks (default 1000 = 1s).
    #[serde(default = "default_haul_dropoff_action_ticks")]
    pub haul_dropoff_action_ticks: u64,

    /// Duration of one Mope action in ticks (default 1000 = 1s).
    #[serde(default = "default_mope_action_ticks")]
    pub mope_action_ticks: u64,

    /// Tree generation parameters — energy-based recursive growth profile.
    pub tree_profile: TreeProfile,

    /// Lesser tree placement configuration. Controls how many non-sentient
    /// trees are scattered across the forest floor during worldgen.
    #[serde(default)]
    pub lesser_trees: LesserTreeConfig,

    /// Per-species behavioral data (speed, heartbeat interval, edge
    /// restrictions, spawn rules). Keyed by `Species` enum.
    pub species: BTreeMap<Species, SpeciesData>,

    /// Maximum height of dirt terrain above the base floor_y level (1–4 voxels in
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

    /// Thought system timing configuration. Backward-compatible: older configs
    /// without this field use `ThoughtConfig::default()`.
    #[serde(default)]
    pub thoughts: ThoughtConfig,

    /// Mood system configuration: per-ThoughtKind weights and tier thresholds.
    /// Backward-compatible: older configs without this field use defaults.
    #[serde(default)]
    pub mood: MoodConfig,

    /// Mood consequences configuration: behavioral effects of low mood (moping).
    /// Backward-compatible: older configs without this field use defaults.
    #[serde(default)]
    pub mood_consequences: MoodConsequencesConfig,

    /// Ticks between logistics heartbeats that scan buildings for unmet wants
    /// and create haul tasks. Default 5000 = 5 sim-seconds.
    #[serde(default = "default_logistics_heartbeat_interval")]
    pub logistics_heartbeat_interval_ticks: u64,

    /// Maximum number of haul tasks created per logistics heartbeat. Prevents
    /// flooding the task queue with haul jobs.
    #[serde(default = "default_max_haul_tasks_per_heartbeat")]
    pub max_haul_tasks_per_heartbeat: u32,

    /// Number of bread items given to each elf on spawn.
    #[serde(default = "default_elf_starting_bread")]
    pub elf_starting_bread: u32,

    /// Number of bows given to each elf on spawn (0 or 1).
    #[serde(default = "default_elf_starting_bows")]
    pub elf_starting_bows: u32,

    /// Number of arrows given to each elf on spawn.
    #[serde(default = "default_elf_starting_arrows")]
    pub elf_starting_arrows: u32,

    /// Default personal item desires for newly spawned elves. Each entry is an
    /// `(item_kind, target_quantity)` pair — idle elves create `AcquireItem`
    /// tasks to maintain these quantities in their personal inventory.
    #[serde(default = "default_elf_default_wants")]
    pub elf_default_wants: Vec<crate::building::LogisticsWant>,

    /// Default logistics priority for newly furnished storehouses.
    #[serde(default = "default_storehouse_default_priority")]
    pub storehouse_default_priority: u8,

    /// Default fruit want for newly furnished storehouses.
    #[serde(default = "default_storehouse_default_fruit_want")]
    pub storehouse_default_fruit_want: u32,

    /// Default bread want for newly furnished storehouses.
    #[serde(default = "default_storehouse_default_bread_want")]
    pub storehouse_default_bread_want: u32,

    /// Default logistics priority for newly furnished kitchens.
    #[serde(default = "default_kitchen_default_priority")]
    pub kitchen_default_priority: u8,

    /// Default fruit want for newly furnished kitchens.
    #[serde(default = "default_kitchen_default_fruit_want")]
    pub kitchen_default_fruit_want: u32,

    /// Default bread target for newly furnished kitchens.
    #[serde(default = "default_kitchen_default_bread_target")]
    pub kitchen_default_bread_target: u32,

    /// Ticks of work to complete one fruit extraction (3000 = 3 sim-seconds).
    #[serde(default = "default_extract_work_ticks")]
    pub extract_work_ticks: u64,

    /// Parameters for component-based crafting recipes (flour, thread, cord,
    /// and their downstream products). Recipes are dynamically generated per
    /// fruit species based on part properties.
    #[serde(default)]
    pub component_recipes: ComponentRecipeConfig,

    /// Parameters for wood-type Grow recipes (armor, bows, arrows). Recipes
    /// are dynamically generated per wood material at catalog build time.
    #[serde(default)]
    pub grow_recipes: GrowRecipeConfig,

    /// Creatures to spawn when a new game starts. Each spec describes a group
    /// of one species with optional per-creature food/rest/bread overrides.
    #[serde(default)]
    pub initial_creatures: Vec<InitialCreatureSpec>,

    /// Ground item piles to place when a new game starts.
    #[serde(default)]
    pub initial_ground_piles: Vec<InitialGroundPileSpec>,

    /// Default logistics priority for newly furnished workshops.
    #[serde(default = "default_workshop_priority")]
    pub workshop_default_priority: u8,

    /// Default logistics priority for newly furnished greenhouses. Low so that
    /// storehouses, kitchens, and workshops can pull fruit from them.
    #[serde(default = "default_greenhouse_default_priority")]
    pub greenhouse_default_priority: u8,

    /// Greenhouse base production interval in ticks for a single interior
    /// tile. Actual interval = base / max(1, interior_area). E.g., 60000
    /// means a 1-tile greenhouse produces one fruit per 60 sim-seconds; a
    /// 4-tile greenhouse produces one per 15 sim-seconds.
    #[serde(default = "default_greenhouse_base_production_ticks")]
    pub greenhouse_base_production_ticks: u64,

    /// Worldgen generator configuration. Groups config for generators that run
    /// during world creation (fruit variety, civilizations, knowledge). The
    /// existing tree profile stays at the top level for backward compatibility.
    #[serde(default)]
    pub worldgen: crate::worldgen::WorldgenConfig,

    /// Projectile gravity in sub-voxel units per tick² (positive = downward).
    /// Defaults to `EARTH_GRAVITY_SUB_VOXEL` (5267), calibrated for 2m voxels
    /// at 1000 ticks/sec. Override for gameplay tuning.
    #[serde(default = "default_arrow_gravity")]
    pub arrow_gravity: i64,

    /// Arrow launch speed in sub-voxel units per tick. Defaults to
    /// `SUB_VOXEL_ONE / 20` (~50 voxels/sec = 25 m/s for a modest bow).
    #[serde(default = "default_arrow_base_speed")]
    pub arrow_base_speed: i64,

    /// Base arrow aim deviation at DEX 0, in parts-per-million of the
    /// velocity magnitude. Applied as random lateral offsets to the aim
    /// vector. The actual deviation is `base * (1 << 20) / stat_multiplier(dex)`,
    /// so higher DEX reduces it. Default 50000 = 5% of velocity magnitude.
    #[serde(default = "default_arrow_base_deviation_ppm")]
    pub arrow_base_deviation_ppm: i64,

    /// Multiplier applied to arrow impact damage. The base formula yields
    /// damage = impact_speed / reference_speed (≈1 at normal launch speed);
    /// this multiplier scales the result before applying.
    #[serde(default = "default_arrow_damage_multiplier")]
    pub arrow_damage_multiplier: i64,

    /// Cooldown in ticks between ranged shots (global, all species).
    /// At 1000 ticks/sec, 3000 = 3 seconds between shots.
    #[serde(default = "default_shoot_cooldown_ticks")]
    pub shoot_cooldown_ticks: u64,

    /// Maximum consecutive pathfinding failures before an AttackTarget task
    /// is cancelled. The creature returns to normal behavior and may re-detect
    /// the target later if it moves to a reachable location.
    #[serde(default = "default_attack_path_retry_limit")]
    pub attack_path_retry_limit: u32,

    /// Maximum squared distance a defensive creature will pursue targets.
    /// Defensive creatures only chase within this radius of their current
    /// position — they won't cross the map to engage. Default 25 (~5 voxels).
    #[serde(default = "default_defensive_pursuit_range_sq")]
    pub defensive_pursuit_range_sq: i64,

    /// Ticks to wait before retrying movement when a creature's destination
    /// voxel is occupied by a hostile creature (voxel exclusion).
    #[serde(default = "default_voxel_exclusion_retry_ticks")]
    pub voxel_exclusion_retry_ticks: u64,

    /// Per-item-kind max hit points for the durability system. Items not in
    /// this map have no durability tracking (indestructible). Stackable items
    /// like arrows should use small values (e.g., 3) to keep stacking viable;
    /// equipment like armor and weapons can use larger values.
    #[serde(default = "default_item_durability")]
    pub item_durability: BTreeMap<ItemKind, i32>,

    /// HP percentage at or below which an item is labelled "(worn)" in its
    /// display name. Should be > `durability_damaged_pct`; if not, the worn
    /// range becomes empty and items jump straight from healthy to damaged.
    /// Default 70.
    #[serde(default = "default_durability_worn_pct")]
    pub durability_worn_pct: i32,

    /// HP percentage at or below which an item is labelled "(damaged)" instead
    /// of "(worn)". Default 40.
    #[serde(default = "default_durability_damaged_pct")]
    pub durability_damaged_pct: i32,

    /// Minimum durability damage an arrow takes on impact (creature or
    /// surface). Actual damage is uniform random in
    /// `[arrow_impact_damage_min, arrow_impact_damage_max]`. Default 0.
    #[serde(default = "default_arrow_impact_damage_min")]
    pub arrow_impact_damage_min: i32,

    /// Maximum durability damage an arrow takes on impact. Default 3.
    #[serde(default = "default_arrow_impact_damage_max")]
    pub arrow_impact_damage_max: i32,

    // -- Armor damage reduction --
    /// Minimum damage a creature takes per hit after armor reduction.
    /// Armor can never reduce incoming damage below this value. Default 1.
    #[serde(default = "default_armor_min_damage")]
    pub armor_min_damage: i64,

    /// Reciprocal of the chance that armor/clothing degrades on a
    /// non-penetrating hit (raw damage ≤ total armor). 1-in-N chance per
    /// hit. Set to 0 to disable non-penetrating degradation. Default 20.
    #[serde(default = "default_armor_non_penetrating_degrade_chance_recip")]
    pub armor_non_penetrating_degrade_chance_recip: i32,

    /// Armor value penalty when an armor piece is in "worn" condition
    /// (HP% ≤ `durability_worn_pct`). Effective value = base - penalty,
    /// floored at 2. Default 1.
    #[serde(default = "default_armor_worn_penalty")]
    pub armor_worn_penalty: i32,

    /// Armor value penalty when an armor piece is in "damaged" condition
    /// (HP% ≤ `durability_damaged_pct`). Effective value = base - penalty,
    /// floored at 1. Default 2.
    #[serde(default = "default_armor_damaged_penalty")]
    pub armor_damaged_penalty: i32,

    /// Weighted random target location weights for armor degradation on
    /// hit: \[Torso, Legs, Head, Feet, Hands\]. Higher weight = more likely
    /// to be targeted. When the chosen location has nothing equipped, no
    /// degradation occurs. Default \[5, 4, 3, 2, 1\].
    #[serde(default = "default_armor_degrade_location_weights")]
    pub armor_degrade_location_weights: [i32; 5],

    // -- Melee weapon stats --
    /// Base melee damage dealt by a spear, replacing species base damage.
    /// STR scaling still applies on top. Default 15.
    #[serde(default = "default_spear_base_damage")]
    pub spear_base_damage: i64,

    /// Squared melee range for spear attacks (extended reach). Default 8.
    #[serde(default = "default_spear_melee_range_sq")]
    pub spear_melee_range_sq: i64,

    /// Base melee damage dealt by a club, replacing species base damage.
    /// STR scaling still applies on top. Default 20.
    #[serde(default = "default_club_base_damage")]
    pub club_base_damage: i64,

    /// Squared melee range for club attacks (normal reach). Default 3.
    #[serde(default = "default_club_melee_range_sq")]
    pub club_melee_range_sq: i64,

    /// Minimum durability damage a melee weapon takes per strike. Default 0.
    #[serde(default = "default_melee_weapon_impact_damage_min")]
    pub melee_weapon_impact_damage_min: i32,

    /// Maximum durability damage a melee weapon takes per strike. Default 2.
    #[serde(default = "default_melee_weapon_impact_damage_max")]
    pub melee_weapon_impact_damage_max: i32,

    // -- Creature gravity (F-creature-gravity) --
    /// HP damage per voxel fallen when a creature falls due to gravity.
    /// Set to 0 to disable fall damage. Default 10.
    #[serde(default = "default_fall_damage_per_voxel")]
    pub fall_damage_per_voxel: i64,
}

fn default_platform_mana_cost_per_mille() -> u32 {
    20
}

fn default_mana_cost_per_mille() -> u32 {
    20
}

fn default_grow_mana_cost_per_mille() -> u32 {
    20
}

fn default_mana_abandon_threshold() -> u32 {
    3
}

fn default_carve_ticks() -> u64 {
    1000
}

fn default_furnish_ticks() -> u64 {
    2000
}

fn default_sleep_ticks_bed() -> u64 {
    10_000
}

fn default_sleep_ticks_ground() -> u64 {
    20_000
}

fn default_terrain_noise_scale() -> f32 {
    8.0
}

fn default_floor_y() -> i32 {
    50
}

fn default_logistics_heartbeat_interval() -> u64 {
    5000
}

fn default_max_haul_tasks_per_heartbeat() -> u32 {
    5
}

fn default_elf_starting_bread() -> u32 {
    2
}

fn default_elf_starting_bows() -> u32 {
    0
}

fn default_elf_starting_arrows() -> u32 {
    0
}

fn default_elf_default_wants() -> Vec<crate::building::LogisticsWant> {
    use crate::building::LogisticsWant;
    use crate::inventory::{ItemKind, MaterialFilter};
    vec![
        LogisticsWant {
            item_kind: ItemKind::Bread,
            material_filter: MaterialFilter::Any,
            target_quantity: 2,
        },
        LogisticsWant {
            item_kind: ItemKind::Tunic,
            material_filter: MaterialFilter::Any,
            target_quantity: 1,
        },
        LogisticsWant {
            item_kind: ItemKind::Leggings,
            material_filter: MaterialFilter::Any,
            target_quantity: 1,
        },
        LogisticsWant {
            item_kind: ItemKind::Boots,
            material_filter: MaterialFilter::NonWood,
            target_quantity: 1,
        },
        LogisticsWant {
            item_kind: ItemKind::Hat,
            material_filter: MaterialFilter::Any,
            target_quantity: 1,
        },
        LogisticsWant {
            item_kind: ItemKind::Gloves,
            material_filter: MaterialFilter::Any,
            target_quantity: 1,
        },
    ]
}

fn default_storehouse_default_priority() -> u8 {
    2
}

fn default_storehouse_default_fruit_want() -> u32 {
    10
}

fn default_storehouse_default_bread_want() -> u32 {
    20
}

fn default_kitchen_default_priority() -> u8 {
    8
}

fn default_kitchen_default_fruit_want() -> u32 {
    5
}

fn default_kitchen_default_bread_target() -> u32 {
    50
}

fn default_sleep_action_ticks() -> u64 {
    1000
}

fn default_eat_action_ticks() -> u64 {
    1500
}

fn default_harvest_action_ticks() -> u64 {
    1500
}

fn default_acquire_item_action_ticks() -> u64 {
    1000
}

fn default_haul_pickup_action_ticks() -> u64 {
    1000
}

fn default_haul_dropoff_action_ticks() -> u64 {
    1000
}

fn default_mope_action_ticks() -> u64 {
    1000
}

fn default_extract_work_ticks() -> u64 {
    3000
}

/// Build a stat distribution map from (TraitKind, mean, stdev) triples.
/// Stats not listed default to (0, 50) at query time in `roll_creature_traits`.
fn stat_dists(entries: &[(TraitKind, i32, i32)]) -> BTreeMap<TraitKind, StatDistribution> {
    entries
        .iter()
        .map(|&(kind, mean, stdev)| (kind, StatDistribution { mean, stdev }))
        .collect()
}
impl Default for GameConfig {
    fn default() -> Self {
        let mut species = BTreeMap::new();
        species.insert(
            Species::Elf,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: Some(1250),
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 3000,
                allowed_edge_types: None, // elves can traverse all edges
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: Some(750),
                rope_ladder_tpv: Some(900),
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 333_333_333,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 10,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle {
                    weapon_preference: WeaponPreference::PreferRanged,
                    ammo_exhausted: AmmoExhaustedBehavior::Flee,
                    initiative: EngagementInitiative::Defensive,
                    disengage_threshold_pct: 100,
                },
                hostile_detection_range_sq: 225, // 15-voxel detection radius
                mp_max: 707_106_972_102_150, // base; WIL scales at spawn (median WIL=50 → ~1e15)
                mana_per_tick: 2_526_194_874, // base; avg(WIL,INT) scales at heartbeat (median avg=40 → ~3.3e9)
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 0, 40),
                    (TraitKind::Agility, 20, 40),
                    (TraitKind::Dexterity, 50, 40),
                    (TraitKind::Constitution, 0, 40),
                    (TraitKind::Willpower, 50, 40),
                    (TraitKind::Intelligence, 30, 40),
                    (TraitKind::Perception, 10, 40),
                    (TraitKind::Charisma, 30, 40),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Capybara,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: None,
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 4000,
                allowed_edge_types: Some(vec![EdgeType::Ground]),
                ground_only: true,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: None,
                rope_ladder_tpv: None,
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 0,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle::default(),
                hostile_detection_range_sq: 0,
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, -100, 40),
                    (TraitKind::Agility, -68, 40),
                    (TraitKind::Dexterity, -100, 40),
                    (TraitKind::Constitution, -74, 40),
                    (TraitKind::Willpower, 100, 40),
                    (TraitKind::Intelligence, -100, 40),
                    (TraitKind::Perception, 50, 40),
                    (TraitKind::Charisma, 150, 40),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Boar,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: None,
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 4000,
                allowed_edge_types: Some(vec![EdgeType::Ground]),
                ground_only: true,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: None,
                rope_ladder_tpv: None,
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 0,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle::default(),
                hostile_detection_range_sq: 0,
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 50, 40),
                    (TraitKind::Agility, -26, 40),
                    (TraitKind::Dexterity, -50, 40),
                    (TraitKind::Constitution, -32, 40),
                    (TraitKind::Willpower, -30, 40),
                    (TraitKind::Intelligence, -100, 40),
                    (TraitKind::Perception, 30, 40),
                    (TraitKind::Charisma, -100, 40),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Deer,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: None,
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 3500,
                allowed_edge_types: Some(vec![EdgeType::Ground]),
                ground_only: true,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: None,
                rope_ladder_tpv: None,
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 0,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle::default(),
                hostile_detection_range_sq: 0,
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, -50, 40),
                    (TraitKind::Agility, 32, 40),
                    (TraitKind::Dexterity, 30, 40),
                    (TraitKind::Constitution, -100, 40),
                    (TraitKind::Willpower, 0, 40),
                    (TraitKind::Intelligence, -50, 40),
                    (TraitKind::Perception, 100, 40),
                    (TraitKind::Charisma, 0, 40),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Elephant,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: None,
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 5000,
                allowed_edge_types: Some(vec![EdgeType::Ground]),
                ground_only: true,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [2, 2, 2],
                wood_ladder_tpv: None,
                rope_ladder_tpv: None,
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 0,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle::default(),
                hostile_detection_range_sq: 0,
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 250, 50),
                    (TraitKind::Agility, -49, 50),
                    (TraitKind::Dexterity, -100, 50),
                    (TraitKind::Constitution, 158, 50),
                    (TraitKind::Willpower, 50, 50),
                    (TraitKind::Intelligence, 50, 50),
                    (TraitKind::Perception, 30, 50),
                    (TraitKind::Charisma, 50, 50),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Goblin,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: Some(2500), // 2x slower than elf
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 3000,
                allowed_edge_types: None,
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 0,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: Some(800),
                rope_ladder_tpv: Some(950),
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 10,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle {
                    weapon_preference: WeaponPreference::PreferMelee,
                    ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
                    initiative: EngagementInitiative::Aggressive,
                    disengage_threshold_pct: 0,
                },
                hostile_detection_range_sq: 225, // 15-voxel detection radius
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 58, 40),
                    (TraitKind::Agility, 15, 40),
                    (TraitKind::Dexterity, 30, 40),
                    (TraitKind::Constitution, -32, 40),
                    (TraitKind::Willpower, -30, 40),
                    (TraitKind::Intelligence, -50, 40),
                    (TraitKind::Perception, 0, 40),
                    (TraitKind::Charisma, -50, 40),
                ]),
                raid_size: 6,
            },
        );
        species.insert(
            Species::Monkey,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: Some(800),
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 3000,
                allowed_edge_types: None,
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: Some(600),
                rope_ladder_tpv: Some(700),
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 0,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle::default(),
                hostile_detection_range_sq: 0,
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, -50, 40),
                    (TraitKind::Agility, -14, 40),
                    (TraitKind::Dexterity, 100, 40),
                    (TraitKind::Constitution, -132, 40),
                    (TraitKind::Willpower, 0, 40),
                    (TraitKind::Intelligence, 50, 40),
                    (TraitKind::Perception, 50, 40),
                    (TraitKind::Charisma, 50, 40),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Orc,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: Some(5000), // 2x slower than goblin
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 3000,
                allowed_edge_types: None,
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 0,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: Some(800),
                rope_ladder_tpv: Some(950),
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 10,
                melee_interval_ticks: 1200,
                melee_range_sq: 3,
                engagement_style: EngagementStyle {
                    weapon_preference: WeaponPreference::PreferMelee,
                    ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
                    initiative: EngagementInitiative::Aggressive,
                    disengage_threshold_pct: 0,
                },
                hostile_detection_range_sq: 225, // 15-voxel detection radius
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 132, 40),
                    (TraitKind::Agility, -26, 40),
                    (TraitKind::Dexterity, -50, 40),
                    (TraitKind::Constitution, 58, 40),
                    (TraitKind::Willpower, 30, 40),
                    (TraitKind::Intelligence, -80, 40),
                    (TraitKind::Perception, 42, 40),
                    (TraitKind::Charisma, -50, 40),
                ]),
                raid_size: 3,
            },
        );
        species.insert(
            Species::Squirrel,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: Some(600),
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 2500,
                allowed_edge_types: None,
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 333_333_333,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 40,
                bread_restore_pct: 30,
                footprint: [1, 1, 1],
                wood_ladder_tpv: Some(500),
                rope_ladder_tpv: Some(600),
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 0,
                melee_interval_ticks: 1000,
                melee_range_sq: 3,
                engagement_style: EngagementStyle::default(),
                hostile_detection_range_sq: 0,
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, -150, 40),
                    (TraitKind::Agility, -49, 40),
                    (TraitKind::Dexterity, 100, 40),
                    (TraitKind::Constitution, -232, 40),
                    (TraitKind::Willpower, 0, 40),
                    (TraitKind::Intelligence, 30, 40),
                    (TraitKind::Perception, 100, 40),
                    (TraitKind::Charisma, 30, 40),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Troll,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: Some(5000),
                flight_ticks_per_voxel: None,
                heartbeat_interval_ticks: 5000,
                allowed_edge_types: None,
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 500, // 2 HP/sec at 1000 ticks/sec
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 0,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 0, // don't eat fruit
                bread_restore_pct: 0,
                footprint: [2, 2, 2],
                wood_ladder_tpv: Some(3000), // slow on ladders
                rope_ladder_tpv: Some(3500),
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 10,
                melee_interval_ticks: 1500,
                melee_range_sq: 3,
                engagement_style: EngagementStyle {
                    weapon_preference: WeaponPreference::PreferMelee,
                    ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
                    initiative: EngagementInitiative::Aggressive,
                    disengage_threshold_pct: 0,
                },
                hostile_detection_range_sq: 225, // 15-voxel detection radius
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 232, 50),
                    (TraitKind::Agility, -26, 50),
                    (TraitKind::Dexterity, -50, 50),
                    (TraitKind::Constitution, 200, 50),
                    (TraitKind::Willpower, 50, 50),
                    (TraitKind::Intelligence, -150, 50),
                    (TraitKind::Perception, -32, 50),
                    (TraitKind::Charisma, -100, 50),
                ]),
                raid_size: 2,
            },
        );
        species.insert(
            Species::Hornet,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: None,
                flight_ticks_per_voxel: Some(250), // fast flyer: 4 voxels/sec
                heartbeat_interval_ticks: 2000,
                allowed_edge_types: None,
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 0,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 0,
                bread_restore_pct: 0,
                footprint: [1, 1, 1],
                wood_ladder_tpv: None,
                rope_ladder_tpv: None,
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 10,
                melee_interval_ticks: 800, // fast stinger
                melee_range_sq: 3,
                engagement_style: EngagementStyle {
                    weapon_preference: WeaponPreference::PreferMelee,
                    ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
                    initiative: EngagementInitiative::Aggressive,
                    disengage_threshold_pct: 30, // flees when hurt
                },
                hostile_detection_range_sq: 225, // 15-voxel detection radius
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 100, 30),
                    (TraitKind::Agility, -26, 40),
                    (TraitKind::Dexterity, 50, 30),
                    (TraitKind::Constitution, -74, 30),
                    (TraitKind::Willpower, 0, 30),
                    (TraitKind::Intelligence, -100, 30),
                    (TraitKind::Perception, -10, 40),
                    (TraitKind::Charisma, -150, 30),
                ]),
                raid_size: 1,
            },
        );
        species.insert(
            Species::Wyvern,
            SpeciesData {
                walk_ticks_per_voxel: 500,
                climb_ticks_per_voxel: None,
                flight_ticks_per_voxel: Some(200), // faster than hornet
                heartbeat_interval_ticks: 4000,
                allowed_edge_types: None,
                ground_only: false,
                hp_max: 100,
                ticks_per_hp_regen: 0,
                food_max: 1_000_000_000_000_000,
                food_decay_per_tick: 0,
                food_hunger_threshold_pct: 50,
                food_restore_pct: 0,
                bread_restore_pct: 0,
                footprint: [2, 2, 2],
                wood_ladder_tpv: None,
                rope_ladder_tpv: None,
                rest_max: 1_000_000_000_000_000,
                rest_decay_per_tick: 0,
                rest_tired_threshold_pct: 50,
                rest_per_sleep_tick: 60_000_000_000,
                melee_damage: 10,
                melee_interval_ticks: 1200,
                melee_range_sq: 5, // larger reach than 1x1 creatures
                engagement_style: EngagementStyle {
                    weapon_preference: WeaponPreference::PreferMelee,
                    ammo_exhausted: AmmoExhaustedBehavior::SwitchToMelee,
                    initiative: EngagementInitiative::Aggressive,
                    disengage_threshold_pct: 20, // flees when badly hurt
                },
                hostile_detection_range_sq: 225, // 15-voxel detection radius
                mp_max: 0,
                mana_per_tick: 0,
                stat_distributions: stat_dists(&[
                    (TraitKind::Strength, 200, 50),
                    (TraitKind::Agility, -68, 40),
                    (TraitKind::Dexterity, -50, 40),
                    (TraitKind::Constitution, 158, 50),
                    (TraitKind::Willpower, 50, 40),
                    (TraitKind::Intelligence, -100, 40),
                    (TraitKind::Perception, 42, 40),
                    (TraitKind::Charisma, -150, 40),
                ]),
                raid_size: 1,
            },
        );

        Self {
            tick_duration_ms: 1,
            tree_heartbeat_interval_ticks: 10000,
            mana_base_generation_rate_mm: 1000,
            mana_mood_multiplier_range_permille: (200, 2000),
            platform_mana_cost_per_mille: 20,
            default_mana_cost_per_mille: 20,
            grow_mana_cost_per_mille: 20,
            mana_abandon_threshold: 3,
            fruit_production_rate_ppm: 500_000,
            fruit_max_per_tree: 20,
            fruit_initial_attempts: 12,
            world_size: (1024, 255, 1024),
            floor_y: 50,
            starting_mana_mm: 100_000,
            starting_mana_capacity_mm: 500_000,
            build_work_ticks_per_voxel: 1000,
            carve_work_ticks_per_voxel: 1000,
            furnish_work_ticks_per_item: 2000,
            sleep_ticks_bed: 10_000,
            sleep_ticks_ground: 20_000,
            sleep_action_ticks: 1000,
            eat_action_ticks: 1500,
            harvest_action_ticks: 1500,
            acquire_item_action_ticks: 1000,
            haul_pickup_action_ticks: 1000,
            haul_dropoff_action_ticks: 1000,
            mope_action_ticks: 1000,
            tree_profile: TreeProfile::fantasy_mega(),
            lesser_trees: LesserTreeConfig::default(),
            species,
            terrain_max_height: 4,
            terrain_noise_scale: 8.0,
            structural: StructuralConfig::default(),
            thoughts: ThoughtConfig::default(),
            mood: MoodConfig::default(),
            mood_consequences: MoodConsequencesConfig::default(),
            logistics_heartbeat_interval_ticks: 5000,
            max_haul_tasks_per_heartbeat: 5,
            elf_starting_bread: 2,
            elf_starting_bows: 0,
            elf_starting_arrows: 0,
            elf_default_wants: default_elf_default_wants(),
            storehouse_default_priority: 2,
            storehouse_default_fruit_want: 10,
            storehouse_default_bread_want: 20,
            kitchen_default_priority: 8,
            kitchen_default_fruit_want: 5,
            kitchen_default_bread_target: 50,
            extract_work_ticks: 3000,
            component_recipes: ComponentRecipeConfig::default(),
            grow_recipes: GrowRecipeConfig::default(),
            initial_creatures: vec![
                InitialCreatureSpec {
                    species: Species::Elf,
                    count: 5,
                    spawn_position: VoxelCoord::new(512, 51, 512),
                    food_pcts: vec![100, 90, 70, 60, 48],
                    rest_pcts: vec![100, 95, 80, 60, 45],
                    bread_counts: vec![2, 2, 2, 2, 2],
                    initial_equipment: vec![
                        // Elf 0: red-dyed tunic + undyed oak boots.
                        vec![
                            InitialEquipSpec {
                                item_kind: ItemKind::Tunic,
                                material: None,
                                dye_color: Some(ItemColor::new(180, 40, 40)),
                            },
                            InitialEquipSpec {
                                item_kind: ItemKind::Boots,
                                material: Some(Material::Oak),
                                dye_color: None,
                            },
                        ],
                        // Elf 1: leggings only (no dye).
                        vec![InitialEquipSpec {
                            item_kind: ItemKind::Leggings,
                            material: None,
                            dye_color: None,
                        }],
                        // Elf 2: blue tunic + hat.
                        vec![
                            InitialEquipSpec {
                                item_kind: ItemKind::Tunic,
                                material: None,
                                dye_color: Some(ItemColor::new(50, 70, 180)),
                            },
                            InitialEquipSpec {
                                item_kind: ItemKind::Hat,
                                material: None,
                                dye_color: Some(ItemColor::new(50, 70, 180)),
                            },
                        ],
                        // Elf 3: no equipment (bare).
                        vec![],
                        // Elf 4: green-dyed leggings only.
                        vec![InitialEquipSpec {
                            item_kind: ItemKind::Leggings,
                            material: None,
                            dye_color: Some(ItemColor::new(50, 150, 50)),
                        }],
                    ],
                },
                InitialCreatureSpec {
                    species: Species::Capybara,
                    count: 5,
                    spawn_position: VoxelCoord::new(512, 51, 512),
                    food_pcts: vec![],
                    rest_pcts: vec![],
                    bread_counts: vec![],
                    initial_equipment: vec![],
                },
                InitialCreatureSpec {
                    species: Species::Boar,
                    count: 3,
                    spawn_position: VoxelCoord::new(512, 51, 512),
                    food_pcts: vec![],
                    rest_pcts: vec![],
                    bread_counts: vec![],
                    initial_equipment: vec![],
                },
                InitialCreatureSpec {
                    species: Species::Deer,
                    count: 3,
                    spawn_position: VoxelCoord::new(512, 51, 512),
                    food_pcts: vec![],
                    rest_pcts: vec![],
                    bread_counts: vec![],
                    initial_equipment: vec![],
                },
                InitialCreatureSpec {
                    species: Species::Squirrel,
                    count: 3,
                    spawn_position: VoxelCoord::new(512, 51, 512),
                    food_pcts: vec![],
                    rest_pcts: vec![],
                    bread_counts: vec![],
                    initial_equipment: vec![],
                },
            ],
            initial_ground_piles: vec![
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Bread,
                    quantity: 5,
                    material: None,
                    dye_color: None,
                },
                // Undyed tunics.
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Tunic,
                    quantity: 1,
                    material: None,
                    dye_color: None,
                },
                // Purple-dyed tunic.
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Tunic,
                    quantity: 1,
                    material: None,
                    dye_color: Some(ItemColor::new(130, 50, 160)),
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Leggings,
                    quantity: 2,
                    material: None,
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Boots,
                    quantity: 2,
                    material: None,
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Hat,
                    quantity: 2,
                    material: None,
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Gloves,
                    quantity: 2,
                    material: None,
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Bow,
                    quantity: 2,
                    material: None,
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Arrow,
                    quantity: 30,
                    material: None,
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Helmet,
                    quantity: 1,
                    material: Some(Material::Oak),
                    dye_color: None,
                },
                // Red-dyed oak breastplate.
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Breastplate,
                    quantity: 1,
                    material: Some(Material::Oak),
                    dye_color: Some(ItemColor::new(180, 40, 40)),
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Greaves,
                    quantity: 1,
                    material: Some(Material::Oak),
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Gauntlets,
                    quantity: 1,
                    material: Some(Material::Oak),
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(512, 51, 522),
                    item_kind: ItemKind::Boots,
                    quantity: 1,
                    material: Some(Material::Oak),
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(128, 1, 138),
                    item_kind: ItemKind::Spear,
                    quantity: 3,
                    material: Some(Material::Oak),
                    dye_color: None,
                },
                InitialGroundPileSpec {
                    position: VoxelCoord::new(128, 1, 138),
                    item_kind: ItemKind::Club,
                    quantity: 3,
                    material: Some(Material::Oak),
                    dye_color: None,
                },
            ],
            workshop_default_priority: default_workshop_priority(),
            greenhouse_default_priority: default_greenhouse_default_priority(),
            greenhouse_base_production_ticks: default_greenhouse_base_production_ticks(),
            worldgen: crate::worldgen::WorldgenConfig::default(),
            arrow_gravity: default_arrow_gravity(),
            arrow_base_speed: default_arrow_base_speed(),
            arrow_base_deviation_ppm: default_arrow_base_deviation_ppm(),
            arrow_damage_multiplier: default_arrow_damage_multiplier(),
            shoot_cooldown_ticks: default_shoot_cooldown_ticks(),
            attack_path_retry_limit: default_attack_path_retry_limit(),
            defensive_pursuit_range_sq: default_defensive_pursuit_range_sq(),
            voxel_exclusion_retry_ticks: default_voxel_exclusion_retry_ticks(),
            item_durability: default_item_durability(),
            durability_worn_pct: default_durability_worn_pct(),
            durability_damaged_pct: default_durability_damaged_pct(),
            arrow_impact_damage_min: default_arrow_impact_damage_min(),
            arrow_impact_damage_max: default_arrow_impact_damage_max(),
            armor_min_damage: default_armor_min_damage(),
            armor_non_penetrating_degrade_chance_recip:
                default_armor_non_penetrating_degrade_chance_recip(),
            armor_worn_penalty: default_armor_worn_penalty(),
            armor_damaged_penalty: default_armor_damaged_penalty(),
            armor_degrade_location_weights: default_armor_degrade_location_weights(),
            spear_base_damage: default_spear_base_damage(),
            spear_melee_range_sq: default_spear_melee_range_sq(),
            club_base_damage: default_club_base_damage(),
            club_melee_range_sq: default_club_melee_range_sq(),
            melee_weapon_impact_damage_min: default_melee_weapon_impact_damage_min(),
            melee_weapon_impact_damage_max: default_melee_weapon_impact_damage_max(),
            fall_damage_per_voxel: default_fall_damage_per_voxel(),
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
        // Verify species data survived (12 species).
        assert_eq!(config.species.len(), 12);
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
            "mana_base_generation_rate_mm": 1000,
            "mana_mood_multiplier_range_permille": [200, 2000],
            "platform_mana_cost_per_mille": 20,

            "fruit_production_rate_ppm": 500000,
            "fruit_max_per_tree": 20,
            "fruit_initial_attempts": 12,
            "build_work_ticks_per_voxel": 1000,
            "world_size": [1024, 255, 1024],
                        "floor_y": 50,
            "starting_mana_mm": 100000,
            "starting_mana_capacity_mm": 500000,
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
            "mana_base_generation_rate_mm": 2000,
            "mana_mood_multiplier_range_permille": [100, 3000],
            "platform_mana_cost_per_mille": 16,

            "fruit_production_rate_ppm": 800000,
            "fruit_max_per_tree": 25,
            "fruit_initial_attempts": 15,
            "build_work_ticks_per_voxel": 2000,
            "world_size": [128, 64, 128],
                        "starting_mana_mm": 200000,
            "starting_mana_capacity_mm": 1000000,
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
                    "allowed_edge_types": ["Ground"],
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
    fn config_backward_compat_floor_y() {
        // Old configs without "floor_y" should deserialize with the default (50).
        let json = r#"{
            "tick_duration_ms": 1,
            "tree_heartbeat_interval_ticks": 10000,
            "mana_base_generation_rate_mm": 1000,
            "mana_mood_multiplier_range_permille": [200, 2000],
            "platform_mana_cost_per_mille": 20,

            "fruit_production_rate_ppm": 500000,
            "fruit_max_per_tree": 20,
            "fruit_initial_attempts": 12,
            "build_work_ticks_per_voxel": 1000,
            "world_size": [1024, 255, 1024],
                        "starting_mana_mm": 100000,
            "starting_mana_capacity_mm": 500000,
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
        assert_eq!(config.floor_y, 50, "Missing floor_y should default to 50");
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
