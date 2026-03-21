//! Shared utility algorithms for Elven Canopy crates.
//!
//! Currently provides `parallel_dedup`, a radix-partitioned parallel
//! deduplication algorithm built on rayon and hashbrown. The approach:
//! partition items by hash into buckets (parallel scatter), then dedup
//! each bucket independently (parallel gather). This avoids all
//! cross-thread contention — each phase touches only thread-local data.
//!
//! Used by `elven_canopy_sim` and `tabulosity` for bulk dedup on the
//! rayon worker pool.

mod parallel_dedup;

pub use parallel_dedup::{parallel_dedup, parallel_dedup_with, sequential_dedup};
