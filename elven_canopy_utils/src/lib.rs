//! Shared utility algorithms for Elven Canopy crates.
//!
//! Provides:
//! - `parallel_dedup`: radix-partitioned parallel deduplication (rayon +
//!   hashbrown). Used by `elven_canopy_sim` and `tabulosity`.
//! - `fixed`: deterministic fixed-point arithmetic types (`Fixed64` scalar,
//!   `FixedVec3` 3D vector, `isqrt_i128`). Used by `elven_canopy_sim`
//!   (projectile ballistics) and `elven_canopy_music` (composition scoring).

pub mod fixed;
mod parallel_dedup;

pub use parallel_dedup::{parallel_dedup, parallel_dedup_with, sequential_dedup};
